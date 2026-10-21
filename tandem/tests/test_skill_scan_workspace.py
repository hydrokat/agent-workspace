import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

import pytest

SCRIPT = Path(__file__).resolve().parents[2] / "templates/skills/workspace-cleanup/scripts/scan_workspace.py"
GIT_ENV = {**os.environ, "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null",
           "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com",
           "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com"}
DAY = 86400


def git(repo: Path, *args: str) -> None:
    subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True, env=GIT_ENV)


def put(root: Path, rel: str, content: str = "x\n", age_days: float = 0) -> Path:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)
    if age_days:
        stamp = time.time() - age_days * DAY
        os.utime(path, (stamp, stamp))
    return path


def run(root: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run([sys.executable, str(SCRIPT), "--root", str(root), *args], capture_output=True, text=True)


def out(proc: subprocess.CompletedProcess[str]) -> dict[str, Any]:
    assert proc.returncode == 0, proc.stdout + proc.stderr
    data: dict[str, Any] = json.loads(proc.stdout)
    return data


def err(proc: subprocess.CompletedProcess[str], code: int) -> dict[str, Any]:
    assert proc.returncode == code, proc.stdout + proc.stderr
    data: dict[str, Any] = json.loads(proc.stdout)
    return data


@pytest.fixture
def ws(tmp_path: Path) -> Path:
    root = tmp_path / "ws"
    root.mkdir()
    git(root, "init", "-q", "-b", "feature")
    put(root, "AGENTS.md", "# Agents\n\n### Core Directory Map\n- `/src`: code\n- `/docs`: docs\n- `/specs`\n")
    put(root, ".gitignore", "*.bak\nbuild.log\nnode_modules/\n")
    put(root, "README.md")
    put(root, "src/app.py")
    put(root, ".vscode/settings.json")
    put(root, ".agents/skills/keep/SKILL.md")
    put(root, ".agents/agents/real/AGENT.md")
    put(root, "specs/in-progress.md", "- working on .tmp/active.txt\n")
    put(root, "specs/p-001-x/impl.md")
    put(root, "knowledgebase/context-history/h.md")
    git(root, "add", "-A")
    git(root, "commit", "-q", "-m", "base")
    put(root, ".DS_Store")
    put(root, "src/app.py.swp")
    put(root, "notes~")
    put(root, ".idea/workspace.xml")
    put(root, "old.md.bak")
    put(root, "knowledgebase/guidelines/security.md.bak")
    put(root, "specs/p-001-x/impl.md.bak")
    put(root, "knowledgebase/context-history/h.md.bak")
    (root / "scratch-empty").mkdir()
    (root / "docs").mkdir()
    (root / "empty-nested/a/b").mkdir(parents=True)
    put(root, ".claude/skills/orphan-skill/SKILL.md")
    put(root, ".claude/skills/keep/SKILL.md")
    put(root, ".claude/agents/ghost.md")
    put(root, ".claude/agents/real.md")
    put(root, ".claude/settings.local.json", "{}")
    put(root, ".tmp/old.txt", age_days=60)
    put(root, ".tmp/new.txt")
    put(root, ".tmp/active.txt", age_days=60)
    put(root, "random-notes.txt")
    put(root, "build.log")
    put(root, "node_modules/x/.DS_Store")
    return root


def found(data: dict[str, Any]) -> dict[str, str]:
    return {c["path"]: c["category"] for c in data["candidates"]}


def test_scan_lists_each_category(ws: Path) -> None:
    got = found(out(run(ws)))
    assert got[".DS_Store"] == "os-editor-cruft"
    assert got["src/app.py.swp"] == "os-editor-cruft" and got["notes~"] == "os-editor-cruft"
    assert got[".idea"] == "os-editor-cruft"
    assert got["old.md.bak"] == "stale-backup" and got["knowledgebase/guidelines/security.md.bak"] == "stale-backup"
    assert got["scratch-empty"] == "empty-directory" and got["empty-nested"] == "empty-directory"
    assert got[".claude/skills/orphan-skill"] == "orphaned-generated-output"
    assert got[".claude/agents/ghost.md"] == "orphaned-generated-output"
    assert got[".tmp/old.txt"] == "stale-scratch"
    assert got["random-notes.txt"] == "unrecognized-file"


def test_scan_never_flags_do_not_flag_content(ws: Path) -> None:
    got = found(out(run(ws)))
    for path in (".vscode", ".vscode/settings.json", "docs", "empty-nested/a", ".claude/skills/keep",
                 ".claude/agents/real.md", ".claude/settings.local.json", ".tmp/new.txt", ".tmp/active.txt",
                 "build.log", "README.md", "src/app.py", "specs/p-001-x/impl.md.bak",
                 "knowledgebase/context-history/h.md.bak", "node_modules/x/.DS_Store", ".git", "AGENTS.md"):
        assert path not in got, path
    assert not [p for p in got if p.startswith(("specs", "node_modules", ".git/"))]


def test_unrecognized_files_need_review_and_every_candidate_has_a_reason(ws: Path) -> None:
    data = out(run(ws))
    by_path = {c["path"]: c for c in data["candidates"]}
    assert by_path["random-notes.txt"]["needs_review"] is True
    assert by_path[".DS_Store"]["needs_review"] is False
    assert all(c["reason"] and c["modified"] for c in data["candidates"])
    assert data["counts"]["stale-backup"] == 2


def test_stale_days_controls_scratch_age(ws: Path) -> None:
    assert ".tmp/old.txt" in found(out(run(ws, "--stale-days", "30")))
    assert ".tmp/old.txt" not in found(out(run(ws, "--stale-days", "90")))
    err(run(ws, "--stale-days", "0"), 2)


def test_scan_works_without_git(tmp_path: Path) -> None:
    root = tmp_path / "plain"
    root.mkdir()
    put(root, "AGENTS.md", "- `/src`\n")
    put(root, "src/a.py")
    put(root, ".DS_Store")
    put(root, "mystery.dat")
    got = found(out(run(root)))
    assert got == {".DS_Store": "os-editor-cruft", "mystery.dat": "unrecognized-file"}


def test_scan_does_not_follow_symlinked_directories(ws: Path, tmp_path: Path) -> None:
    outside = tmp_path / "outside"
    put(outside, ".DS_Store")
    (ws / "codebase-symlinks").mkdir()
    (ws / "codebase-symlinks" / "other").symlink_to(outside)
    (ws / "linked").symlink_to(outside)
    got = found(out(run(ws)))
    assert not [p for p in got if p.startswith(("codebase-symlinks", "linked/"))]


def delete(ws: Path, *paths: str) -> subprocess.CompletedProcess[str]:
    return run(ws, "--delete", *paths)


def test_delete_removes_only_approved_scanned_paths(ws: Path) -> None:
    out(run(ws))
    data = out(delete(ws, ".DS_Store", "old.md.bak", ".idea", "scratch-empty", ".claude/skills/orphan-skill"))
    assert sorted(data["deleted"]) == sorted([".DS_Store", "old.md.bak", ".idea", "scratch-empty", ".claude/skills/orphan-skill"])
    for gone in (".DS_Store", "old.md.bak", ".idea", "scratch-empty", ".claude/skills/orphan-skill"):
        assert not (ws / gone).exists()
    assert (ws / "notes~").exists() and (ws / "random-notes.txt").exists() and (ws / "empty-nested").exists()


def test_delete_refuses_paths_that_were_not_scanned(ws: Path) -> None:
    out(run(ws))
    data = err(delete(ws, "README.md", "src/app.py", "../outside", "/etc/hosts", ".DS_Store"), 1)
    assert data["deleted"] == [".DS_Store"]
    assert {r["path"] for r in data["refused"]} == {"README.md", "src/app.py", "../outside", "/etc/hosts"}
    assert (ws / "README.md").exists() and (ws / "src/app.py").exists()


def test_delete_requires_a_prior_scan(ws: Path) -> None:
    err(delete(ws, ".DS_Store"), 1)
    assert (ws / ".DS_Store").exists()


def test_delete_refuses_a_second_time_and_after_the_file_changed(ws: Path) -> None:
    out(run(ws))
    out(delete(ws, ".DS_Store"))
    err(delete(ws, ".DS_Store"), 1)
    (ws / "notes~").write_text("changed after the scan, longer than before\n")
    data = err(delete(ws, "notes~"), 1)
    assert "changed" in data["refused"][0]["reason"] and (ws / "notes~").exists()


def test_delete_empty_directory_refuses_when_it_is_no_longer_empty(ws: Path) -> None:
    out(run(ws))
    put(ws, "scratch-empty/new.txt")
    data = err(delete(ws, "scratch-empty"), 1)
    assert (ws / "scratch-empty/new.txt").exists() and data["refused"]


def test_delete_never_follows_symlinks(ws: Path, tmp_path: Path) -> None:
    victim = tmp_path / "victim"
    put(victim, "keep.txt")
    (ws / "link-out").symlink_to(victim)
    got = found(out(run(ws)))
    assert got["link-out"] == "unrecognized-file"
    out(delete(ws, "link-out"))
    assert not (ws / "link-out").is_symlink() and (victim / "keep.txt").exists()


def test_delete_refuses_protected_paths_even_if_a_forged_scan_lists_them(ws: Path) -> None:
    out(run(ws))
    scan_file = ws / ".agent-workspace/cleanup-scan.json"
    data = json.loads(scan_file.read_text())
    for rel in ("specs/p-001-x/impl.md", "knowledgebase/context-history/h.md", "knowledgebase/context-history/h.md.bak",
                "specs/p-001-x/impl.md.bak", ".git/HEAD", "AGENTS.md"):
        stat = (ws / rel).lstat()
        data["candidates"].append({"path": rel, "kind": "file", "category": "stale-backup",
                                   "mtime_ns": stat.st_mtime_ns, "size": stat.st_size})
    scan_file.write_text(json.dumps(data))
    result = err(delete(ws, "specs/p-001-x/impl.md", "knowledgebase/context-history/h.md",
                        "knowledgebase/context-history/h.md.bak", "specs/p-001-x/impl.md.bak", ".git/HEAD", "AGENTS.md"), 1)
    assert result["deleted"] == [] and len(result["refused"]) == 6
    assert all((ws / r).exists() for r in ("specs/p-001-x/impl.md", "knowledgebase/context-history/h.md", "AGENTS.md"))


def test_delete_in_knowledgebase_is_limited_to_bak_files(ws: Path) -> None:
    out(run(ws))
    scan_file = ws / ".agent-workspace/cleanup-scan.json"
    data = json.loads(scan_file.read_text())
    plain = ws / "knowledgebase/guidelines/real.md"
    put(ws, "knowledgebase/guidelines/real.md")
    stat = plain.lstat()
    data["candidates"].append({"path": "knowledgebase/guidelines/real.md", "kind": "file", "category": "unrecognized-file",
                               "mtime_ns": stat.st_mtime_ns, "size": stat.st_size})
    scan_file.write_text(json.dumps(data))
    err(delete(ws, "knowledgebase/guidelines/real.md"), 1)
    assert plain.exists()
    out(delete(ws, "knowledgebase/guidelines/security.md.bak"))
    assert not (ws / "knowledgebase/guidelines/security.md.bak").exists()


def test_delete_rejects_a_corrupt_scan_file(ws: Path) -> None:
    out(run(ws))
    (ws / ".agent-workspace/cleanup-scan.json").write_text("not json")
    err(delete(ws, ".DS_Store"), 1)
    assert (ws / ".DS_Store").exists()


def test_root_must_be_a_directory(tmp_path: Path) -> None:
    err(run(tmp_path / "missing"), 2)
