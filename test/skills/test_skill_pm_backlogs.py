import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

import pytest

SCRIPT = Path(__file__).resolve().parents[2] / "templates/skills/pm-new-sprint/scripts/sprints.py"
SKILLS_DIR = Path(__file__).resolve().parents[2] / "templates/skills"
FIXTURE = Path(__file__).resolve().parent / "fixtures/specs"


def run(root: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(SCRIPT), "--root", str(root), *args],
        capture_output=True,
        text=True,
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


@pytest.fixture
def workspace(tmp_path: Path) -> Path:
    specs_src = FIXTURE / "specs"
    specs_dest = tmp_path / "specs"
    shutil.copytree(specs_src, specs_dest)
    (tmp_path / "sprints").mkdir()
    return tmp_path


def test_pm_backlogs_skill_assets_and_metadata() -> None:
    skill_dir = SKILLS_DIR / "pm-backlogs"
    skill_md = skill_dir / "SKILL.md"
    openai_yaml = skill_dir / "agents/openai.yaml"
    backlog_asset = skill_dir / "assets/backlog.md"

    assert skill_md.is_file(), "pm-backlogs missing SKILL.md"
    assert openai_yaml.is_file(), "pm-backlogs missing agents/openai.yaml"
    assert backlog_asset.is_file(), "pm-backlogs missing assets/backlog.md"

    yaml_text = openai_yaml.read_text(encoding="utf-8")
    assert "/pm-backlogs" in yaml_text

    text = skill_md.read_text(encoding="utf-8")
    assert "name: pm-backlogs" in text
    assert "/pm-backlogs" in text
    assert "sprints.py backlog" in text
    assert "5.0" in text  # Capacity limit
    assert "Must Have" in text
    assert "pm-new-sprint" in text
    assert "sprint.md" in text


def test_commands_catalog_includes_pm_backlogs(workspace: Path) -> None:
    res = out(run(workspace, "commands"))
    cmds = {c["command"]: c for c in res["commands"]}
    assert len(res["commands"]) == 10
    assert "/pm-backlogs" in cmds
    assert cmds["/pm-backlogs"]["usable"] is True

    # Check pm-commands docs say ten
    pm_cmds_skill = (SKILLS_DIR / "pm-commands/SKILL.md").read_text(encoding="utf-8")
    assert "nine" not in pm_cmds_skill.lower()
    assert "ten" in pm_cmds_skill.lower()


def test_backlog_add_phase_lint_end_to_end(workspace: Path) -> None:
    # 1. Backlog scan finds incomplete phases
    backlog_data = out(run(workspace, "backlog", "--scope", "all"))
    assert len(backlog_data["phases"]) > 0

    # 2. Create sprint
    out(run(workspace, "create", "--title", "E2E Backlog", "--slug", "e2e-backlog"))

    # 3. Add phase with subset of tasks
    add_res = out(run(
        workspace,
        "add-phase",
        "s-001-e2e-backlog",
        "p-002-sample",
        "--tasks", "t-002",
        "--with-deps",
    ))
    assert "t-002" in add_res["tasks"]

    # 4. Lint passes
    lint_res = out(run(workspace, "lint", "s-001-e2e-backlog"))
    assert lint_res["ok"] is True
