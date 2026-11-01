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


def test_readonly_skills_assets_and_interfaces() -> None:
    expected_skills = {
        "pm-sprint-explain": ("/pm-sprint-explain", "explanation.md"),
        "pm-sprint-status": ("/pm-sprint-status", "status.md"),
        "pm-sprint-list": ("/pm-sprint-list", "list.md"),
        "pm-commands": ("/pm-commands", "commands.md"),
    }

    for skill_name, (prompt, asset_name) in expected_skills.items():
        skill_path = SKILLS_DIR / skill_name
        assert (skill_path / "SKILL.md").is_file(), f"{skill_name} missing SKILL.md"
        assert (skill_path / "agents/openai.yaml").is_file(), f"{skill_name} missing openai.yaml"
        assert (skill_path / "assets" / asset_name).is_file(), f"{skill_name} missing {asset_name}"

        yaml_text = (skill_path / "agents/openai.yaml").read_text(encoding="utf-8")
        assert prompt in yaml_text, f"{prompt} not in {skill_name} openai.yaml"

        skill_text = (skill_path / "SKILL.md").read_text(encoding="utf-8")
        assert prompt in skill_text, f"{prompt} not in {skill_name} SKILL.md"
        assert "Read-Only" in skill_text or "read-only" in skill_text, f"{skill_name} missing Read-Only mandate"


def test_readonly_empty_state_and_commands(workspace: Path) -> None:
    # Empty workspace commands
    cmd_res = out(run(workspace, "commands"))
    assert cmd_res["active_status"] is None
    cmds = {c["command"]: c for c in cmd_res["commands"]}
    assert cmds["/pm-new-sprint"]["usable"] is True
    assert cmds["/pm-sprint-explain"]["usable"] is False
    assert cmds["/pm-sprint-lock"]["usable"] is False

    # Empty list
    list_res = out(run(workspace, "list"))
    assert list_res["sprints"] == []


def test_readonly_data_map_contracts(workspace: Path) -> None:
    out(run(
        workspace,
        "create",
        "--title", "Readonly Contract",
        "--slug", "ro-contract",
        "--phase", "p-002-sample",
    ))

    # Test show keys for explain
    show_res = out(run(workspace, "show", "s-001-ro-contract"))
    assert all(k in show_res for k in ("sprint", "title", "status", "start_date", "end_date", "phases", "deliverables"))

    # Test status keys
    status_res = out(run(workspace, "status", "s-001-ro-contract"))
    assert all(k in status_res for k in ("sprint", "status", "in_development", "remaining_by_moscow", "eta"))

    # Test list keys
    list_res = out(run(workspace, "list"))
    assert len(list_res["sprints"]) == 1
    item = list_res["sprints"][0]
    assert all(k in item for k in ("sprint", "number", "slug", "title", "status", "phases_count"))
