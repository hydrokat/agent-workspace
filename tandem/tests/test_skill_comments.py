import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

import pytest

SCRIPT = Path(__file__).resolve().parents[2] / "templates/skills/code-cleanup/scripts/comments.py"
FIXTURES = Path(__file__).resolve().parent / "fixtures/comments"
GIT_ENV = {**os.environ, "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null",
           "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com",
           "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com"}


def git(repo: Path, *args: str) -> None:
    subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True, env=GIT_ENV)


def make_repo(path: Path, files: dict[str, str], branch: str = "feature") -> Path:
    path.mkdir(parents=True, exist_ok=True)
    git(path, "init", "-q", "-b", branch)
    for name, content in files.items():
        target = path / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(content.encode())
    git(path, "add", "-f", "-A")
    return path


def run(repo: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run([sys.executable, str(SCRIPT), "--root", str(repo), *args],
                          capture_output=True, text=True, cwd=repo)


def out(proc: subprocess.CompletedProcess[str]) -> dict[str, Any]:
    assert proc.returncode == 0, proc.stdout + proc.stderr
    data: dict[str, Any] = json.loads(proc.stdout)
    return data


def err(proc: subprocess.CompletedProcess[str], code: int) -> dict[str, Any]:
    assert proc.returncode == code, proc.stdout + proc.stderr
    data: dict[str, Any] = json.loads(proc.stdout)
    return data


def fixture_repo(tmp_path: Path) -> Path:
    files = {f.name: f.read_text() for f in FIXTURES.iterdir()}
    files["node_modules/dep/x.js"] = "// vendored comment\n"
    files["package-lock.json"] = "{}\n"
    return make_repo(tmp_path / "repo", files)


def summary(data: dict[str, Any], name: str) -> list[tuple[str, str, bool]]:
    return [(c["text"], c["kind"], c["trailing"]) for c in data["candidates"] if c["file"] == name]


@pytest.fixture
def scanned(tmp_path: Path) -> dict[str, Any]:
    return out(run(fixture_repo(tmp_path), "scan", "--include-other", "--limit", "500"))


EXPECTED = {
    "sample.py": [
        ("# increment counter", "restates-code", False),
        ("# increment counter", "restates-code", True),
        ("# x = old_function(3)", "commented-out-code", False),
        ("# ----------------------------", "divider", False),
        ("#", "empty", False),
        ("# Step 1: load the config", "narration", False),
        ("# TODO: handle retries", "stale-todo", False),
        ("# Retry because the upstream API rate-limits bursts.", "other", False),
    ],
    "sample.js": [
        ("// const old = compute(1);", "commented-out-code", False),
        ("// half of total", "restates-code", True),
        ("// return x", "restates-code", True),
        ("/* Retry on 429. */", "other", False),
        ("// ---------- section ----------", "divider", False),
    ],
    "sample.go": [
        ("// helper does stuff", "other", False),
        ("// fmt.Println(x)", "commented-out-code", False),
        ("// trailing", "other", True),
        ("// rune", "other", True),
    ],
    "sample.rs": [
        ("// char slash", "other", True),
        ("/* nested /* inner */ still comment */", "other", False),
        ("// let x = old(1);", "commented-out-code", False),
    ],
    "Sample.java": [
        ("// System.out.println(s);", "commented-out-code", False),
        ("/* plain block */", "other", False),
    ],
    "sample.css": [("/* color: red; */", "commented-out-code", False)],
    "sample.sh": [
        ("# trailing note", "other", True),
        ("# Step 2: cleanup", "narration", False),
    ],
    "sample.yaml": [
        ("# trailing", "other", True),
        ("# ----- section -----", "divider", False),
    ],
    "sample.rb": [("# x = old(1)", "commented-out-code", False)],
    "sample.php": [
        ("// $x = foo();", "commented-out-code", False),
        ("# note", "other", False),
    ],
    "sample.toml": [("# real", "other", True)],
}


@pytest.mark.parametrize("name", sorted(EXPECTED))
def test_scan_lists_exactly_the_unneeded_comments(scanned: dict[str, Any], name: str) -> None:
    assert summary(scanned, name) == EXPECTED[name]


def test_scan_never_lists_text_inside_strings_regexes_or_heredocs(scanned: dict[str, Any]) -> None:
    listed = " ".join(c["text"] for c in scanned["candidates"])
    for forbidden in ("not a comment", "# also", "//path", "not comment", "nor //", "inner */}", "heredoc content",
                      "shell comment in block", "raw // string", "a // b", "#frag", "#anchor", "x.example"):
        assert forbidden not in listed


def test_auto_kept_comments_are_counted_not_listed(scanned: dict[str, Any]) -> None:
    kept = scanned["kept"]
    assert kept["shebang"] == 2 and kept["doc"] >= 6 and kept["directive"] >= 4
    assert kept["tracked-todo"] == 1
    listed = " ".join(c["text"] for c in scanned["candidates"])
    for forbidden in ("coding", "noqa", "type: ignore", "eslint-disable", "JSDoc kept", "@OA",
                      "Package demo", "Exported is documented", "doc kept", "crate doc", "tracked", "ana"):
        assert forbidden not in listed


def test_skipped_files_are_reported(scanned: dict[str, Any]) -> None:
    assert scanned["skipped"]["unsupported"] == {".sql": 1}
    assert scanned["skipped"]["ignored"] == 2
    assert not any(c["file"].startswith("node_modules") for c in scanned["candidates"])


def test_other_is_hidden_unless_requested(tmp_path: Path) -> None:
    repo = fixture_repo(tmp_path)
    data = out(run(repo, "scan", "--limit", "500"))
    assert all(c["kind"] != "other" for c in data["candidates"])
    assert data["counts"]["other"] > 0


def test_ids_are_sequential_and_scan_paginates(tmp_path: Path) -> None:
    repo = fixture_repo(tmp_path)
    first = out(run(repo, "scan", "--include-other", "--limit", "5"))
    assert [c["id"] for c in first["candidates"]] == ["c1", "c2", "c3", "c4", "c5"]
    assert first["next_offset"] == 5 and first["total"] > 5
    second = out(run(repo, "scan", "--include-other", "--limit", "5", "--offset", "5"))
    assert second["candidates"][0]["id"] == "c6"
    err(run(repo, "scan", "--limit", "9999"), 2)


def test_scan_limits_to_given_paths(tmp_path: Path) -> None:
    repo = fixture_repo(tmp_path)
    data = out(run(repo, "scan", "sample.py", "--include-other"))
    assert {c["file"] for c in data["candidates"]} == {"sample.py"}


def test_path_outside_repo_is_refused(tmp_path: Path) -> None:
    repo = fixture_repo(tmp_path)
    err(run(repo, "scan", "../elsewhere"), 2)
    err(run(repo, "scan", "/etc"), 2)


def test_diff_scope_limits_candidates_to_changed_lines(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "repo", {"a.py": "# old comment here\nx = 1\n"})
    git(repo, "commit", "-q", "-m", "base")
    (repo / "a.py").write_text("# old comment here\nx = 1\n# new narration comment\ny = 2  # new trailing\n")
    data = out(run(repo, "scan", "--diff", "HEAD", "--include-other"))
    assert [c["text"] for c in data["candidates"]] == ["# new narration comment", "# new trailing"]
    err(run(repo, "scan", "--diff", "nope"), 2)
    err(run(repo, "scan", "--diff=--upload-pack=x"), 2)


def test_scan_outside_git_repo(tmp_path: Path) -> None:
    plain = tmp_path / "plain"
    plain.mkdir()
    err(run(plain, "scan"), 1)


def plan(repo: Path, tmp_path: Path, **selection: Any) -> subprocess.CompletedProcess[str]:
    scan_file = tmp_path / "scan.json"
    out(run(repo, "scan", "--include-other", "--limit", "500", "--out", str(scan_file)))
    plan_file = tmp_path / "plan.json"
    plan_file.write_text(json.dumps({"scan": str(scan_file), **selection}))
    return run(repo, "remove", "--plan", str(plan_file))


def test_removes_whole_line_and_trailing_comments(tmp_path: Path) -> None:
    source = "import os\n\n# increment counter\ncounter = 0\ncounter += 1  # increment counter\nname = 'a'  # note\n"
    repo = make_repo(tmp_path / "repo", {"a.py": source})
    data = out(plan(repo, tmp_path, all=True))
    assert data["removed"] == 3
    assert (repo / "a.py").read_text() == "import os\n\ncounter = 0\ncounter += 1\nname = 'a'\n"
    compile((repo / "a.py").read_text(), "a.py", "exec")


def test_selection_by_id_kind_and_exclude(tmp_path: Path) -> None:
    source = "a = 1\n# x = old(1)\n# Step 1: go\n# ------------\nb = 2\n"
    repo = make_repo(tmp_path / "repo", {"a.py": source})
    scan = out(run(repo, "scan", "--include-other"))
    ids = {c["kind"]: c["id"] for c in scan["candidates"]}
    out(plan(repo, tmp_path, kinds=["divider"], ids=[ids["narration"]], exclude=[ids["narration"]]))
    assert (repo / "a.py").read_text() == "a = 1\n# x = old(1)\n# Step 1: go\nb = 2\n"


def test_double_blank_line_is_collapsed_only_when_created(tmp_path: Path) -> None:
    source = "a = 1\n\n# Step 1: go\n\nb = 2\n# Step 2: go\nc = 3\n"
    repo = make_repo(tmp_path / "repo", {"a.py": source})
    out(plan(repo, tmp_path, all=True))
    assert (repo / "a.py").read_text() == "a = 1\n\nb = 2\nc = 3\n"


def test_crlf_line_endings_are_preserved(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "repo", {"a.js": "let a = 1; // a is one\r\n// Step 1: go\r\nlet b = 2;\r\n"})
    out(plan(repo, tmp_path, all=True))
    assert (repo / "a.js").read_bytes() == b"let a = 1;\r\nlet b = 2;\r\n"


def test_removing_nothing_leaves_files_byte_identical(tmp_path: Path) -> None:
    repo = fixture_repo(tmp_path)
    before = {f.name: f.read_bytes() for f in repo.iterdir() if f.is_file()}
    err(plan(repo, tmp_path, ids=[]), 2)
    after = {f.name: f.read_bytes() for f in repo.iterdir() if f.is_file()}
    assert before == after


def test_fixture_python_still_compiles_after_removing_everything(tmp_path: Path) -> None:
    repo = fixture_repo(tmp_path)
    out(plan(repo, tmp_path, all=True))
    compile((repo / "sample.py").read_text(), "sample.py", "exec")
    text = (repo / "sample.py").read_text()
    assert "# noqa: F401" in text and "type: ignore" in text and "# not a comment" in text
    assert "# increment counter" not in text and "#!/usr/bin/env python3" in text
    assert 'url = "http://example.com/#frag"' in text
    js = (repo / "sample.js").read_text()
    assert "/\\/\\/ not comment/g" in js and "template // not" in js and "eslint-disable" in js


def test_drift_leaves_the_file_untouched_and_exits_one(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "repo", {"a.py": "x = 1\n# Step 1: go\ny = 2\n", "b.py": "# Step 1: go\nz = 3\n"})
    scan_file = tmp_path / "scan.json"
    out(run(repo, "scan", "--out", str(scan_file)))
    (repo / "a.py").write_text("x = 1\ny = 2\n# Step 1: go\n")
    plan_file = tmp_path / "plan.json"
    plan_file.write_text(json.dumps({"scan": str(scan_file), "all": True}))
    data = err(run(repo, "remove", "--plan", str(plan_file)), 1)
    status = {f["file"]: f["status"] for f in data["files"]}
    assert status["a.py"].startswith("drift") and status["b.py"] == "removed"
    assert (repo / "a.py").read_text() == "x = 1\ny = 2\n# Step 1: go\n"
    assert (repo / "b.py").read_text() == "z = 3\n"


def test_plan_cannot_reach_files_outside_the_repo_or_untracked(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "repo", {"a.py": "x = 1\n"})
    victim = tmp_path / "victim.py"
    victim.write_text("# Step 1: go\n")
    (repo / "link.py").symlink_to(victim)
    (repo / "untracked.py").write_text("# Step 1: go\n")
    scan_file = tmp_path / "scan.json"
    base = {"id": "c1", "start": 1, "end": 1, "kind": "narration", "reason": "r", "text": "# Step 1: go",
            "trailing": False, "col": 0, "source": "# Step 1: go"}
    scan_file.write_text(json.dumps({"candidates": [
        {**base, "file": "../victim.py"}, {**base, "id": "c2", "file": "link.py"},
        {**base, "id": "c3", "file": "untracked.py"}, {**base, "id": "c4", "file": "/etc/hosts"},
    ]}))
    plan_file = tmp_path / "plan.json"
    plan_file.write_text(json.dumps({"scan": str(scan_file), "all": True}))
    data = err(run(repo, "remove", "--plan", str(plan_file)), 1)
    assert data["removed"] == 0 and all(f["status"].startswith("refused") for f in data["files"])
    assert victim.read_text() == "# Step 1: go\n" and (repo / "untracked.py").read_text() == "# Step 1: go\n"


@pytest.mark.parametrize("content", ["not json", "[]", '{"scan": 5}', '{"scan": "/nonexistent", "all": true}',
                                     '{"scan": "x", "kinds": ["bogus"]}'])
def test_malformed_plans_are_rejected(tmp_path: Path, content: str) -> None:
    repo = make_repo(tmp_path / "repo", {"a.py": "x = 1\n"})
    plan_file = tmp_path / "plan.json"
    plan_file.write_text(content)
    err(run(repo, "remove", "--plan", str(plan_file)), 2)


def test_oversized_plan_is_rejected(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "repo", {"a.py": "x = 1\n"})
    plan_file = tmp_path / "plan.json"
    with plan_file.open("wb") as handle:
        handle.truncate(21_000_000)
    err(run(repo, "remove", "--plan", str(plan_file)), 2)


def test_selecting_unknown_ids_is_rejected(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "repo", {"a.py": "x = 1\n# Step 1: go\n"})
    err(plan(repo, tmp_path, ids=["c99"]), 2)


def test_protected_branch_is_refused(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "repo", {"a.py": "x = 1\n# Step 1: go\n"}, branch="main")
    data = err(plan(repo, tmp_path, all=True), 1)
    assert data["branch"] == "main"
    assert (repo / "a.py").read_text() == "x = 1\n# Step 1: go\n"
    scan_file = tmp_path / "scan.json"
    plan_file = tmp_path / "plan2.json"
    plan_file.write_text(json.dumps({"scan": str(scan_file), "all": True}))
    out(run(repo, "remove", "--plan", str(plan_file), "--allow-protected-branch"))
    assert (repo / "a.py").read_text() == "x = 1\n"


def test_python_that_would_not_parse_is_refused(tmp_path: Path) -> None:
    source = "def f():\n    # Step 1: go\n    pass\n"
    repo = make_repo(tmp_path / "repo", {"a.py": source})
    scan_file = tmp_path / "scan.json"
    out(run(repo, "scan", "--out", str(scan_file)))
    data = json.loads(scan_file.read_text())
    data["candidates"][0].update({"start": 1, "end": 1, "source": "def f():", "text": "def f():"})
    scan_file.write_text(json.dumps(data))
    plan_file = tmp_path / "plan.json"
    plan_file.write_text(json.dumps({"scan": str(scan_file), "all": True}))
    result = run(repo, "remove", "--plan", str(plan_file))
    assert result.returncode == 1 and (repo / "a.py").read_text() == source


def test_file_modes_survive_removal(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "repo", {"run.sh": "#!/bin/sh\n# Step 1: go\necho hi\n"})
    os.chmod(repo / "run.sh", 0o755)
    out(plan(repo, tmp_path, all=True))
    assert (repo / "run.sh").stat().st_mode & 0o777 == 0o755
    assert (repo / "run.sh").read_text() == "#!/bin/sh\necho hi\n"


def test_unterminated_constructs_do_not_hang_or_crash(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "repo", {
        "a.js": "const s = `open template ${\n", "b.rs": "fn f() { let r = r#\"open\n",
        "c.py": "x = (\n", "d.go": "/* never closed\n", "e.c": "char *p = \"open\n// c\n",
    })
    out(run(repo, "scan", "--include-other"))


def test_tricky_javascript_survives_removal(tmp_path: Path) -> None:
    source = (
        "const a = (1 + 2) / 3; // Step 1: divide\n"
        "const b = [4][0] / 2 / 1; // Step 2: again\n"
        "const c = `x ${true ? `y ${a}` : \"z\"} // inside template`;\n"
        "const d = /[/]\\/\\/ not a comment/.test(c); // Step 3: test\n"
        "const e = '\"' + \"'\"; // Step 4: quotes\n"
        "const f = x => x /* mid */ + 1;\n"
    )
    repo = make_repo(tmp_path / "repo", {"t.js": source})
    data = out(run(repo, "scan", "--include-other"))
    assert [c["text"] for c in data["candidates"]] == [
        "// Step 1: divide", "// Step 2: again", "// Step 3: test", "// Step 4: quotes",
    ]
    out(plan(repo, tmp_path, all=True))
    result = (repo / "t.js").read_text()
    assert "// inside template" in result and "/* mid */" in result and "[/]" in result
    node = shutil.which("node")
    if node:
        subprocess.run([node, "--check", str(repo / "t.js")], check=True, capture_output=True)


def test_php_multiline_strings_and_inline_html_are_not_comments(tmp_path: Path) -> None:
    source = (
        "<html>don't // touch # this</html>\n<?php\n$p = '~^\n  (a)://  # protocol\n  $~';\n"
        "$q = \"line\n##END##\";\n// Step 1: go\n?>\n<p>it's # fine // really</p>\n<?php // Step 2: more ?>\n"
    )
    repo = make_repo(tmp_path / "repo", {"a.php": source, "b.blade.php": "{{-- Step 1: go --}}\n<?php // Step 1: go\n"})
    data = out(run(repo, "scan", "--include-other"))
    assert [c["text"] for c in data["candidates"]] == ["// Step 1: go"]
    assert data["skipped"]["unsupported"] == {".php": 1}


def test_rust_multiline_string_hides_comment_markers(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "repo", {"a.rs": 'let s = "first\n// not a comment\nlast";\n// Step 1: go\n'})
    data = out(run(repo, "scan", "--include-other"))
    assert [c["text"] for c in data["candidates"]] == ["// Step 1: go"]
