import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
SKILL = ROOT / "templates/skills/spec-explain"
SPECS = ROOT / "templates/skills/spec-planner/scripts/specs.py"
FIXTURE = Path(__file__).resolve().parent / "fixtures/specs"


def specs(*args: str) -> dict[str, Any]:
    proc = subprocess.run([sys.executable, str(SPECS), "--root", str(FIXTURE), *args],
                          capture_output=True, text=True)
    assert proc.returncode == 0, proc.stdout + proc.stderr
    data: dict[str, Any] = json.loads(proc.stdout)
    return data


def data_map_fields() -> set[str]:
    text = (SKILL / "SKILL.md").read_text()
    block = text.split("<!-- data-map -->")[1].split("<!-- /data-map -->")[0]
    return {w for w in re.findall(r"`([a-z_]+)`", block)}


def test_every_field_in_the_data_map_exists_in_specs_output() -> None:
    phase = specs("show", "p-002")
    available = set(phase) | set(phase["tasks"][0]) | {"counts"}
    missing = data_map_fields() - available
    assert not missing, missing
    assert {"type", "status", "description", "affected_paths", "open_questions", "decisions"} <= data_map_fields()


def test_template_has_every_section_of_the_layout() -> None:
    headings = re.findall(r"^#{1,2} (.+)$", (SKILL / "assets/explanation.md").read_text(), re.MULTILINE)
    assert headings[1:] == ["What it fixes", "What it develops", "Why", "Where", "State"]


def test_specs_output_groups_fixes_and_developments_by_type() -> None:
    tasks = specs("show", "p-002")["tasks"]
    fixes = [t["id"] for t in tasks if t["type"] == "Bug Fix"]
    develops = [t["id"] for t in tasks if t["type"] in ("New Feature", "Enhancement")]
    other = [t["id"] for t in tasks if t["type"] == "Delivery"]
    assert (fixes, develops, other) == (["t-002"], ["t-001"], ["t-003"])


def test_phase_without_bug_fixes_has_no_fix_tasks() -> None:
    types = {t["type"] for t in specs("show", "p-001")["tasks"]}
    assert "Bug Fix" not in types


def test_state_fields_cover_progress_questions_and_gates() -> None:
    phase = specs("show", "p-002")
    assert phase["counts"]["Done"] == 1 and phase["counts"]["In Progress"] == 1
    assert len(phase["open_questions"]) == 2
    assert specs("show", "p-002", "--task", "t-002")["task"]["external_gates"]


def test_task_scope_returns_one_task() -> None:
    data = specs("show", "p-002", "--task", "t-002")
    assert data["task"]["id"] == "t-002" and "tasks" not in data


def test_not_found_lists_valid_phases() -> None:
    proc = subprocess.run([sys.executable, str(SPECS), "--root", str(FIXTURE), "resolve", "p-009"],
                          capture_output=True, text=True)
    assert proc.returncode == 1 and "p-002-sample" in json.loads(proc.stdout)["valid"]


def test_skill_declares_name_trigger_and_interface() -> None:
    text = (SKILL / "SKILL.md").read_text()
    assert text.startswith("---\nname: spec-explain\n") and "/spec-explain" in text.split("---")[1]
    assert "default_prompt: \"/spec-explain\"" in (SKILL / "agents/openai.yaml").read_text()
