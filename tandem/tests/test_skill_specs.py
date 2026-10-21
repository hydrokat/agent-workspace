import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any

import pytest

SCRIPT = Path(__file__).resolve().parents[2] / "templates/skills/spec-planner/scripts/specs.py"
FIXTURE = Path(__file__).resolve().parent / "fixtures/specs"


COORD_DIR = tempfile.mkdtemp(prefix="specs-test-coord-")


def run(root: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(SCRIPT), "--root", str(root), *args],
        capture_output=True, text=True, env={**os.environ, "TANDEM_COORD_DIR": COORD_DIR},
    )


def out(proc: subprocess.CompletedProcess[str]) -> dict[str, Any]:
    assert proc.returncode == 0, proc.stdout + proc.stderr
    data: dict[str, Any] = json.loads(proc.stdout)
    return data


def err(proc: subprocess.CompletedProcess[str], code: int) -> dict[str, Any]:
    assert proc.returncode == code, proc.stdout + proc.stderr
    data: dict[str, Any] = json.loads(proc.stdout)
    assert "error" in data
    return data


@pytest.fixture
def root() -> Path:
    return FIXTURE


def test_list_reports_every_phase_with_counts(root: Path) -> None:
    phases = {p["dir"]: p for p in out(run(root, "list"))["phases"]}
    assert set(phases) == {
        "p-001-agent-workspace-package", "p-002-sample", "p-003-broken", "phase-001",
    }
    p1 = phases["p-001-agent-workspace-package"]
    assert p1["counts"]["Done"] == 10 and p1["counts"]["In Review"] == 1
    assert p1["title"] == "tandem — Multi-Agent Terminal Workspace"
    assert p1["legacy"] is False and p1["next_task"] == "t-011"


def test_list_flags_legacy_and_next_pending(root: Path) -> None:
    phases = {p["dir"]: p for p in out(run(root, "list", "--json"))["phases"]}
    assert phases["phase-001"]["legacy"] is True
    assert phases["p-002-sample"]["next_task"] == "t-002"


def test_show_p001_parses_all_tasks_as_written(root: Path) -> None:
    phase = out(run(root, "show", "p-001"))
    assert len(phase["tasks"]) == 11
    statuses = {t["id"]: t["status"] for t in phase["tasks"]}
    assert statuses["t-001"] == "Done" and statuses["t-011"] == "In Review"
    types = {t["id"]: t["type"] for t in phase["tasks"]}
    assert types["t-001"] == "New Feature" and types["t-010"] == "Delivery"
    assert phase["scope_type"] == "Single Repo"
    assert phase["branches"]["target"] == "trunk"


def test_show_task_exposes_checklists_paths_and_dependencies(root: Path) -> None:
    task = out(run(root, "show", "p-002", "--task", "t-002"))["task"]
    assert task["type"] == "Bug Fix" and task["status"] == "In Progress"
    assert task["objectives"] == {"checked": 1, "total": 2}
    assert task["testing"] == {"checked": 1, "total": 2}
    assert task["security"] == {"checked": 0, "total": 1}
    assert task["affected_paths"] == ["src/other.py"]
    assert task["depends_on"] == ["t-001"]
    assert task["external_gates"] == ["p-001 t-011 / PR #13 merged"]


def test_affected_paths_split_inline_code_spans(root: Path) -> None:
    task = out(run(root, "show", "p-002", "--task", "t-001"))["task"]
    assert task["affected_paths"] == ["src/thing.py", "tests/test_thing.py", "docs/"]
    assert task["depends_on"] == [] and task["external_gates"] == []


def test_show_phase_surfaces_objectives_decisions_and_open_questions(root: Path) -> None:
    phase = out(run(root, "show", "p-002"))
    assert phase["objectives"][0] == "Add a thing."
    assert phase["decisions"] == [{"topic": "Language", "decision": "Python"}]
    assert [q["task"] for q in phase["open_questions"]] == ["t-002", "t-002"]
    assert phase["repositories"] == [{"name": "sample-repo", "role": "all code"}]


def test_repositories_accept_parenthetical_notes_after_the_name(root: Path) -> None:
    repos = out(run(root, "show", "p-001"))["repositories"]
    assert [r["name"] for r in repos] == ["agent-workspace-package", "agent-workspace"]
    assert repos[1]["role"].startswith("planning only")


def test_status_falls_back_to_timeline_when_status_section_is_free_text(root: Path) -> None:
    task = out(run(root, "show", "p-001", "--task", "t-011"))["task"]
    assert task["status"] == "In Review" and task["status_source"] == "timeline"


def test_malformed_files_degrade_with_warnings(root: Path) -> None:
    phase = out(run(root, "show", "p-003"))
    task = phase["tasks"][0]
    assert task["type"] is None and task["objectives"] is None
    assert task["status"] == "Pending"
    assert phase["warnings"] and task["warnings"]
    assert phase["branches"] is None


def test_legacy_phase_is_readable(root: Path) -> None:
    phase = out(run(root, "show", "phase-001"))
    assert phase["legacy"] is True
    assert phase["tasks"][0]["id"] == "task-001" and phase["tasks"][0]["status"] == "Done"


def test_next_numbers_continue_past_legacy_and_current(root: Path) -> None:
    assert out(run(root, "next-phase"))["next"] == "004"
    assert out(run(root, "next-task", "p-002"))["next"] == "t-004"
    assert out(run(root, "next-task", "p-001"))["next"] == "t-012"


@pytest.mark.parametrize("arg", ["p-002", "002", "2", "p-002-sample"])
def test_resolve_accepts_every_spelling(root: Path, arg: str) -> None:
    data = out(run(root, "resolve", arg))
    assert data["phase"] == "p-002-sample" and data["selected"] == "explicit"
    assert data["path"] == "specs/p-002-sample"


def test_resolve_without_argument_picks_latest_incomplete_phase(root: Path) -> None:
    data = out(run(root, "resolve"))
    assert data["phase"] == "p-003-broken" and data["selected"] == "active"


def test_resolve_unknown_phase_lists_valid_ones(root: Path) -> None:
    data = err(run(root, "resolve", "p-009"), 1)
    assert "p-002-sample" in data["valid"]


@pytest.mark.parametrize("arg", ["../etc", "p-002/../../x", "p-002-sample/../..", "p-0002", "P-002", "p-002 x"])
def test_invalid_arguments_are_rejected(root: Path, arg: str) -> None:
    err(run(root, "resolve", arg), 2)


def test_invalid_task_argument_is_rejected(root: Path) -> None:
    err(run(root, "show", "p-002", "--task", "../t-001"), 2)
    err(run(root, "show", "p-002", "--task", "t-099"), 1)


def test_symlinked_phase_escaping_specs_is_ignored(tmp_path: Path) -> None:
    specs = tmp_path / "specs"
    specs.mkdir()
    outside = tmp_path / "outside"
    outside.mkdir()
    (outside / "impl.md").write_text("# Implementation Plan: Evil\n")
    (specs / "p-001-evil").symlink_to(outside)
    assert out(run(tmp_path, "list"))["phases"] == []
    err(run(tmp_path, "show", "p-001"), 1)


def test_missing_specs_dir_is_reported(tmp_path: Path) -> None:
    data = err(run(tmp_path, "list"), 1)
    assert "specs" in data["error"]


def test_empty_specs_dir_resolves_to_nothing(tmp_path: Path) -> None:
    (tmp_path / "specs").mkdir()
    assert out(run(tmp_path, "list"))["phases"] == []
    err(run(tmp_path, "resolve"), 1)
    assert out(run(tmp_path, "next-phase"))["next"] == "001"


ASSETS = SCRIPT.parent.parent / "assets"
PLACEHOLDER_RE = re.compile(r"\[[^\]\n]{3,}\]")


def mkspecs(tmp_path: Path) -> Path:
    (tmp_path / "specs").mkdir(parents=True)
    return tmp_path


def new_phase(root: Path, *extra: str) -> dict[str, Any]:
    args = ["new-phase", "--title", "Demo Phase", "--branch", "feat/demo", "--target", "trunk", *extra]
    return out(run(root, *args))


def fill_placeholders(path: Path) -> None:
    text = path.read_text()
    asset_text = "".join(f.read_text() for f in ASSETS.glob("*.md"))
    for placeholder in set(PLACEHOLDER_RE.findall(asset_text)):
        if placeholder not in ("[ ]", "[x]"):
            text = text.replace(placeholder, "filled")
    path.write_text(text)


def lint_codes(root: Path, phase: str) -> set[str]:
    proc = run(root, "lint", phase)
    data = json.loads(proc.stdout)
    if data["ok"]:
        assert proc.returncode == 0
        return set()
    assert proc.returncode == 1
    return {p["code"] for p in data["problems"]}


def test_new_phase_creates_impl_and_delivery_task(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    data = new_phase(root, "--repo", "my-repo", "--pattern", "feat/<short-description>")
    assert data["phase"] == "p-001-my-repo"
    impl = (root / "specs/p-001-my-repo/impl.md").read_text()
    assert impl.startswith("# Implementation Plan: Demo Phase\n")
    for expected in ("**Working Branch**: feat/demo", "**Merge Target**: trunk", "specs/p-001-my-repo",
                     "**Scope Type**: Single Repo", "**Primary Repository**: my-repo",
                     "**Observed Branch Pattern**: feat/<short-description>",
                     "- **t-001**: Delivery: Commit, PR, Review, CI, Merge (Pending)",
                     "- [ ] t-001: Delivery: Commit, PR, Review, CI, Merge"):
        assert expected in impl, expected
    delivery = (root / "specs/p-001-my-repo/t-001.md").read_text()
    assert delivery.startswith("# t-001: Delivery: Commit, PR, Review, CI, Merge\n")
    assert "**Working Branch**: feat/demo" in delivery
    assert "`/code-cleanup --diff trunk`" in delivery
    assert "{{" not in impl + delivery


def test_new_phase_multi_repo_and_numbering_after_legacy(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    (root / "specs/phase-003").mkdir()
    (root / "specs/p-004-old").mkdir()
    data = new_phase(root, "--multi")
    assert data["phase"] == "p-005-multi"
    impl = (root / "specs/p-005-multi/impl.md").read_text()
    assert "**Scope Type**: Multi Repo" in impl and "**Primary Repository**: multi" in impl
    assert "**Observed Branch Pattern**: N/A" in impl


def test_new_phase_refuses_to_overwrite(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    (root / "specs/p-001-my-repo").write_text("a file squatting on the name")
    err(run(root, "new-phase", "--repo", "my-repo", "--title", "T", "--branch", "b", "--target", "t"), 1)
    assert (root / "specs/p-001-my-repo").read_text() == "a file squatting on the name"


def test_new_phase_rejects_bad_input(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    base = {"--title": "T", "--branch": "b", "--target": "t"}

    def attempt(*pre: str, **override: str) -> None:
        fields = {**base, **override}
        err(run(root, "new-phase", *pre, *[f"{k}={v}" for k, v in fields.items()]), 2)

    attempt("--repo=My Repo")
    attempt("--repo=../x")
    attempt("--repo=a", "--multi")
    attempt()
    attempt("--repo=a", **{"--title": "multi\nline"})
    attempt("--repo=a", **{"--title": "x" * 200})
    attempt("--repo=a", **{"--title": " "})
    for field in ("--branch", "--target"):
        for value in ("has space", "-x", "a;b", "", "a..b", "ends/"):
            attempt("--repo=a", **{field: value})
    assert list((root / "specs").iterdir()) == []


def test_new_task_inserts_before_delivery_and_renumbers(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    new_phase(root, "--repo", "my-repo")
    first = out(run(root, "new-task", "p-001", "--title", "Add the thing", "--type", "New Feature",
                    "--paths", "src/a.py", "src/b.py"))
    assert first["task"] == "t-001" and first["delivery"] == "t-002"
    second = out(run(root, "new-task", "p-001", "--title", "Fix it", "--type", "Bug Fix", "--depends", "t-001"))
    assert second["task"] == "t-002" and second["delivery"] == "t-003"
    phase = Path(root / "specs/p-001-my-repo")
    impl = (phase / "impl.md").read_text()
    assert "- **t-001**: Add the thing (Pending)\n- **t-002**: Fix it (Pending)\n- **t-003**: Delivery" in impl
    assert "- [ ] t-001: [New Feature] Add the thing\n- [ ] t-002: [Bug Fix] Fix it\n- [ ] t-003: Delivery" in impl
    task = (phase / "t-001.md").read_text()
    assert task.startswith("# t-001: Add the thing\n") and "- **Type**: New Feature" in task
    assert "- `src/a.py`\n- `src/b.py`" in task and "## Depends On\n- None" in task
    assert "**Working Branch**: feat/demo" in task and "{{" not in task
    assert "## Depends On\n- t-001" in (phase / "t-002.md").read_text()
    delivery = (phase / "t-003.md").read_text()
    assert delivery.startswith("# t-003: Delivery") and not (phase / "t-004.md").exists()
    shown = out(run(root, "show", "p-001"))
    assert [t["id"] for t in shown["tasks"]] == ["t-001", "t-002", "t-003"]
    assert shown["tasks"][-1]["type"] == "Delivery" and shown["tasks"][1]["depends_on"] == ["t-001"]


def test_new_task_without_paths_leaves_a_placeholder_for_lint(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    new_phase(root, "--repo", "r")
    out(run(root, "new-task", "p-001", "--title", "T", "--type", "Enhancement"))
    for f in (root / "specs/p-001-r").glob("*.md"):
        fill_placeholders(f)
    assert "placeholder" not in lint_codes(root, "p-001")


def test_new_task_rejects_bad_input(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    new_phase(root, "--repo", "r")
    base = ["new-task", "p-001"]
    err(run(root, *base, "--title", "T", "--type", "Chore"), 2)
    err(run(root, *base, "--title", "", "--type", "Bug Fix"), 2)
    err(run(root, *base, "--title", "a\nb", "--type", "Bug Fix"), 2)
    err(run(root, *base, "--title", "T", "--type", "Bug Fix", "--depends", "t-009"), 2)
    err(run(root, *base, "--title", "T", "--type", "Bug Fix", "--depends", "../x"), 2)
    err(run(root, *base, "--title", "T", "--type", "Bug Fix", "--paths", "bad\x07path"), 2)
    err(run(root, "new-task", "p-009", "--title", "T", "--type", "Bug Fix"), 1)
    assert sorted(f.name for f in (root / "specs/p-001-r").iterdir()) == ["impl.md", "t-001.md"]


def test_new_task_refuses_legacy_and_misordered_phases(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    shutil.copytree(FIXTURE / "specs/phase-001", root / "specs/phase-001")
    err(run(root, "new-task", "phase-001", "--title", "T", "--type", "Bug Fix"), 2)
    shutil.copytree(FIXTURE / "specs/p-001-agent-workspace-package", root / "specs/p-001-agent-workspace-package")
    before = (root / "specs/p-001-agent-workspace-package/impl.md").read_text()
    data = err(run(root, "new-task", "p-001", "--title", "T", "--type", "Bug Fix"), 1)
    assert "delivery" in data["error"]
    assert (root / "specs/p-001-agent-workspace-package/impl.md").read_text() == before


def test_new_task_in_phase_without_delivery_appends_with_a_warning(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    phase = root / "specs/p-001-r"
    phase.mkdir()
    (phase / "impl.md").write_text(
        "# Implementation Plan: X\n\n## Branch Strategy\n- **Working Branch**: `b`\n- **Merge Target**: `t`\n\n"
        "## Timeline\n- **t-001**: Old (Done)\n\n## Tasks\n- [x] t-001: [Enhancement] Old\n")
    (phase / "t-001.md").write_text("# t-001: Old\n\n## Status\n- [x] Done\n")
    data = out(run(root, "new-task", "p-001", "--title", "New", "--type", "Bug Fix"))
    assert data["task"] == "t-002" and data["delivery"] is None and data["warnings"]
    assert "- **t-002**: New (Pending)" in (phase / "impl.md").read_text()


def test_lint_flags_unfilled_placeholders_in_a_fresh_phase(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    new_phase(root, "--repo", "r")
    out(run(root, "new-task", "p-001", "--title", "T", "--type", "Enhancement", "--paths", "a.py"))
    assert "placeholder" in lint_codes(root, "p-001")


def test_lint_passes_when_the_phase_is_filled_in(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    new_phase(root, "--repo", "r")
    out(run(root, "new-task", "p-001", "--title", "T", "--type", "Enhancement", "--paths", "a.py"))
    for f in (root / "specs/p-001-r").glob("*.md"):
        fill_placeholders(f)
    assert lint_codes(root, "p-001") == set()


def filled_phase(tmp_path: Path) -> Path:
    root = mkspecs(tmp_path)
    new_phase(root, "--repo", "r")
    out(run(root, "new-task", "p-001", "--title", "T", "--type", "Enhancement", "--paths", "a.py"))
    for f in (root / "specs/p-001-r").glob("*.md"):
        fill_placeholders(f)
    return root


@pytest.mark.parametrize("section,code", [
    ("## Affected Paths", "missing-section"), ("## Depends On", "missing-section"),
    ("## Testing (TDD)", "missing-section"), ("## Security", "missing-section"),
    ("## Knowledge Updates", "missing-section"),
])
def test_lint_reports_each_missing_task_section(tmp_path: Path, section: str, code: str) -> None:
    root = filled_phase(tmp_path)
    task = root / "specs/p-001-r/t-001.md"
    task.write_text(task.read_text().replace(section, "## Renamed"))
    data = json.loads(run(root, "lint", "p-001").stdout)
    assert any(p["code"] == code and p["file"] == "t-001.md" and section.lstrip("# ").split(" (")[0] in p["message"]
               for p in data["problems"])


@pytest.mark.parametrize("section", ["## Branch Strategy", "## Repository Scope", "## Code Structure Diagram",
                                     "## Timeline", "## Tasks"])
def test_lint_reports_each_missing_impl_section(tmp_path: Path, section: str) -> None:
    root = filled_phase(tmp_path)
    impl = root / "specs/p-001-r/impl.md"
    impl.write_text(impl.read_text().replace(section, "## Renamed"))
    data = json.loads(run(root, "lint", "p-001").stdout)
    assert any(p["code"] == "missing-section" and p["file"] == "impl.md" for p in data["problems"])


def test_lint_reports_leftover_script_fields(tmp_path: Path) -> None:
    root = filled_phase(tmp_path)
    task = root / "specs/p-001-r/t-001.md"
    task.write_text(task.read_text() + "\nstray {{title}} field\n")
    assert "unrendered-field" in lint_codes(root, "p-001")


def test_lint_ignores_bracket_text_inside_code(tmp_path: Path) -> None:
    root = filled_phase(tmp_path)
    task = root / "specs/p-001-r/t-001.md"
    task.write_text(task.read_text() + "\nUse `[Task Title]` or\n```text\n[Objective 1]\n```\n")
    assert lint_codes(root, "p-001") == set()


def test_lint_reports_status_disagreement_between_impl_and_task(tmp_path: Path) -> None:
    root = filled_phase(tmp_path)
    task = root / "specs/p-001-r/t-001.md"
    task.write_text(task.read_text().replace("- [ ] Pending", "- [x] Done"))
    assert "status-mismatch" in lint_codes(root, "p-001")
    task.write_text(task.read_text().replace("- [x] Done", "- [ ] In Progress"))
    assert "status-mismatch" in lint_codes(root, "p-001")


def test_lint_reports_task_files_and_impl_entries_that_do_not_match(tmp_path: Path) -> None:
    root = filled_phase(tmp_path)
    (root / "specs/p-001-r/t-009.md").write_text("# t-009: Stray\n")
    assert "task-not-in-impl" in lint_codes(root, "p-001")
    (root / "specs/p-001-r/t-009.md").unlink()
    (root / "specs/p-001-r/t-001.md").unlink()
    assert "task-file-missing" in lint_codes(root, "p-001")


def test_lint_reports_delivery_not_last(tmp_path: Path) -> None:
    root = filled_phase(tmp_path)
    impl = root / "specs/p-001-r/impl.md"
    task = root / "specs/p-001-r/t-003.md"
    task.write_text("# t-003: Late work\n\n## Requirement Classification\n- **Type**: Bug Fix\n")
    impl.write_text(impl.read_text().replace("(Pending)\n\n## Tasks", "(Pending)\n- **t-003**: Late work (Pending)\n\n## Tasks")
                    + "- [ ] t-003: [Bug Fix] Late work\n")
    assert "delivery-not-last" in lint_codes(root, "p-001")


def test_lint_reports_missing_and_duplicate_delivery(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    phase = root / "specs/p-001-r"
    phase.mkdir()
    (phase / "impl.md").write_text(
        "# Implementation Plan: X\n\n## Timeline\n- **t-001**: Old (Done)\n\n## Tasks\n- [x] t-001: [Enhancement] Old\n")
    (phase / "t-001.md").write_text("# t-001: Old\n\n## Status\n- [x] Done\n")
    assert "delivery-missing" in lint_codes(root, "p-001")
    for number in (2, 3):
        (phase / f"t-00{number}.md").write_text(f"# t-00{number}: Delivery: Commit, PR, Review, CI, Merge\n")
    assert "multiple-deliveries" in lint_codes(root, "p-001")


def test_lint_ignores_parenthetical_notes_after_impl_titles(tmp_path: Path) -> None:
    root = filled_phase(tmp_path)
    impl = root / "specs/p-001-r/impl.md"
    impl.write_text(impl.read_text().replace("[Enhancement] T", "[Enhancement] T (PR #1, waiting)"))
    assert lint_codes(root, "p-001") == set()
    impl.write_text(impl.read_text().replace("T (PR #1, waiting)", "Different title"))
    assert "title-mismatch" in lint_codes(root, "p-001")


def test_lint_on_p001_as_written_reports_only_real_gaps(root: Path) -> None:
    data = json.loads(run(root, "lint", "p-001").stdout)
    codes = {p["code"] for p in data["problems"]}
    assert codes == {"delivery-not-last", "missing-section"}
    gaps = [p for p in data["problems"] if p["code"] == "missing-section"]
    assert {p["file"] for p in gaps}.isdisjoint({"impl.md", "t-010.md"})
    assert all(p["message"].startswith("missing section") for p in gaps)


def test_lint_refuses_legacy_phases(root: Path) -> None:
    err(run(root, "lint", "phase-001"), 2)


def test_script_writes_nothing_outside_the_phase(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    (root / "marker").write_text("x")
    new_phase(root, "--repo", "r")
    out(run(root, "new-task", "p-001", "--title", "T", "--type", "Bug Fix"))
    assert {p.name for p in root.iterdir()} == {"specs", "marker"}
    assert {p.name for p in (root / "specs").iterdir()} == {"p-001-r"}
    assert not [p for p in (root / "specs/p-001-r").iterdir() if p.name.startswith(".")]


def sample_copy(tmp_path: Path) -> Path:
    shutil.copytree(FIXTURE / "specs", tmp_path / "specs")
    return tmp_path


def read(root: Path, rel: str) -> str:
    return (root / "specs" / rel).read_text()


def snapshot(root: Path) -> dict[str, bytes]:
    return {str(p.relative_to(root)): p.read_bytes() for p in (root / "specs").rglob("*") if p.is_file()}


def test_set_status_updates_task_and_both_impl_lists(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    data = out(run(root, "set-status", "p-002", "t-002", "In Review"))
    assert data["from"] == "In Progress" and data["to"] == "In Review" and data["changed"] is True
    assert "## Status\n- [ ] In Review\n" in read(root, "p-002-sample/t-002.md")
    impl = read(root, "p-002-sample/impl.md")
    assert "- **t-002**: Fix the other thing (In Review)" in impl and "- [ ] t-002: [Bug Fix]" in impl
    assert out(run(root, "show", "p-002", "--task", "t-002"))["task"]["status"] == "In Review"
    assert "status-mismatch" not in lint_codes(root, "p-002")


def test_set_status_is_idempotent(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    out(run(root, "set-status", "p-002", "t-002", "Blocked"))
    before = snapshot(root)
    assert out(run(root, "set-status", "p-002", "t-002", "Blocked"))["changed"] is False
    assert snapshot(root) == before


def test_set_status_replaces_free_text_status_and_annotated_timeline_labels(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    out(run(root, "set-status", "p-001", "t-011", "Blocked"))
    assert "## Status\n- [ ] Blocked\n" in read(root, "p-001-agent-workspace-package/t-011.md")
    assert "(Blocked)" in read(root, "p-001-agent-workspace-package/impl.md").split("**t-011**")[1].split("\n")[0]


def test_set_status_done_is_refused_until_objectives_testing_and_security_are_checked(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    before = snapshot(root)
    data = err(run(root, "set-status", "p-002", "t-002", "Done"), 1)
    assert set(data["missing"]) == {"objectives", "testing", "security"}
    assert snapshot(root) == before


def test_set_status_done_succeeds_when_everything_is_checked(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    new_phase(root, "--repo", "r")
    out(run(root, "new-task", "p-001", "--title", "T", "--type", "Enhancement", "--paths", "a.py"))
    task = root / "specs/p-001-r/t-001.md"
    head, _, tail = task.read_text().partition("## Status")
    task.write_text(head.replace("- [ ]", "- [x]") + "## Status" + tail)
    data = out(run(root, "set-status", "p-001", "t-001", "Done"))
    assert data["to"] == "Done"
    assert "## Status\n- [x] Done\n" in task.read_text()
    impl = (root / "specs/p-001-r/impl.md").read_text()
    assert "- **t-001**: T (Done)" in impl and "- [x] t-001: [Enhancement] T" in impl


def test_set_status_done_on_delivery_task_only_needs_objectives(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    new_phase(root, "--repo", "r")
    err(run(root, "set-status", "p-001", "t-001", "Done"), 1)
    task = root / "specs/p-001-r/t-001.md"
    head, _, tail = task.read_text().partition("## Status")
    task.write_text(head.replace("- [ ]", "- [x]") + "## Status" + tail)
    out(run(root, "set-status", "p-001", "t-001", "Done"))


def test_set_status_done_is_refused_when_a_checklist_is_missing(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    task = root / "specs/p-002-sample/t-002.md"
    task.write_text(task.read_text().replace("- [ ] Fix", "- [x] Fix").replace("## Security\n- [ ] Security audited\n", ""))
    data = err(run(root, "set-status", "p-002", "t-002", "Done"), 1)
    assert "security" in data["missing"]


def test_set_status_appends_a_status_section_when_missing(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    task = root / "specs/p-002-sample/t-002.md"
    task.write_text(task.read_text().split("## Status")[0].rstrip("\n"))
    out(run(root, "set-status", "p-002", "t-002", "Blocked"))
    assert task.read_text().endswith("## Status\n- [ ] Blocked\n")


def test_set_status_rejects_bad_input(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    before = snapshot(root)
    err(run(root, "set-status", "p-002", "t-002", "Finished"), 2)
    err(run(root, "set-status", "p-002", "../t-002", "Done"), 2)
    err(run(root, "set-status", "p-002", "t-099", "Blocked"), 1)
    err(run(root, "set-status", "p-009", "t-001", "Blocked"), 1)
    err(run(root, "set-status", "phase-001", "task-001", "Blocked"), 2)
    assert snapshot(root) == before


def test_set_status_fails_without_touching_files_when_impl_has_no_entry(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    impl = root / "specs/p-002-sample/impl.md"
    impl.write_text(impl.read_text().replace("- **t-002**: Fix the other thing (In Progress)\n", ""))
    before = snapshot(root)
    err(run(root, "set-status", "p-002", "t-002", "Blocked"), 1)
    assert snapshot(root) == before


def test_normalize_adds_only_missing_metadata_sections_before_status(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    new_phase(root, "--repo", "r")
    out(run(root, "new-task", "p-001", "--title", "T", "--type", "Enhancement", "--paths", "a.py"))
    task = root / "specs/p-001-r/t-001.md"
    before = task.read_text()
    data = out(run(root, "normalize", "p-001", "t-001"))
    assert data["added"] == ["Agent Assignment", "Progress"]
    after = task.read_text()
    assert after.index("## Agent Assignment") < after.index("## Progress") < after.index("## Status")
    assert after.replace(after[after.index("## Agent Assignment"):after.index("## Status")], "") == before
    assert "## Knowledgebase References" not in after and "- **Primary Agent**: [agent-name]" in after


def test_normalize_adds_all_four_sections_to_a_bare_task_and_is_idempotent(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    data = out(run(root, "normalize", "p-002", "t-002"))
    assert data["added"] == ["Agent Assignment", "Knowledgebase References", "Progress", "Knowledgebase Updates"]
    once = snapshot(root)
    assert out(run(root, "normalize", "p-002", "t-002"))["added"] == []
    assert snapshot(root) == once


def test_normalize_rejects_unknown_or_invalid_tasks(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    err(run(root, "normalize", "p-002", "t-099"), 1)
    err(run(root, "normalize", "p-002", "../x"), 2)
    err(run(root, "normalize", "phase-001", "task-001"), 2)


def compiled(root: Path, name: str) -> str:
    return (root / "specs/compiled" / name).read_text()


def test_compile_all_writes_both_files_with_classification(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    data = out(run(root, "compile", "--date", "2026-01-02"))
    assert data["filter"] == "all" and data["written"] == ["completed.md", "incomplete.md"]
    done, todo = compiled(root, "completed.md"), compiled(root, "incomplete.md")
    assert done.startswith("# Compiled Specs — Completed\n\n> Generated: 2026-01-02\n> Scope: all phases\n")
    assert "## p-001-agent-workspace-package: tandem — Multi-Agent Terminal Workspace" in todo + done
    assert "#### t-002: Fix the other thing\n\nMissing:\n- [ ] Done — objectives not fully checked" in todo
    assert "- [ ] Security Audited — no security audit section found" in todo
    assert "| t-001 | Add the thing | ✅ | ✅ | ✅ |" in todo
    assert data["phases"] == {"in_scope": 4, "complete": 0, "incomplete": 4}


def test_compile_orders_incomplete_tasks_first_and_phases_numerically(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    out(run(root, "compile", "incomplete", "--date", "2026-01-02"))
    todo = compiled(root, "incomplete.md")
    table = todo.split("## p-002-sample")[1].split("### Incomplete Tasks")[0]
    assert table.index("| t-002 |") < table.index("| t-001 |")
    assert todo.index("## p-001-") < todo.index("## p-002-") < todo.index("## p-003-")


def test_compile_filters_write_and_clear_only_their_files(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    (root / "specs/compiled").mkdir()
    for name in ("completed.md", "incomplete.md", "extra.md"):
        (root / "specs/compiled" / name).write_text("stale")
    assert out(run(root, "compile", "completed"))["written"] == ["completed.md"]
    assert compiled(root, "incomplete.md") == "stale" and compiled(root, "extra.md") == "stale"
    assert "> Scope: completed filter (all phases scanned)" in compiled(root, "completed.md")
    assert out(run(root, "compile", "incomplete"))["written"] == ["incomplete.md"]
    assert "> Scope: incomplete filter (all phases scanned)" in compiled(root, "incomplete.md")
    out(run(root, "compile", "002"))
    assert compiled(root, "extra.md") == "stale" and "> Scope: phase 002" in compiled(root, "completed.md")
    assert "## p-002-sample" in compiled(root, "incomplete.md") and "p-001" not in compiled(root, "incomplete.md")
    out(run(root, "compile", "all"))
    assert not (root / "specs/compiled/extra.md").exists()


def test_compile_range_and_unrecognised_argument(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    data = out(run(root, "compile", "001-002"))
    assert "> Scope: phases 001–002" in compiled(root, "incomplete.md") and data["phases"]["in_scope"] == 3
    data = out(run(root, "compile", "bogus"))
    assert data["filter"] == "all" and "unrecognised" in data["note"]
    data = out(run(root, "compile", "009-001"))
    assert data["filter"] == "all" and data["note"]


def test_compile_warns_about_missing_phase_directories(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    data = out(run(root, "compile", "009"))
    assert data["phases"]["in_scope"] == 0 and any("009" in w for w in data["warnings"])
    assert "No matching specs were found" in compiled(root, "completed.md")
    assert "No matching specs were found" in compiled(root, "incomplete.md")


COMPLETE_TASK = (
    "# t-001: Done thing\n\n## Objectives\n- [x] a\n\n## Testing (TDD)\n- [x] t\n\n## Security\n- [x] s\n\n"
    "## Status\n- [x] Done\n"
)


def complete_phase(root: Path) -> None:
    phase = root / "specs/p-001-done"
    phase.mkdir()
    (phase / "impl.md").write_text(
        "# Implementation Plan: Done\n\n## Phase Objectives\n1. Ship it.\n\n## Tasks\n- [x] t-001: [Bug Fix] Done thing\n")
    (phase / "t-001.md").write_text(COMPLETE_TASK)


def test_compile_empty_states(tmp_path: Path) -> None:
    root = mkspecs(tmp_path / "a")
    out(run(root, "compile"))
    assert "No matching specs were found" in compiled(root, "incomplete.md")
    done = mkspecs(tmp_path / "b")
    complete_phase(done)
    out(run(done, "compile"))
    assert "All phases within the selected scope are fully complete. See completed.md." in compiled(done, "incomplete.md")
    assert "1 of 1 phases fully complete." in compiled(done, "completed.md")
    assert "- Ship it." in compiled(done, "completed.md")
    none = sample_copy(tmp_path / "c")
    out(run(none, "compile", "003"))
    assert "No phases are fully complete within the selected scope." in compiled(none, "completed.md")


def test_compile_treats_a_phase_without_tasks_as_incomplete_and_unreadable_tasks_as_unknown(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    (root / "specs/p-001-empty").mkdir()
    (root / "specs/p-001-empty/impl.md").write_text("# Implementation Plan: Empty\n")
    phase = root / "specs/p-002-locked"
    phase.mkdir()
    (phase / "impl.md").write_text("# Implementation Plan: Locked\n\n## Tasks\n- [ ] t-001: [Bug Fix] Locked\n")
    locked = phase / "t-001.md"
    locked.write_text("# t-001: Locked\n")
    locked.chmod(0)
    try:
        out(run(root, "compile"))
    finally:
        locked.chmod(0o644)
    todo = compiled(root, "incomplete.md")
    assert "## p-001-empty: Empty" in todo and "no task files" in todo
    assert "| t-001 | Locked | ❓ | ❓ | ❓ |" in todo and "specs/p-002-locked/t-001.md" in todo


def test_compile_never_modifies_specs_and_skips_the_output_directory(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    before = snapshot(root)
    out(run(root, "compile"))
    out(run(root, "compile"))
    after = {k: v for k, v in snapshot(root).items() if "/compiled/" not in k}
    assert after == before
    assert "compiled" not in compiled(root, "incomplete.md").split("## Summary")[1]


def test_compile_refuses_a_symlinked_output_directory(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    outside = tmp_path / "outside"
    outside.mkdir()
    (outside / "keep.md").write_text("keep")
    (root / "specs/compiled").symlink_to(outside)
    err(run(root, "compile"), 1)
    assert (outside / "keep.md").read_text() == "keep"


def test_compile_detects_completion_signals_from_the_documented_patterns(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    phase = root / "specs/p-001-signals"
    phase.mkdir()
    (phase / "impl.md").write_text("# Implementation Plan: Signals\n\n## Tasks\n"
                                   + "".join(f"- [ ] t-00{n}: [Bug Fix] Task {n}\n" for n in range(1, 5)))
    (phase / "t-001.md").write_text("# t-001: Task 1\n\nStatus: Complete\nTested: Yes\nSecurity Reviewed: Yes\n")
    (phase / "t-002.md").write_text("# t-002: Task 2\n\n## Objectives\n- [x] a\n- [ ] b\n\n## Status\n- [x] Done\n"
                                    "- [x] Tested\n- [x] Security Audited\n")
    (phase / "t-003.md").write_text("# t-003: Task 3\n\n## Objectives\n- [x] a\n\n## Test Results\n- [x] ok\n"
                                    "\n## Security Review\n- [x] ok\n")
    (phase / "t-004.md").write_text("# t-004: Task 4\n\n## Objectives\n- [x] a\n\n## Tests\n\n## Security\n")
    out(run(root, "compile", "--date", "2026-01-02"))
    todo, done = compiled(root, "incomplete.md"), compiled(root, "completed.md")
    assert "| t-001 | Task 1 | ✅ | ✅ | ✅ |" in todo or "| t-001 | Task 1 | ✅ | ✅ | ✅ |" in done
    assert "| t-002 | Task 2 | ❌ | ✅ | ✅ |" in todo
    assert "| t-003 | Task 3 | ✅ | ✅ | ✅ |" in todo
    assert "| t-004 | Task 4 | ✅ | ❌ | ❌ |" in todo


def test_compile_output_matches_the_documented_format_golden_files(tmp_path: Path) -> None:
    golden = FIXTURE.parent / "compile"
    shutil.copytree(golden / "specs", tmp_path / "specs")
    out(run(tmp_path, "compile", "--date", "2026-01-02"))
    assert compiled(tmp_path, "completed.md") == (golden / "golden/completed.md").read_text()
    assert compiled(tmp_path, "incomplete.md") == (golden / "golden/incomplete.md").read_text()


def test_lint_flags_unfilled_metadata_placeholders_added_by_normalize(tmp_path: Path) -> None:
    root = filled_phase(tmp_path)
    assert lint_codes(root, "p-001") == set()
    out(run(root, "normalize", "p-001", "t-001"))
    assert "placeholder" in lint_codes(root, "p-001")
    task = root / "specs/p-001-r/t-001.md"
    text = task.read_text()
    for placeholder in ("[agent-name]", "[agent-name, if needed]", "[caveman, task-specific skill-name, if needed]"):
        text = text.replace(placeholder, "filled")
    task.write_text(text)
    assert lint_codes(root, "p-001") == set()


def test_status_headings_inside_code_fences_and_prose_are_not_the_status_section(tmp_path: Path) -> None:
    root = sample_copy(tmp_path)
    task = root / "specs/p-002-sample/t-002.md"
    decoy = "\n## Notes\nSee `## Status` below.\n\n```markdown\n## Status\n- [ ] Pending\n```\n"
    task.write_text(task.read_text().replace("\n## Objectives", decoy + "\n## Objectives", 1))
    out(run(root, "set-status", "p-002", "t-002", "Blocked"))
    text = task.read_text()
    assert "```markdown\n## Status\n- [ ] Pending\n```" in text
    assert text.rstrip().endswith("## Status\n- [ ] Blocked")
    out(run(root, "normalize", "p-002", "t-002"))
    text = task.read_text()
    assert text.index("## Agent Assignment") > text.index("```\n") and text.rstrip().endswith("## Status\n- [ ] Blocked")


COORD = SCRIPT.parents[2] / "agent-coordination/scripts/coord.py"


def wave_phase(root: Path, tasks: dict[str, dict[str, Any]], name: str = "p-001-w") -> Path:
    phase = root / "specs" / name
    phase.mkdir(parents=True, exist_ok=True)
    timeline, listing = [], []
    for task_id, spec in tasks.items():
        status = spec.get("status", "Pending")
        delivery = spec.get("delivery", False)
        title = DELIVERY_TITLE if delivery else spec.get("title", f"Task {task_id}")
        timeline.append(f"- **{task_id}**: {title} ({status})")
        listing.append(f"- [{'x' if status == 'Done' else ' '}] {task_id}: " + ("" if delivery else "[Enhancement] ") + title)
        lines = [f"# {task_id}: {title}", ""]
        if not delivery:
            lines += ["## Requirement Classification", "- **Type**: Enhancement", ""]
            paths = spec.get("paths", [])
            if paths is not None:
                lines += ["## Affected Paths", *[f"- `{p}`" for p in paths], ""]
            deps = spec.get("depends", [])
            lines += ["## Depends On", *([f"- {d}" for d in deps] or ["- None"]), ""]
        lines += ["## Status", f"- [{'x' if status == 'Done' else ' '}] {status}", ""]
        (phase / f"{task_id}.md").write_text("\n".join(lines))
    (phase / "impl.md").write_text(
        "# Implementation Plan: Waves\n\n## Branch Strategy\n- **Working Branch**: `feat/w`\n- **Merge Target**: `trunk`\n\n"
        "## Timeline\n" + "\n".join(timeline) + "\n\n## Tasks\n" + "\n".join(listing) + "\n\n## Open Questions\n1. none\n")
    return phase


DELIVERY_TITLE = "Delivery: Commit, PR, Review, CI, Merge"
END = {"delivery": True}


def waves(root: Path, *args: str) -> dict[str, Any]:
    return out(run(root, "waves", "p-001", *args))


def test_waves_layers_a_diamond_of_dependencies(tmp_path: Path) -> None:
    wave_phase(tmp_path, {
        "t-001": {"paths": ["a/"]}, "t-002": {"paths": ["b/"], "depends": ["t-001"]},
        "t-003": {"paths": ["c/"], "depends": ["t-001"]}, "t-004": {"paths": ["d/"], "depends": ["t-002", "t-003"]},
        "t-005": END,
    })
    data = waves(tmp_path)
    assert data["waves"] == [["t-001"], ["t-002", "t-003"], ["t-004"], ["t-005"]]
    assert data["order"] == ["t-001", "t-002", "t-003", "t-004", "t-005"]
    assert data["tasks"]["t-001"]["state"] == "ready" and data["tasks"]["t-002"]["state"] == "waiting"
    assert data["tasks"]["t-002"]["reasons"] == ["depends:t-001"]
    assert data["tasks"]["t-004"]["wave"] == 3


def test_waves_runs_disjoint_independent_tasks_together_and_the_delivery_alone_last(tmp_path: Path) -> None:
    wave_phase(tmp_path, {"t-001": {"paths": ["a/"]}, "t-002": {"paths": ["b/x.py"]}, "t-003": END})
    assert waves(tmp_path)["waves"] == [["t-001", "t-002"], ["t-003"]]


@pytest.mark.parametrize("first,second", [
    ("src/a.py", "src/a.py"), ("src/", "src/a/b.py"), ("src/a/b.py", "src"), ("src/**/*.py", "src/x/y.py"),
    ("./src/a.py", "src/a.py/"), ("CHANGELOG.md", "docs/CHANGELOG.md"), ("package.json", "web/package.json"),
    ("uv.lock", "tandem/uv.lock"), ("test/pack.test.js", "pkg/test/pack.test.js"),
])
def test_waves_separates_tasks_whose_paths_overlap_or_share_a_hot_file(tmp_path: Path, first: str, second: str) -> None:
    wave_phase(tmp_path, {"t-001": {"paths": [first]}, "t-002": {"paths": [second]}, "t-003": END})
    data = waves(tmp_path)
    assert data["waves"] == [["t-001"], ["t-002"], ["t-003"]]
    assert any(r.startswith("paths:") for r in data["tasks"]["t-002"]["reasons"])
    assert data["tasks"]["t-002"]["conflicts_with"] == ["t-001"]


def test_waves_does_not_treat_sibling_names_as_overlap(tmp_path: Path) -> None:
    wave_phase(tmp_path, {"t-001": {"paths": ["src/app"]}, "t-002": {"paths": ["src/application.py"]}, "t-003": END})
    assert waves(tmp_path)["waves"][0] == ["t-001", "t-002"]


def test_waves_keeps_a_gated_task_gated_until_cleared(tmp_path: Path) -> None:
    phase = wave_phase(tmp_path, {"t-001": {"paths": ["a/"]}, "t-002": {"paths": ["b/"]}, "t-003": END})
    task = phase / "t-002.md"
    task.write_text(task.read_text().replace("- None", "- p-001 t-011 / PR #13 merged"))
    data = waves(tmp_path)
    assert data["waves"] == [["t-001", "t-002"], ["t-003"]]
    assert data["tasks"]["t-002"]["state"] == "gated"
    assert data["tasks"]["t-002"]["reasons"] == ["gate:p-001 t-011 / PR #13 merged"]
    assert data["gates"] == [{"task": "t-002", "gate": "p-001 t-011 / PR #13 merged"}]
    assert waves(tmp_path, "--cleared", "something else")["tasks"]["t-002"]["state"] == "gated"
    cleared = waves(tmp_path, "--cleared", "PR #13")
    assert cleared["tasks"]["t-002"]["state"] == "ready" and cleared["gates"] == []


def test_waves_runs_a_task_with_unknown_footprint_alone(tmp_path: Path) -> None:
    wave_phase(tmp_path, {"t-001": {"paths": ["a/"]}, "t-002": {"paths": None}, "t-003": {"paths": ["c/"]}, "t-004": END})
    data = waves(tmp_path)
    assert data["waves"] == [["t-001"], ["t-002"], ["t-003"], ["t-004"]]
    assert "unknown-footprint" in data["tasks"]["t-002"]["reasons"]


def test_waves_treats_an_empty_affected_paths_section_as_unknown(tmp_path: Path) -> None:
    wave_phase(tmp_path, {"t-001": {"paths": []}, "t-002": {"paths": ["b/"]}, "t-003": END})
    assert waves(tmp_path)["waves"] == [["t-001"], ["t-002"], ["t-003"]]


def test_waves_reports_a_dependency_cycle(tmp_path: Path) -> None:
    wave_phase(tmp_path, {
        "t-001": {"paths": ["a/"], "depends": ["t-002"]}, "t-002": {"paths": ["b/"], "depends": ["t-001"]}, "t-003": END,
    })
    data = err(run(tmp_path, "waves", "p-001"), 1)
    assert set(data["cycle"]) >= {"t-001", "t-002"} and "t-001" in data["error"] and "t-002" in data["error"]


def test_waves_dependency_order_beats_number_order_for_overlapping_tasks(tmp_path: Path) -> None:
    wave_phase(tmp_path, {
        "t-001": {"paths": ["a.py"], "depends": ["t-002"]}, "t-002": {"paths": ["a.py"]}, "t-003": END,
    })
    assert waves(tmp_path)["waves"] == [["t-002"], ["t-001"], ["t-003"]]


def test_waves_chunks_by_max_parallel(tmp_path: Path) -> None:
    wave_phase(tmp_path, {f"t-00{n}": {"paths": [f"d{n}/"]} for n in range(1, 4)} | {"t-004": END})
    assert waves(tmp_path)["waves"] == [["t-001", "t-002", "t-003"], ["t-004"]]
    assert waves(tmp_path, "--max-parallel", "2")["waves"] == [["t-001", "t-002"], ["t-003"], ["t-004"]]
    assert waves(tmp_path, "--max-parallel", "1")["order"] == ["t-001", "t-002", "t-003", "t-004"]
    for bad in ("0", "9", "x"):
        assert run(tmp_path, "waves", "p-001", "--max-parallel", bad).returncode == 2


def test_waves_skips_done_tasks_and_counts_them_as_satisfied(tmp_path: Path) -> None:
    wave_phase(tmp_path, {
        "t-001": {"paths": ["a/"], "status": "Done"}, "t-002": {"paths": ["b/"], "depends": ["t-001"]}, "t-003": END,
    })
    data = waves(tmp_path)
    assert data["waves"] == [["t-002"], ["t-003"]]
    assert data["tasks"]["t-001"]["state"] == "done" and data["tasks"]["t-002"]["state"] == "ready"
    assert waves(tmp_path, "--done", "t-002")["waves"] == [["t-003"]]


def test_waves_puts_running_work_first_and_holds_back_conflicting_tasks(tmp_path: Path) -> None:
    wave_phase(tmp_path, {
        "t-001": {"paths": ["a/"], "status": "In Progress"}, "t-002": {"paths": ["a/x.py"]},
        "t-003": {"paths": ["c/"]}, "t-004": {"paths": ["d/"], "depends": ["t-001"]}, "t-005": END,
    })
    data = waves(tmp_path)
    assert data["waves"] == [["t-001", "t-003"], ["t-002", "t-004"], ["t-005"]]
    assert data["tasks"]["t-001"]["state"] == "running" and data["tasks"]["t-003"]["state"] == "ready"
    assert data["tasks"]["t-002"]["state"] == "waiting" and "paths:a/x.py" in data["tasks"]["t-002"]["reasons"]
    claimed = waves(tmp_path, "--running", "t-003")
    assert claimed["tasks"]["t-003"]["state"] == "running"


def test_waves_blocked_and_in_review_tasks_hold_back_only_their_dependents(tmp_path: Path) -> None:
    wave_phase(tmp_path, {
        "t-001": {"paths": ["a/"], "status": "Blocked"}, "t-002": {"paths": ["b/"], "depends": ["t-001"]},
        "t-003": {"paths": ["c/"], "depends": ["t-002"]}, "t-004": {"paths": ["d/"]},
        "t-005": {"paths": ["e/"], "status": "In Review"}, "t-006": {"paths": ["f/"], "depends": ["t-005"]}, "t-007": END,
    })
    data = waves(tmp_path)
    assert data["waves"] == [["t-004"]]
    assert data["tasks"]["t-001"]["state"] == "blocked" and data["tasks"]["t-005"]["state"] == "in-review"
    assert data["tasks"]["t-002"]["reasons"] == ["depends:t-001 (blocked)"]
    assert data["tasks"]["t-003"]["reasons"] == ["depends:t-002 (held)"]
    assert data["tasks"]["t-006"]["reasons"] == ["depends:t-005 (in review)"]
    assert data["tasks"]["t-007"]["state"] == "waiting" and data["held"] == ["t-002", "t-003", "t-006", "t-007"]


def test_waves_holds_a_task_with_an_unknown_dependency(tmp_path: Path) -> None:
    wave_phase(tmp_path, {"t-001": {"paths": ["a/"], "depends": ["t-099"]}, "t-002": {"paths": ["b/"]}, "t-003": END})
    data = waves(tmp_path)
    assert data["waves"] == [["t-002"]] and data["tasks"]["t-001"]["reasons"] == ["depends:t-099 (unknown task)"]


def test_waves_marks_tasks_claimed_in_coordination_as_running(tmp_path: Path) -> None:
    wave_phase(tmp_path, {"t-001": {"paths": ["a/"]}, "t-002": {"paths": ["b/"]}, "t-003": END})
    coord_dir = tmp_path / "coord"
    env = {**os.environ, "TANDEM_COORD_DIR": str(coord_dir), "TANDEM_SESSION_ID": "sess-a",
           "TANDEM_ROLE": "implementer", "TANDEM_CLI": "claude"}
    subprocess.run([sys.executable, str(COORD), "register"], env=env, check=True, capture_output=True)
    subprocess.run([sys.executable, str(COORD), "claim", "p-001-w", "t-001", "--paths", "a/"],
                   env=env, check=True, capture_output=True)
    proc = subprocess.run([sys.executable, str(SCRIPT), "--root", str(tmp_path), "waves", "p-001"],
                          capture_output=True, text=True, env=env)
    data = json.loads(proc.stdout)
    assert data["tasks"]["t-001"]["state"] == "running" and data["tasks"]["t-002"]["state"] == "ready"
    assert data["waves"][0] == ["t-001", "t-002"]
    broken = subprocess.run([sys.executable, str(SCRIPT), "--root", str(tmp_path), "waves", "p-001"],
                            capture_output=True, text=True, env={**env, "TANDEM_COORD_DIR": "relative/path"})
    assert json.loads(broken.stdout)["warnings"] and json.loads(broken.stdout)["tasks"]["t-001"]["state"] == "ready"


def test_waves_refuses_legacy_phases_and_unknown_phases(root: Path) -> None:
    err(run(root, "waves", "phase-001"), 2)
    err(run(root, "waves", "p-009"), 1)


def test_waves_write_inserts_and_replaces_the_parallel_waves_section(tmp_path: Path) -> None:
    phase = wave_phase(tmp_path, {"t-001": {"paths": ["a/"]}, "t-002": {"paths": ["b/"], "depends": ["t-001"]}, "t-003": END})
    impl = phase / "impl.md"
    impl.write_text(impl.read_text().replace("## Open Questions", "```markdown\n## Parallel Waves\ndecoy\n```\n\n## Open Questions"))
    out(run(tmp_path, "waves", "p-001", "--write"))
    first = impl.read_text()
    assert first.count("\n## Parallel Waves\n") == 2 and "decoy" in first
    assert "| 1 | t-001 | ready now |" in first and "| 2 | t-002 | t-002: depends:t-001 |" in first
    assert first.index("## Parallel Waves\n\nGenerated") < first.index("## Open Questions")
    out(run(tmp_path, "waves", "p-001", "--write"))
    assert impl.read_text() == first
    task = phase / "t-002.md"
    task.write_text(task.read_text().replace("- t-001", "- None"))
    out(run(tmp_path, "waves", "p-001", "--write"))
    assert "| 1 | t-001 · t-002 | ready now |" in impl.read_text() and impl.read_text().count("Generated by") == 1


def test_new_phase_template_has_a_parallel_waves_section(tmp_path: Path) -> None:
    root = mkspecs(tmp_path)
    new_phase(root, "--repo", "r")
    impl = (root / "specs/p-001-r/impl.md").read_text()
    assert "## Parallel Waves" in impl and "specs.py waves" in impl
    out(run(root, "new-task", "p-001", "--title", "T", "--type", "Enhancement", "--paths", "a/"))
    out(run(root, "waves", "p-001", "--write"))
    assert "| 1 | t-001 | ready now |" in (root / "specs/p-001-r/impl.md").read_text()
