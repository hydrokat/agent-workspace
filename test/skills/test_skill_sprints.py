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

SCRIPT = Path(__file__).resolve().parents[2] / "templates/skills/pm-new-sprint/scripts/sprints.py"
FIXTURE = Path(__file__).resolve().parent / "fixtures/specs"


def run(root: Path, *args: str, coord_dir: str | None = None) -> subprocess.CompletedProcess[str]:
    env = {**os.environ}
    if coord_dir:
        env["TANDEM_COORD_DIR"] = coord_dir
    return subprocess.run(
        [sys.executable, str(SCRIPT), "--root", str(root), *args],
        capture_output=True,
        text=True,
        env=env,
    )


def out(proc: subprocess.CompletedProcess[str]) -> dict[str, Any]:
    assert proc.returncode == 0, f"stdout: {proc.stdout}\nstderr: {proc.stderr}"
    data: dict[str, Any] = json.loads(proc.stdout)
    return data


def err(proc: subprocess.CompletedProcess[str], code: int = 1) -> dict[str, Any]:
    assert proc.returncode == code, f"stdout: {proc.stdout}\nstderr: {proc.stderr}"
    data: dict[str, Any] = json.loads(proc.stdout)
    assert "error" in data
    return data


def test_no_forbidden_imports() -> None:
    content = SCRIPT.read_text(encoding="utf-8")
    assert "import hashlib" not in content and "from hashlib" not in content
    assert "import secrets" not in content and "from secrets" not in content


@pytest.fixture
def workspace(tmp_path: Path) -> Path:
    # Set up a mock workspace with specs and sprints
    specs_src = FIXTURE / "specs"
    specs_dest = tmp_path / "specs"
    shutil.copytree(specs_src, specs_dest)
    (tmp_path / "sprints").mkdir()
    return tmp_path


def test_next_sprint_empty(workspace: Path) -> None:
    res = out(run(workspace, "next-sprint"))
    assert res["next"] == "001"


def test_next_sprint_increments(workspace: Path) -> None:
    (workspace / "sprints" / "s-001-setup").mkdir()
    (workspace / "sprints" / "s-002-core").mkdir()
    res = out(run(workspace, "next-sprint"))
    assert res["next"] == "003"


def test_resolve(workspace: Path) -> None:
    sprint_dir = workspace / "sprints" / "s-001-setup"
    sprint_dir.mkdir()
    (sprint_dir / "sprint.md").write_text("# Sprint s-001: Setup\n\n## Metadata\n- **Status**: open\n", encoding="utf-8")

    # Resolve by shorthand number
    res1 = out(run(workspace, "resolve", "1"))
    assert res1["sprint"] == "s-001-setup"
    assert res1["number"] == "001"
    assert res1["status"] == "open"

    # Resolve by s-001
    res2 = out(run(workspace, "resolve", "s-001"))
    assert res2["sprint"] == "s-001-setup"

    # Resolve active without arg
    res3 = out(run(workspace, "resolve"))
    assert res3["sprint"] == "s-001-setup"
    assert res3["selected"] == "active"

    # Resolve invalid / path traversal
    err(run(workspace, "resolve", "../escaped"), code=2)
    err(run(workspace, "resolve", "999"), code=1)


def test_create_and_active_sprint_refusal(workspace: Path) -> None:
    # Create first open sprint
    res = out(run(
        workspace,
        "create",
        "--title", "Alpha Launch",
        "--slug", "alpha-launch",
        "--phase", "p-002-sample",
        "--moscow", "Must Have",
        "--reach", "100",
        "--impact", "2.0",
        "--confidence", "0.8",
        "--effort", "2.5",
    ))
    assert res["sprint"] == "s-001-alpha-launch"
    assert res["status"] == "open"
    sprint_dir = workspace / "sprints" / "s-001-alpha-launch"
    assert (sprint_dir / "sprint.md").is_file()
    assert (sprint_dir / "business-requirements-document.html").is_file()
    assert (sprint_dir / "functional-requirements-document.html").is_file()
    assert (sprint_dir / "functional-specification-document.html").is_file()

    # Second active sprint creation should be refused (exit 1)
    err_res = err(run(
        workspace,
        "create",
        "--title", "Beta Launch",
        "--slug", "beta-launch",
    ), code=1)
    assert "active sprint already exists" in err_res["error"]

    # Passing --status planned should succeed
    res_planned = out(run(
        workspace,
        "create",
        "--title", "Beta Launch",
        "--slug", "beta-launch",
        "--status", "planned",
    ))
    assert res_planned["sprint"] == "s-002-beta-launch"
    assert res_planned["status"] == "planned"


def test_rice_validation(workspace: Path) -> None:
    # Invalid Impact
    err(run(
        workspace,
        "create",
        "--title", "Bad Impact",
        "--phase", "p-002-sample",
        "--impact", "5.0",
    ), code=2)

    # Invalid Confidence
    err(run(
        workspace,
        "create",
        "--title", "Bad Confidence",
        "--phase", "p-002-sample",
        "--confidence", "0.9",
    ), code=2)

    # Invalid MoSCoW
    err(run(
        workspace,
        "create",
        "--title", "Bad Moscow",
        "--phase", "p-002-sample",
        "--moscow", "Super Must",
    ), code=2)


def test_discover_and_capacity(workspace: Path) -> None:
    res = out(run(workspace, "discover", "--capacity-days", "5.0"))
    assert "mandatory" in res
    assert "recommended" in res
    assert "optional" in res
    assert res["capacity_days"] == 5.0
    assert "overloaded" in res


def test_lock_and_unlock(workspace: Path) -> None:
    out(run(
        workspace,
        "create",
        "--title", "Lock Test",
        "--slug", "lock-test",
    ))

    # Lock sprint
    lock_res = out(run(workspace, "lock", "s-001-lock-test", "--reason", "Scope frozen"))
    assert lock_res["status"] == "locked"

    # Adding normal phase to locked sprint should fail
    err_res = err(run(
        workspace,
        "add-phase", "s-001-lock-test", "p-002-sample",
        "--moscow", "Must Have",
        "--reach", "100", "--impact", "1.0", "--confidence", "0.8", "--effort", "1.0",
    ), code=1)
    assert "locked" in err_res["error"]

    # Unlock requires confirm and reason
    err(run(workspace, "unlock", "s-001-lock-test"), code=1)
    err(run(workspace, "unlock", "s-001-lock-test", "--confirm"), code=1)

    unlock_res = out(run(workspace, "unlock", "s-001-lock-test", "--confirm", "--reason", "Approved mid-flight addition"))
    assert unlock_res["status"] == "open"


def test_hotfix_bypass_on_locked_sprint(workspace: Path) -> None:
    out(run(
        workspace,
        "create",
        "--title", "Hotfix Test",
        "--slug", "hotfix-test",
    ))
    out(run(workspace, "lock", "s-001-hotfix-test", "--reason", "Locked"))

    # Add phase with hotfix bypass
    res = out(run(
        workspace,
        "add-phase", "s-001-hotfix-test", "p-002-sample",
        "--moscow", "Must Have",
        "--reach", "50", "--impact", "3.0", "--confidence", "1.0", "--effort", "0.5",
        "--hotfix",
        "--reason", "Critical security vulnerability patch",
    ))
    assert res["sprint"] == "s-001-hotfix-test"
    assert "p-002-sample" in res["phases"]


def test_close_sprint_with_and_without_carryover(workspace: Path) -> None:
    out(run(
        workspace,
        "create",
        "--title", "Close Test",
        "--slug", "close-test",
        "--phase", "p-002-sample",
        "--moscow", "Must Have",
    ))

    # Close with unfinished tasks without carryover -> exit 1
    err_res = err(run(workspace, "close", "s-001-close-test"), code=1)
    assert err_res["can_close"] is False
    assert "incomplete_tasks" in err_res
    assert err_res["recommendation"] in ("implement", "carry-over")

    # Close with carry-over flag -> exit 0
    close_res = out(run(workspace, "close", "s-001-close-test", "--carry-over"))
    assert close_res["status"] == "closed"


def test_list_and_show(workspace: Path) -> None:
    out(run(workspace, "create", "--title", "First Sprint", "--slug", "first"))
    list_res = out(run(workspace, "list"))
    assert len(list_res["sprints"]) == 1
    assert list_res["sprints"][0]["slug"] == "first"
    assert list_res["sprints"][0]["status"] == "open"

    show_res = out(run(workspace, "show", "s-001-first"))
    assert show_res["sprint"] == "s-001-first"
    assert "deliverables" in show_res
    assert "audit_log" in show_res


def write_sprint(workspace: Path, name: str, committed_line: str | None, rows: list[tuple[str, str, str]]) -> None:
    folder = workspace / "sprints" / name
    folder.mkdir()
    meta = ["- **Status**: open", "- **AI-Assisted Capacity Limit**: 5.0 Days"]
    if committed_line is not None:
        meta.append(f"- **Committed Capacity**: {committed_line}")
    table = ["| Phase | Title | MoSCoW | Reach | Impact | Confidence | Effort | RICE Score | Spec Link |", "|---|---|---|---|---|---|---|---|---|"]
    table += [f"| {phase} | {phase} | {moscow} | 100 | 1 | 0.8 | {effort} | 10 | [x](../../specs/{phase}/impl.md) |" for phase, moscow, effort in rows]
    (folder / "sprint.md").write_text("# Sprint s-001: Demo\n\n## Metadata\n" + "\n".join(meta) + "\n\n## Prioritized Scope (MoSCoW & RICE)\n\n" + "\n".join(table) + "\n", encoding="utf-8")


def test_committed_effort_is_not_double_counted(workspace: Path) -> None:
    write_sprint(workspace, "s-001-demo", "5.0 Days", [("p-001-a", "Must Have", "2.0d"), ("p-002-b", "Should Have", "3.0d")])
    listed = out(run(workspace, "list"))["sprints"][0]
    assert listed["committed_effort"] == 5.0
    assert out(run(workspace, "show", "s-001-demo"))["committed_effort"] == 5.0


def test_committed_effort_comes_from_the_scope_table_rows(workspace: Path) -> None:
    write_sprint(workspace, "s-001-demo", "9.9 Days", [("p-001-a", "Must Have", "1.5d"), ("p-002-b", "Won't Have", "4d")])
    assert out(run(workspace, "list"))["sprints"][0]["committed_effort"] == 1.5


def test_committed_effort_falls_back_to_the_metadata_line_without_rows(workspace: Path) -> None:
    write_sprint(workspace, "s-001-demo", "4.0 Days", [])
    assert out(run(workspace, "list"))["sprints"][0]["committed_effort"] == 4.0


def test_status_command(workspace: Path) -> None:
    coord_dir = tempfile.mkdtemp(prefix="sprints-coord-")
    out(run(
        workspace,
        "create",
        "--title", "Status Test",
        "--slug", "status-test",
        "--phase", "p-002-sample",
        "--moscow", "Must Have",
    ))
    res = out(run(workspace, "status", "s-001-status-test", coord_dir=coord_dir))
    assert res["sprint"] == "s-001-status-test"
    assert "in_development" in res
    assert "remaining_by_moscow" in res
    assert "eta" in res
    assert "sequential_hours" in res["eta"]
    assert "parallel_waves_hours" in res["eta"]


def test_next_work_selector(workspace: Path) -> None:
    coord_dir = tempfile.mkdtemp(prefix="sprints-coord-")
    out(run(
        workspace,
        "create",
        "--title", "Work Test",
        "--slug", "work-test",
        "--phase", "p-002-sample",
        "--moscow", "Must Have",
    ))
    res = out(run(workspace, "next-work", coord_dir=coord_dir))
    assert res["sprint"] == "s-001-work-test"
    assert res["next_phase"] == "p-002-sample"
    assert res["next_task"] is not None


def test_lint_and_security(workspace: Path) -> None:
    out(run(
        workspace,
        "create",
        "--title", "Lint Test",
        "--slug", "lint-test",
        "--phase", "p-002-sample",
        "--moscow", "Must Have",
    ))

    # Fresh sprint without prose placeholders should pass lint
    res = out(run(workspace, "lint", "s-001-lint-test"))
    assert res["ok"] is True

    # Injecting a script tag into an HTML deliverable must fail lint
    html_file = workspace / "sprints" / "s-001-lint-test" / "business-requirements-document.html"
    html_content = html_file.read_text(encoding="utf-8")
    html_file.write_text(html_content + "\n<script>alert('xss')</script>", encoding="utf-8")

    err_res = err(run(workspace, "lint", "s-001-lint-test"), code=1)
    assert any(p.get("code") == "security-script-tag" for p in err_res.get("problems", []))


def test_commands_catalog(workspace: Path) -> None:
    # Before creating sprint
    res1 = out(run(workspace, "commands"))
    cmds1 = {c["command"]: c for c in res1["commands"]}
    assert cmds1["/pm-new-sprint"]["usable"] is True
    assert cmds1["/pm-sprint-lock"]["usable"] is False

    # After creating open sprint
    out(run(workspace, "create", "--title", "Catalog Test", "--slug", "catalog-test"))
    res2 = out(run(workspace, "commands"))
    cmds2 = {c["command"]: c for c in res2["commands"]}
    assert cmds2["/pm-sprint-lock"]["usable"] is True
    assert cmds2["/pm-sprint-unlock"]["usable"] is False


def test_backlog_unknown_sprint_and_empty(workspace: Path) -> None:
    # Unknown sprint exits 2
    err(run(workspace, "backlog", "--sprint", "nonexistent-sprint"), code=2)

    # Empty specs workspace exits 0 with empty backlog
    empty_ws = workspace / "empty_ws"
    empty_ws.mkdir()
    (empty_ws / "specs").mkdir()
    (empty_ws / "sprints").mkdir()
    res = out(run(empty_ws, "backlog"))
    assert res["target"]["status"] == "none"
    assert res["target"]["can_add"] is False
    assert res["phases"] == []
    assert res["tasks"] == []


def test_backlog_target_block_states(workspace: Path) -> None:
    # 1. No sprint active
    res1 = out(run(workspace, "backlog"))
    assert res1["target"]["sprint"] is None
    assert res1["target"]["status"] == "none"
    assert res1["target"]["can_add"] is False

    # 2. Open sprint active
    out(run(
        workspace,
        "create",
        "--title", "Active Sprint",
        "--slug", "active-sprint",
        "--capacity", "6.0",
    ))
    res2 = out(run(workspace, "backlog"))
    assert res2["target"]["sprint"] == "s-001-active-sprint"
    assert res2["target"]["status"] == "open"
    assert res2["target"]["capacity"] == 6.0
    assert res2["target"]["can_add"] is True

    # 3. Locked sprint active
    out(run(workspace, "lock", "s-001-active-sprint"))
    res3 = out(run(workspace, "backlog"))
    assert res3["target"]["status"] == "locked"
    assert res3["target"]["can_add"] is False


def test_backlog_sources_and_filtering(workspace: Path) -> None:
    # Set up closed sprint s-001 containing a carry-over phase
    s1_dir = workspace / "sprints" / "s-001-closed"
    s1_dir.mkdir()
    (s1_dir / "sprint.md").write_text(
        "# Sprint s-001: Closed\n\n## Metadata\n- **Status**: closed\n\n## Scope\n"
        "| Phase | Title | MoSCoW | Reach | Impact | Confidence | Effort | RICE | Spec Link |\n"
        "|---|---|---|---|---|---|---|---|---|\n"
        "| p-002-sample | Sample Phase | Must Have | 100 | 1 | 0.8 | 1.0d | 80 | ../../specs/p-002-sample/impl.md |\n",
        encoding="utf-8",
    )

    # Set up an active sprint s-002 that commits p-001 and defers p-003-broken as Won't Have
    s2_dir = workspace / "sprints" / "s-002-active"
    s2_dir.mkdir()
    (s2_dir / "sprint.md").write_text(
        "# Sprint s-002: Active\n\n## Metadata\n- **Status**: open\n\n## Scope\n"
        "| Phase | Title | MoSCoW | Reach | Impact | Confidence | Effort | RICE | Spec Link |\n"
        "|---|---|---|---|---|---|---|---|---|\n"
        "| p-001-agent-workspace-package | tandem | Must Have | 100 | 1 | 0.8 | 1.0d | 80 | ../../specs/p-001-agent-workspace-package/impl.md |\n"
        "| p-003-broken | Broken Phase | Won't Have | 100 | 1 | 0.8 | 1.0d | 80 | ../../specs/p-003-broken/impl.md |\n",
        encoding="utf-8",
    )

    # Add an unscheduled incomplete phase p-004-unscheduled
    p4_dir = workspace / "specs" / "p-004-unscheduled"
    p4_dir.mkdir()
    (p4_dir / "impl.md").write_text("# Phase 4\n", encoding="utf-8")
    (p4_dir / "t-001.md").write_text(
        "# t-001: Unscheduled Task\n\n## Objectives\n- [ ] Task\n\n## Status\n- [ ] Pending\n",
        encoding="utf-8",
    )

    res = out(run(workspace, "backlog"))
    phase_names = [p["phase"] for p in res["phases"]]

    # p-001-agent-workspace-package is committed to active sprint -> must not be in backlog
    assert "p-001-agent-workspace-package" not in phase_names

    # phase-001 is fully Done in fixture -> must not be in backlog
    assert "phase-001" not in phase_names

    # p-002-sample was in closed sprint -> source carry-over
    p2 = next(p for p in res["phases"] if p["phase"] == "p-002-sample")
    assert p2["source"] == "carry-over"
    assert p2["sprint"] == "s-001-closed"

    # p-003-broken is Won't Have -> source deferred
    p3 = next(p for p in res["phases"] if p["phase"] == "p-003-broken")
    assert p3["source"] == "deferred"

    # p-004-unscheduled is in no sprint -> source unscheduled
    p4 = next(p for p in res["phases"] if p["phase"] == "p-004-unscheduled")
    assert p4["source"] == "unscheduled"

    # Verify deterministic ordering: carry-over, deferred, outside-subset, unscheduled, then phase number
    sources = [p["source"] for p in res["phases"]]
    assert sources == ["carry-over", "deferred", "unscheduled"]


def test_backlog_task_details_and_claims(workspace: Path) -> None:
    import time

    # Add a blocked task in p-002-sample
    (workspace / "specs" / "p-002-sample" / "t-004.md").write_text(
        "# t-004: Blocked task\n\n## Requirement Classification\n- **Type**: Enhancement\n\n"
        "## Affected Paths\n- `src/blocked.py`\n\n"
        "## Depends On\n- t-002\n\n## Status\n- [ ] Pending\n",
        encoding="utf-8",
    )

    # Set up mock coord overview with a live claim on p-002-sample t-002
    coord_dir = tempfile.mkdtemp(prefix="backlog-coord-")
    claims_dir = Path(coord_dir) / "claims"
    claims_dir.mkdir(parents=True)
    claim_payload = {
        "phase": "p-002-sample",
        "task": "t-002",
        "session": "s-test",
        "heartbeat": time.time(),
    }
    (claims_dir / "p-002-sample__t-002.json").write_text(json.dumps(claim_payload), encoding="utf-8")

    res = out(run(workspace, "backlog", coord_dir=coord_dir))
    p2 = next(p for p in res["phases"] if p["phase"] == "p-002-sample")
    task_map = {t["id"]: t for t in p2["tasks"]}

    # t-001 is Done -> not in backlog tasks
    assert "t-001" not in task_map

    # t-002 is claimed and its dep t-001 is Done -> blocked_by is empty
    t2 = task_map["t-002"]
    assert t2["claimed"] is True
    assert t2["blocked_by"] == []
    assert t2["delivery"] is False

    # t-004 depends on t-002 (which is not Done) -> blocked_by has t-002
    t4 = task_map["t-004"]
    assert "t-002" in t4["blocked_by"]
    assert t4["claimed"] is False

    # Last task in phase is delivery
    last_task = p2["tasks"][-1]
    assert last_task["delivery"] is True


def test_backlog_scopes_and_determinism(workspace: Path) -> None:
    # Scope specs: phase rows have no tasks
    res_specs = out(run(workspace, "backlog", "--scope", "specs"))
    assert "phases" in res_specs
    assert "tasks" not in res_specs
    assert all("tasks" not in p for p in res_specs["phases"])

    # Scope tasks: flat tasks array
    res_tasks = out(run(workspace, "backlog", "--scope", "tasks"))
    assert "tasks" in res_tasks
    assert "phases" not in res_tasks
    assert len(res_tasks["tasks"]) > 0

    # Scope all: both phases (with tasks) and flat tasks
    res_all1 = out(run(workspace, "backlog", "--scope", "all"))
    assert "phases" in res_all1
    assert "tasks" in res_all1
    assert all("tasks" in p for p in res_all1["phases"])

    # Determinism: two runs produce identical JSON output
    proc1 = run(workspace, "backlog", "--scope", "all")
    proc2 = run(workspace, "backlog", "--scope", "all")
    assert proc1.stdout == proc2.stdout


def test_subset_byte_identical_when_no_subsets(workspace: Path) -> None:
    # A sprint without subsets should never have a Tasks column
    out(run(workspace, "create", "--title", "Golden Sprint", "--slug", "golden", "--phase", "p-002-sample"))
    sprint_file = workspace / "sprints" / "s-001-golden" / "sprint.md"
    content_before = sprint_file.read_text(encoding="utf-8")
    assert "| Tasks |" not in content_before

    # Re-reading and rendering should maintain no Tasks column
    sprint_dir = workspace / "sprints" / "s-001-golden"
    model = out(run(workspace, "show", "s-001-golden"))
    assert "| Tasks |" not in sprint_file.read_text(encoding="utf-8")


def test_add_phase_unknown_and_done_tasks_exit_2(workspace: Path) -> None:
    out(run(workspace, "create", "--title", "Subset Test", "--slug", "subset-test"))
    # Unknown task exits 2
    err(run(workspace, "add-phase", "s-001-subset-test", "p-002-sample", "--tasks", "t-999"), code=2)
    # Done task (t-001 in p-002-sample) exits 2
    err(run(workspace, "add-phase", "s-001-subset-test", "p-002-sample", "--tasks", "t-001"), code=2)


def test_add_phase_dependencies_and_with_deps(workspace: Path) -> None:
    # Add a task t-004 depending on t-002
    (workspace / "specs" / "p-002-sample" / "t-004.md").write_text(
        "# t-004: Dependent\n\n## Depends On\n- t-002\n\n## Status\n- [ ] Pending\n",
        encoding="utf-8",
    )
    out(run(workspace, "create", "--title", "Dep Test", "--slug", "dep-test"))

    # Without --with-deps, adding t-004 fails with code 1 naming t-002
    res_err = err(run(workspace, "add-phase", "s-001-dep-test", "p-002-sample", "--tasks", "t-004"), code=1)
    assert "t-002" in res_err["error"]

    # With --with-deps, t-002 is auto-added
    res_ok = out(run(workspace, "add-phase", "s-001-dep-test", "p-002-sample", "--tasks", "t-004", "--with-deps"))
    assert "t-002" in res_ok["tasks"]
    assert "t-004" in res_ok["tasks"]
    assert res_ok["auto_added"] == ["t-002"]
    assert res_ok["delivery_excluded"] is False  # t-004 is last task in p-002-sample


def test_add_phase_merge_and_delivery_excluded(workspace: Path) -> None:
    out(run(workspace, "create", "--title", "Merge Test", "--slug", "merge-test"))
    # Add only t-002: delivery task t-003 excluded
    res1 = out(run(workspace, "add-phase", "s-001-merge-test", "p-002-sample", "--tasks", "t-002"))
    assert res1["tasks"] == ["t-002"]
    assert res1["delivery_excluded"] is True
    assert res1["effort"] == 0.5

    # Merge t-003 into existing subset: now delivery included
    res2 = out(run(workspace, "add-phase", "s-001-merge-test", "p-002-sample", "--tasks", "t-003"))
    assert res2["tasks"] == ["t-002", "t-003"]
    assert res2["delivery_excluded"] is False
    assert res2["effort"] == 1.0

    # Adding a whole phase twice is rejected
    out(run(workspace, "add-phase", "s-001-merge-test", "phase-001"))
    err(run(workspace, "add-phase", "s-001-merge-test", "phase-001"), code=1)


def test_hand_written_sections_preserved(workspace: Path) -> None:
    out(run(workspace, "create", "--title", "Preserve Test", "--slug", "preserve-test"))
    sprint_file = workspace / "sprints" / "s-001-preserve-test" / "sprint.md"
    orig_text = sprint_file.read_text(encoding="utf-8")

    # Inject custom section
    custom_section = "\n## Business Objectives & Context\nImportant business background.\n"
    sprint_file.write_text(orig_text.replace("## Deliverables", custom_section + "\n## Deliverables"), encoding="utf-8")

    out(run(workspace, "add-phase", "s-001-preserve-test", "p-002-sample"))

    # Verify custom section survived
    updated_text = sprint_file.read_text(encoding="utf-8")
    assert "## Business Objectives & Context" in updated_text
    assert "Important business background." in updated_text


def test_subset_aware_commands(workspace: Path) -> None:
    # In p-002-sample, t-001 is Done, t-002 is In Progress, t-003 is Pending
    out(run(workspace, "create", "--title", "Subset Commands", "--slug", "subset-cmds"))
    out(run(workspace, "add-phase", "s-001-subset-cmds", "p-002-sample", "--tasks", "t-003"))

    # status: only subset tasks are counted (t-002 in progress is outside subset)
    status_res = out(run(workspace, "status", "s-001-subset-cmds"))
    assert status_res["total_remaining_tasks"] == 1
    assert len(status_res["in_development"]) == 0
    assert len(status_res["remaining_by_moscow"]["Must Have"]) == 1
    assert status_res["remaining_by_moscow"]["Must Have"][0]["task"] == "t-003"

    # lint: valid subset passes
    lint_res = out(run(workspace, "lint", "s-001-subset-cmds"))
    assert lint_res["ok"] is True

    # next-work must not offer t-002 because t-002 is outside the subset
    next_res = out(run(workspace, "next-work", "s-001-subset-cmds"))
    assert next_res["next_task"] == "t-003"

    # close should fail while t-003 is Pending
    err(run(workspace, "close", "s-001-subset-cmds"), code=1)

    # close with --carry-over should carry over ONLY t-003, not t-002
    close_carry = out(run(workspace, "close", "s-001-subset-cmds", "--carry-over"))
    assert close_carry["status"] == "closed"
    assert len(close_carry["carried_over"]) == 1
    assert close_carry["carried_over"][0]["task"] == "t-003"


def test_lint_detects_unknown_subset_task(workspace: Path) -> None:
    out(run(workspace, "create", "--title", "Lint Subset", "--slug", "lint-subset"))
    out(run(workspace, "add-phase", "s-001-lint-subset", "p-002-sample", "--tasks", "t-003"))

    sprint_file = workspace / "sprints" / "s-001-lint-subset" / "sprint.md"
    orig_content = sprint_file.read_text(encoding="utf-8")
    sprint_file.write_text(orig_content.replace("t-003", "t-999"), encoding="utf-8")

    err_lint = err(run(workspace, "lint", "s-001-lint-subset"), code=1)
    assert any(p["code"] == "subset-unknown-task" for p in err_lint["problems"])


def test_archived_phase_close_status_and_spec_link(workspace: Path) -> None:
    # Set up git repo for prune to work
    subprocess.run(["git", "init"], cwd=workspace, check=True, capture_output=True)
    subprocess.run(["git", "config", "user.email", "test@test.com"], cwd=workspace, check=True, capture_output=True)
    subprocess.run(["git", "config", "user.name", "Test"], cwd=workspace, check=True, capture_output=True)

    # Complete p-002-sample
    t1_text = (workspace / "specs/p-002-sample/t-001.md").read_text()
    (workspace / "specs/p-002-sample/t-002.md").write_text(t1_text.replace("t-001", "t-002"))
    (workspace / "specs/p-002-sample/t-003.md").write_text(t1_text.replace("t-001", "t-003"))

    # Create sprint containing p-002-sample
    out(run(workspace, "create", "--title", "Archived Sprint", "--slug", "archived-sprint", "--phase", "p-002-sample"))

    # Compile so p-002 is in completed.md
    specs_script = Path(__file__).resolve().parents[2] / "templates/skills/spec-planner/scripts/specs.py"
    subprocess.run([sys.executable, str(specs_script), "--root", str(workspace), "compile"], check=True)

    # Commit git state so prune is clean
    subprocess.run(["git", "add", "-A"], cwd=workspace, check=True, capture_output=True)
    subprocess.run(["git", "commit", "-m", "pre-prune"], cwd=workspace, check=True, capture_output=True)

    subprocess.run([sys.executable, str(specs_script), "--root", str(workspace), "prune", "--phases", "p-002-sample", "--apply"], check=True)
    assert not (workspace / "specs/p-002-sample").exists()

    status_res = out(run(workspace, "status", "s-001-archived-sprint"))
    assert status_res["total_remaining_tasks"] == 0
    assert len(status_res["in_development"]) == 0
    assert status_res.get("warnings") == []

    next_res = out(run(workspace, "next-work", "s-001-archived-sprint"))
    assert next_res["next_task"] is None
    assert next_res.get("warnings") == []

    # close: closes cleanly without --carry-over
    close_res = out(run(workspace, "close", "s-001-archived-sprint"))
    assert close_res["status"] == "closed"

    # sprint.md Spec Link rewritten to completed.md
    sprint_text = (workspace / "sprints/s-001-archived-sprint/sprint.md").read_text()
    assert "[p-002-sample](../../specs/compiled/completed.md)" in sprint_text
    assert "[p-002-sample](../../specs/p-002-sample/impl.md)" not in sprint_text


def test_unreadable_phase_fails_close_and_warns_in_status_and_next_work(workspace: Path) -> None:
    out(run(workspace, "create", "--title", "Unreadable Sprint", "--slug", "unreadable-sprint"))
    sprint_file = workspace / "sprints/s-001-unreadable-sprint/sprint.md"
    orig_text = sprint_file.read_text()
    # Inject a non-existent phase into Prioritized Scope
    injected_row = "| p-999-missing | Missing Phase | Must Have | 100.0 | 1.0 | 0.8 | 1.0d | 80.00 | [p-999-missing](../../specs/p-999-missing/impl.md) |\n"
    new_text = re.sub(r"\| (?:\[Phase Directory\]|None)[^\n]+\n", injected_row, orig_text)
    sprint_file.write_text(new_text)

    status_res = out(run(workspace, "status", "s-001-unreadable-sprint"))
    assert "warnings" in status_res
    assert any("p-999-missing" in w for w in status_res["warnings"])

    next_res = out(run(workspace, "next-work", "s-001-unreadable-sprint"))
    assert "warnings" in next_res
    assert any("p-999-missing" in w for w in next_res["warnings"])

    # close: fails plain close reporting unreadable_phases
    err_close = err(run(workspace, "close", "s-001-unreadable-sprint"), code=1)
    assert err_close.get("can_close") is False
    assert "unreadable_phases" in err_close
    assert "p-999-missing" in err_close["unreadable_phases"]

    # lint: detects broken spec link to phase that is nowhere
    err_lint = err(run(workspace, "lint", "s-001-unreadable-sprint"), code=1)
    assert any(p.get("code") in ("broken-spec-link", "missing-spec-phase") for p in err_lint.get("problems", []))




