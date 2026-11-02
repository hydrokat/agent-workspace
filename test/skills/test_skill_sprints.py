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
