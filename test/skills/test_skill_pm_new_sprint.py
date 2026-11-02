import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

import pytest

SCRIPT = Path(__file__).resolve().parents[2] / "templates/skills/pm-new-sprint/scripts/sprints.py"
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


def test_pm_new_sprint_assets_scaffolding(workspace: Path) -> None:
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
        "--effort", "2.0",
    ))
    sprint_dir = workspace / "sprints" / "s-001-alpha-launch"
    assert (sprint_dir / "sprint.md").is_file()
    assert (sprint_dir / "business-requirements-document.html").is_file()
    assert (sprint_dir / "functional-requirements-document.html").is_file()
    assert (sprint_dir / "functional-specification-document.html").is_file()

    # Content checks in sprint.md
    sprint_text = (sprint_dir / "sprint.md").read_text(encoding="utf-8")
    assert "Sprint s-001: Alpha Launch" in sprint_text
    assert "p-002-sample" in sprint_text
    assert "Must Have" in sprint_text
    assert "80.00" in sprint_text

    # HTML docs checks
    for doc_name in (
        "business-requirements-document.html",
        "functional-requirements-document.html",
        "functional-specification-document.html",
    ):
        html_text = (sprint_dir / doc_name).read_text(encoding="utf-8")
        assert "<script" not in html_text.lower()
        assert "javascript:" not in html_text.lower()
        assert "http://" not in html_text
        assert "https://" not in html_text
        assert "s-001-alpha-launch" in html_text
        assert "Alpha Launch" in html_text


def test_html_escaping_against_xss(workspace: Path) -> None:
    out(run(
        workspace,
        "create",
        "--title", "XSS <script>alert(1)</script> Test & 'Quotes'",
        "--slug", "xss-test",
    ))
    sprint_dir = workspace / "sprints" / "s-001-xss-test"
    for doc_name in (
        "business-requirements-document.html",
        "functional-requirements-document.html",
        "functional-specification-document.html",
    ):
        html_text = (sprint_dir / doc_name).read_text(encoding="utf-8")
        assert "<script>" not in html_text
        assert "&lt;script&gt;alert(1)&lt;/script&gt;" in html_text
        assert "&amp;" in html_text
