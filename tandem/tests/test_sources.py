import os
import subprocess
from pathlib import Path

from tandem.coordination.schema import Claim
from tandem.sanitize import clean
from tandem.sources.codebases import list_codebases
from tandem.sources.conflicts import find_conflicts
from tandem.sources.gitinfo import git_info
from tandem.sources.inprogress import parse_in_progress
from tandem.sources.specs import load_phases

IMPL = """# Implementation Plan: Thing

## Timeline
- **t-001**: First (Done)
- **t-002**: Second (In Progress)
- **t-003**: Third (Pending)

## Tasks
- [x] t-001: [New Feature] First
- [ ] t-002: [New Feature] Second
- [ ] t-003: [New Feature] Third
"""


def phase(root: Path, name: str = "p-001-thing", statuses: dict[str, str] | None = None) -> Path:
    d = root / "specs" / name
    d.mkdir(parents=True)
    (d / "impl.md").write_text(IMPL)
    for task, status in (statuses or {"t-001": "Done", "t-002": "In Progress"}).items():
        (d / f"{task}.md").write_text(f"# {task}: x\n\n## Status\n- [x] {status}\n")
    return d


def test_phase_progress_from_task_files(tmp_path: Path) -> None:
    phase(tmp_path)
    [p] = load_phases(tmp_path)
    assert p.name == "p-001-thing" and (p.done, p.total) == (1, 3)
    assert [(t.id, t.status) for t in p.tasks] == [("t-001", "Done"), ("t-002", "In Progress"), ("t-003", "Pending")]


def test_status_falls_back_to_timeline_then_checkbox(tmp_path: Path) -> None:
    phase(tmp_path, statuses={})
    [p] = load_phases(tmp_path)
    assert [t.status for t in p.tasks] == ["Done", "In Progress", "Pending"]


def test_legacy_phase_layout(tmp_path: Path) -> None:
    d = tmp_path / "specs" / "phase-002"
    d.mkdir(parents=True)
    (d / "impl.md").write_text("- [x] task-001: a\n- [ ] task-002: b\n")
    (d / "task-001.md").write_text("## Status\n- [x] Done\n")
    [p] = load_phases(tmp_path)
    assert p.name == "phase-002" and (p.done, p.total) == (1, 2)


def test_no_specs_dir_is_empty(tmp_path: Path) -> None:
    assert load_phases(tmp_path) == []


def test_malformed_and_binary_files_do_not_raise(tmp_path: Path) -> None:
    d = phase(tmp_path)
    (d / "t-002.md").write_bytes(b"\xff\xfe\x00garbage")
    (d / "impl.md").write_text(IMPL + "\x1b[2J" * 5)
    [p] = load_phases(tmp_path)
    assert p.total == 3


def test_phase_without_impl_is_reported_with_warning(tmp_path: Path) -> None:
    (tmp_path / "specs" / "p-009-empty").mkdir(parents=True)
    [p] = load_phases(tmp_path)
    assert p.warning and p.total == 0


def test_huge_impl_is_capped(tmp_path: Path) -> None:
    d = phase(tmp_path)
    (d / "impl.md").write_text(IMPL + "x" * 5_000_000)
    [p] = load_phases(tmp_path)
    assert p.total == 3


def test_escape_sequences_in_titles_are_stripped(tmp_path: Path) -> None:
    d = phase(tmp_path)
    (d / "impl.md").write_text("- [ ] t-001: \x1b]0;pwned\x07\x1b[31mred\x1b[0m title\n")
    [p] = load_phases(tmp_path)
    assert "\x1b" not in p.tasks[0].title and "\x07" not in p.tasks[0].title


def test_compiled_and_files_in_specs_ignored(tmp_path: Path) -> None:
    phase(tmp_path)
    (tmp_path / "specs" / "compiled").mkdir()
    (tmp_path / "specs" / "in-progress.md").write_text("x")
    assert [p.name for p in load_phases(tmp_path)] == ["p-001-thing"]


IN_PROGRESS = """# In Progress

## Example

### p-002-payments-api — t-003: Add refresh token rotation
- **Agent:** `@senior-backend-dev` (ID: `abc-1234`)

### p-001-thing — t-002: Real work
- **Agent:** `@senior-backend-dev` (ID: `sess-1`)
- **Location:** Branch `feat/x`
- **Ongoing:** Writing code.
- **Next step:** Test.
"""


def test_in_progress_parses_entries_and_skips_template_examples(tmp_path: Path) -> None:
    f = tmp_path / "in-progress.md"
    f.write_text(IN_PROGRESS)
    [e] = parse_in_progress(f)
    assert e.spec == "p-001-thing" and e.title == "t-002: Real work"
    assert e.fields["Ongoing"] == "Writing code." and "sess-1" in e.fields["Agent"]


def test_in_progress_missing_file(tmp_path: Path) -> None:
    assert parse_in_progress(tmp_path / "nope.md") == []


def test_sanitize_strips_csi_osc_and_controls() -> None:
    assert clean("a\x1b[31mb\x1b[0m\x1b]0;title\x07c\x00d\r\ne") == "abcd\ne"
    assert clean("[bold]x[/bold]") == "[bold]x[/bold]"


def repo(path: Path) -> None:
    git = ["git", "-c", "user.email=a@b.c", "-c", "user.name=t"]
    path.mkdir(parents=True)
    subprocess.run([*git, "init", "-q", "-b", "trunk"], cwd=path, check=True)
    (path / "f").write_text("x")
    subprocess.run([*git, "add", "-A"], cwd=path, check=True)
    subprocess.run([*git, "commit", "-qm", "i"], cwd=path, check=True)
    subprocess.run([*git, "branch", "feat"], cwd=path, check=True)
    subprocess.run([*git, "worktree", "add", "-q", str(path.parent / (path.name + "-wt")), "feat"], cwd=path, check=True)


def test_git_info_reports_branches_and_worktrees(tmp_path: Path) -> None:
    repo(tmp_path / "r")
    info = git_info(tmp_path / "r")
    assert info is not None and info.current == "trunk"
    assert set(info.branches) == {"trunk", "feat"} and len(info.worktrees) == 2


def test_git_info_not_a_repo_is_none(tmp_path: Path) -> None:
    assert git_info(tmp_path) is None


def test_git_info_missing_dir_is_none(tmp_path: Path) -> None:
    assert git_info(tmp_path / "nope") is None


def test_git_info_times_out(tmp_path: Path) -> None:
    fake = tmp_path / "bin"
    fake.mkdir()
    (fake / "git").write_text("#!/bin/sh\nsleep 5\n")
    (fake / "git").chmod(0o755)
    old = os.environ["PATH"]
    os.environ["PATH"] = f"{fake}:{old}"
    try:
        assert git_info(tmp_path, timeout=0.3) is None
    finally:
        os.environ["PATH"] = old


def test_codebases_from_symlinks_without_following_into_files(tmp_path: Path) -> None:
    repo(tmp_path / "pkg")
    links = tmp_path / "ws" / "codebase-symlinks"
    links.mkdir(parents=True)
    (links / "pkg").symlink_to(tmp_path / "pkg")
    (links / "dangling").symlink_to(tmp_path / "gone")
    found = {c.name: c for c in list_codebases(tmp_path / "ws")}
    assert found["pkg"].exists and found["pkg"].is_git and not found["dangling"].exists


def test_no_codebase_symlinks_dir(tmp_path: Path) -> None:
    assert list_codebases(tmp_path) == []


def claim(task: str, *paths: str) -> Claim:
    return Claim("p-001-x", task, "s", "", "", tuple(paths), 0.0)


def test_conflicts_exact_and_prefix_overlap() -> None:
    found = find_conflicts([claim("t-001", "src/a.py", "docs"), claim("t-002", "docs/readme.md"),
                            claim("t-003", "other/")])
    assert [(c.first, c.second, c.path) for c in found] == [("t-001", "t-002", "docs")]


def test_conflicts_none_when_disjoint_or_empty() -> None:
    assert find_conflicts([claim("t-001", "a"), claim("t-002", "b")]) == []
    assert find_conflicts([claim("t-001"), claim("t-002")]) == []


def test_conflicts_ignore_traversal_and_normalize() -> None:
    found = find_conflicts([claim("t-001", "./src//a.py"), claim("t-002", "src/a.py")])
    assert len(found) == 1
