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


def test_governance_skills_assets_and_interfaces() -> None:
    expected_skills = {
        "pm-sprint-lock": ("/pm-sprint-lock", "lock-summary.md"),
        "pm-sprint-unlock": ("/pm-sprint-unlock", "unlock-warning.md"),
        "pm-sprint-close": ("/pm-sprint-close", "close-report.md"),
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


def test_governance_lifecycle_flow(workspace: Path) -> None:
    # 1. Create open sprint
    out(run(
        workspace,
        "create",
        "--title", "Gov Flow",
        "--slug", "gov-flow",
        "--phase", "p-002-sample",
    ))

    # 2. Lock sprint
    lock_res = out(run(workspace, "lock", "s-001-gov-flow", "--reason", "Scope frozen for week 1"))
    assert lock_res["status"] == "locked"

    # 3. Attempt unlock without confirm fails
    err(run(workspace, "unlock", "s-001-gov-flow"), code=1)

    # 4. Unlock with confirm and reason succeeds
    unlock_res = out(run(
        workspace,
        "unlock", "s-001-gov-flow",
        "--confirm",
        "--reason", "User requested priority adjustment",
    ))
    assert unlock_res["status"] == "open"

    # 5. Re-lock
    out(run(workspace, "lock", "s-001-gov-flow", "--reason", "Re-locked after adjustment"))

    # 6. Close with carry-over
    close_res = out(run(workspace, "close", "s-001-gov-flow", "--carry-over"))
    assert close_res["status"] == "closed"
