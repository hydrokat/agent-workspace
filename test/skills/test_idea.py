import ast
import json
import os
import stat
import subprocess
import sys
from pathlib import Path
from typing import Any

import pytest

SCRIPT = Path(__file__).resolve().parents[2] / "templates/skills/idea/scripts/idea.py"


def run(cwd: Path, *args: str, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    base_env = dict(os.environ)
    for k in (
        "TANDEM_FACTORY",
        "TANDEM_ROLE",
        "TANDEM_FACTORY_IDEA",
        "TANDEM_SELF",
        "TANDEM_BIN",
        "XDG_STATE_HOME",
    ):
        base_env.pop(k, None)
    if env:
        base_env.update(env)
    return subprocess.run(
        [sys.executable, str(SCRIPT), *args],
        capture_output=True,
        text=True,
        cwd=cwd,
        env=base_env,
    )


def out(proc: subprocess.CompletedProcess[str]) -> dict[str, Any]:
    assert proc.returncode == 0, proc.stdout + proc.stderr
    return json.loads(proc.stdout)


def err(proc: subprocess.CompletedProcess[str], code: int = 1) -> dict[str, Any]:
    assert proc.returncode == code, proc.stdout + proc.stderr
    raw = proc.stderr.strip() or proc.stdout.strip()
    data: dict[str, Any] = json.loads(raw)
    assert "error" in data
    return data


def test_slug_cases(tmp_path: Path) -> None:
    d1 = out(run(tmp_path, "slug", "Export the audit log as CSV!"))
    assert d1["slug"] == "export-the-audit-log-as-csv"
    assert len(d1["date"]) == 10 and d1["date"][4] == "-" and d1["date"][7] == "-"

    long_text = "word " * 40
    d2 = out(run(tmp_path, "slug", long_text))
    assert len(d2["slug"]) <= 60
    assert not d2["slug"].endswith("-")

    d3 = out(run(tmp_path, "slug", "!!!"))
    assert d3["slug"] == "idea"

    d4 = out(run(tmp_path, "slug", "Café naïve résumé 日本語"))
    assert isinstance(d4["slug"], str) and len(d4["slug"]) > 0


def test_verdict_reads_machine_readable_line(tmp_path: Path) -> None:
    feas_dir = tmp_path / "specs/feasibility"
    feas_dir.mkdir(parents=True)
    report = feas_dir / "2026-11-10-export-audit-log.md"
    report.write_text(
        "# Feasibility: Export audit log\n"
        '<!-- feasibility: {"verdict": "feasible-with-caveats", "effort": "M", "recommended": "Approach B: streamed export", "unknowns": 2} -->\n\n'
        "**Verdict**: Feasible with caveats\n"
    )
    data = out(run(tmp_path, "verdict", "specs/feasibility/2026-11-10-export-audit-log.md"))
    assert data == {
        "verdict": "feasible-with-caveats",
        "effort": "M",
        "recommended": "Approach B: streamed export",
        "unknowns": 2,
    }


def test_verdict_errors(tmp_path: Path) -> None:
    feas_dir = tmp_path / "specs/feasibility"
    feas_dir.mkdir(parents=True)

    no_line = feas_dir / "2026-11-10-missing.md"
    no_line.write_text("# Feasibility: Missing line\n\n**Verdict**: Feasible\n")
    e1 = err(run(tmp_path, "verdict", "specs/feasibility/2026-11-10-missing.md"), 1)
    assert "feasibility" in e1["error"].lower() or "line" in e1["error"].lower()

    bad_verdict = feas_dir / "2026-11-10-bad-verdict.md"
    bad_verdict.write_text(
        '# Feasibility: Bad\n<!-- feasibility: {"verdict": "maybe", "effort": "M", "recommended": "A", "unknowns": 0} -->\n'
    )
    err(run(tmp_path, "verdict", "specs/feasibility/2026-11-10-bad-verdict.md"), 1)

    bad_effort = feas_dir / "2026-11-10-bad-effort.md"
    bad_effort.write_text(
        '# Feasibility: Bad\n<!-- feasibility: {"verdict": "feasible", "effort": "XXL", "recommended": "A", "unknowns": 0} -->\n'
    )
    err(run(tmp_path, "verdict", "specs/feasibility/2026-11-10-bad-effort.md"), 1)

    outside = tmp_path / "specs/other.md"
    outside.write_text(
        '# Other\n<!-- feasibility: {"verdict": "feasible", "effort": "S", "recommended": "A", "unknowns": 0} -->\n'
    )
    err(run(tmp_path, "verdict", "specs/other.md"), 1)
    err(run(tmp_path, "verdict", "specs/feasibility/../other.md"), 1)


@pytest.mark.parametrize(
    "verdict,auto,expected",
    [
        ("feasible", False, {"decision": "proceed"}),
        ("feasible", True, {"decision": "proceed"}),
        ("feasible-with-caveats", False, {"decision": "ask", "carry": "caveats"}),
        ("feasible-with-caveats", True, {"decision": "proceed", "carry": "caveats"}),
        ("needs-spike", False, {"decision": "stop", "status": "needs-spike"}),
        ("needs-spike", True, {"decision": "stop", "status": "needs-spike"}),
        ("not-feasible", False, {"decision": "stop", "status": "not-feasible"}),
        ("not-feasible", True, {"decision": "stop", "status": "not-feasible"}),
    ],
)
def test_gate_table(tmp_path: Path, verdict: str, auto: bool, expected: dict[str, Any]) -> None:
    args = ["gate", "--verdict", verdict, "--effort", "M"]
    if auto:
        args.append("--auto")
    assert out(run(tmp_path, *args)) == expected


def test_result_formatting_and_validation(tmp_path: Path) -> None:
    proc = run(
        tmp_path,
        "result",
        "--status", "planned",
        "--report", "specs/feasibility/2026-11-10-export-audit-log-as-csv.md",
        "--verdict", "feasible",
        "--effort", "M",
        "--phase", "p-020-agent-workspace-package",
    )
    assert proc.returncode == 0, proc.stdout + proc.stderr
    lines = proc.stdout.strip().splitlines()
    assert len(lines) == 1
    payload = json.loads(lines[0])
    assert "idea_result" in payload
    res = payload["idea_result"]
    assert res["status"] == "planned"
    assert res["phases"] == ["p-020-agent-workspace-package"]
    assert res["next"] == "/spec-implement p-020-agent-workspace-package"
    assert res["slug"] == "export-audit-log-as-csv"

    err(run(tmp_path, "result", "--status", "planned", "--verdict", "feasible", "--effort", "M"), 1)
    err(run(tmp_path, "result", "--status", "bogus"), 1)

    assessed = out(
        run(
            tmp_path,
            "result",
            "--status", "assessed",
            "--report", "specs/feasibility/2026-11-10-export-audit-log-as-csv.md",
            "--verdict", "feasible",
            "--effort", "S",
        )
    )
    assert assessed["idea_result"].get("next") is None

    failed = out(run(tmp_path, "result", "--status", "failed", "--error", "boom"))
    assert failed["idea_result"]["status"] == "failed"
    assert failed["idea_result"]["error"] == "boom"
    assert failed["idea_result"].get("next") is None


def test_mode_detection(tmp_path: Path) -> None:
    empty_state = tmp_path / "empty-state"
    clean_env = {"XDG_STATE_HOME": str(empty_state), "PATH": "/usr/bin:/bin"}

    d_default = out(run(tmp_path, "mode", env=clean_env))
    assert d_default == {"mode": "classic", "reason": "default"}

    d_flag = out(run(tmp_path, "mode", "--factory", env=clean_env))
    assert d_flag == {"mode": "factory", "reason": "flag"}

    d_env_f = out(run(tmp_path, "mode", env={**clean_env, "TANDEM_FACTORY": "1"}))
    assert d_env_f == {"mode": "factory", "reason": "env"}

    d_env_role = out(run(tmp_path, "mode", env={**clean_env, "TANDEM_ROLE": "orchestrator"}))
    assert d_env_role == {"mode": "factory", "reason": "env"}

    d_env_idea = out(run(tmp_path, "mode", env={**clean_env, "TANDEM_FACTORY_IDEA": "i-12345678"}))
    assert d_env_idea == {"mode": "factory", "reason": "env"}

    stub_enabled = tmp_path / "tandem-enabled"
    stub_enabled.write_text(
        '#!/usr/bin/env python3\nimport json\nprint(json.dumps({"enabled": True}))\n',
        encoding="utf-8",
    )
    stub_enabled.chmod(stub_enabled.stat().st_mode | stat.S_IXUSR)
    d_status_on = out(run(tmp_path, "mode", env={**clean_env, "TANDEM_SELF": str(stub_enabled)}))
    assert d_status_on == {"mode": "factory", "reason": "status"}

    stub_disabled = tmp_path / "tandem-disabled"
    stub_disabled.write_text(
        '#!/usr/bin/env python3\nimport json\nprint(json.dumps({"enabled": False}))\n',
        encoding="utf-8",
    )
    stub_disabled.chmod(stub_disabled.stat().st_mode | stat.S_IXUSR)
    d_status_off = out(run(tmp_path, "mode", env={**clean_env, "TANDEM_SELF": str(stub_disabled)}))
    assert d_status_off == {"mode": "classic", "reason": "default"}


@pytest.mark.parametrize(
    "verdict,effort,unknowns,expected_verdict,expected_risk",
    [
        ("feasible", "S", 0, "feasible", "low"),
        ("feasible", "M", 0, "feasible", "low"),
        ("feasible", "M", 1, "feasible", "medium"),
        ("feasible", "L", 0, "feasible", "medium"),
        ("feasible", "XL", 0, "feasible", "high"),
        ("feasible-with-caveats", "S", 0, "feasible-with-risks", "medium"),
        ("feasible-with-caveats", "XL", 0, "feasible-with-risks", "high"),
        ("needs-spike", "S", 1, "infeasible", "high"),
        ("not-feasible", "S", 0, "infeasible", "high"),
    ],
)
def test_factory_map_verdict_and_risk(
    tmp_path: Path,
    verdict: str,
    effort: str,
    unknowns: int,
    expected_verdict: str,
    expected_risk: str,
) -> None:
    mapped = out(
        run(
            tmp_path,
            "factory-map",
            "--verdict",
            verdict,
            "--effort",
            effort,
            "--unknowns",
            str(unknowns),
        )
    )
    assert mapped["factory_verdict"] == expected_verdict
    assert mapped["factory_risk"] == expected_risk


def test_result_supports_factory_idea(tmp_path: Path) -> None:
    proc = run(
        tmp_path,
        "result",
        "--status",
        "planned",
        "--idea",
        "i-7k2m9q4d",
        "--report",
        "specs/feasibility/2026-11-21-export-audit-log.md",
        "--verdict",
        "feasible",
        "--effort",
        "M",
    )
    assert proc.returncode == 0, proc.stdout + proc.stderr
    payload = json.loads(proc.stdout.strip())
    res = payload["idea_result"]
    assert res["status"] == "planned"
    assert res["mode"] == "factory"
    assert res["idea"] == "i-7k2m9q4d"
    assert res["next"] == "tandem factory idea ready i-7k2m9q4d --confirm i-7k2m9q4d"
    assert res["slug"] == "export-audit-log"

    err(run(tmp_path, "factory-map", "--verdict", "bogus", "--effort", "M"), 1)
    err(run(tmp_path, "factory-map", "--verdict", "feasible", "--effort", "XXL"), 1)
    err(
        run(
            tmp_path,
            "factory-map",
            "--verdict",
            "feasible",
            "--effort",
            "M",
            "--unknowns",
            "-1",
        ),
        1,
    )
    err(
        run(
            tmp_path,
            "result",
            "--status",
            "planned",
            "--idea",
            "i-7k2m9q4d",
            "--mode",
            "bogus",
        ),
        1,
    )


def test_idea_script_uses_stdlib_only() -> None:
    tree = ast.parse(SCRIPT.read_text(encoding="utf-8"))
    imported: set[str] = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                imported.add(alias.name.split(".")[0])
        elif isinstance(node, ast.ImportFrom) and node.module:
            imported.add(node.module.split(".")[0])
    non_stdlib = imported - sys.stdlib_module_names
    assert non_stdlib == set(), f"non-stdlib imports found: {non_stdlib}"
