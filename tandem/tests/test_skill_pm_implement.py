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


@pytest.fixture
def workspace(tmp_path: Path) -> Path:
    specs_src = FIXTURE / "specs"
    specs_dest = tmp_path / "specs"
    shutil.copytree(specs_src, specs_dest)
    (tmp_path / "sprints").mkdir()
    return tmp_path


def test_pm_sprint_implement_assets_and_interfaces() -> None:
    skill_dir = SKILLS_DIR / "pm-sprint-implement"
    assert (skill_dir / "SKILL.md").is_file(), "pm-sprint-implement missing SKILL.md"
    assert (skill_dir / "agents/openai.yaml").is_file(), "pm-sprint-implement missing openai.yaml"
    assert (skill_dir / "assets/execution-plan.md").is_file(), "pm-sprint-implement missing execution-plan.md"

    yaml_text = (skill_dir / "agents/openai.yaml").read_text(encoding="utf-8")
    assert "/pm-sprint-implement" in yaml_text, "openai.yaml missing default prompt"

    skill_text = (skill_dir / "SKILL.md").read_text(encoding="utf-8")
    required_keywords = [
        "/pm-sprint-implement",
        "sprints.py next-work",
        "coord.py claim",
        "coord.py heartbeat",
        "coord.py release",
        "specs.py set-status",
        "specs.py waves",
        "specs/in-progress.md",
        "Concurrent Multi-Agent Mode",
        "Orchestrator Parallel Wave Mode",
        "caveman",
        "code-cleanup",
        "Project Scope & Boundary Isolation",
    ]
    for kw in required_keywords:
        assert kw in skill_text, f"SKILL.md missing required instruction/keyword: {kw}"


def test_sprints_next_work_lifecycle_integration(workspace: Path) -> None:
    create_res = out(run(
        workspace,
        "create",
        "--title", "Execution Flow",
        "--slug", "exec-flow",
        "--phase", "p-002-sample",
    ))
    sprint_id = create_res["sprint"]

    next_res = out(run(workspace, "next-work", sprint_id))
    assert next_res["sprint"] == sprint_id
    assert next_res["next_task"] == "t-003"
    assert next_res["task"] is not None

    out(run(workspace, "lock", sprint_id, "--reason", "Sprint locked for execution"))
    locked_next = out(run(workspace, "next-work", sprint_id))
    assert locked_next["sprint"] == sprint_id
    assert locked_next["next_task"] == "t-003"
