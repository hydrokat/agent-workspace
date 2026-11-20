import json
import os
import stat
import subprocess
import sys
from pathlib import Path

import pytest

SCRIPT = (
    Path(__file__).resolve().parents[2]
    / "templates/skills/factory-orchestrator/scripts/factory.py"
)


def make_stub_tandem(directory: Path, name: str = "tandem", exit_code: int = 0) -> Path:
    directory.mkdir(parents=True, exist_ok=True)
    bin_path = directory / name
    bin_path.write_text(
        f"""#!/usr/bin/env python3
import json
import sys

print(json.dumps({{"argv": sys.argv[1:], "bin": sys.argv[0]}}))
sys.exit({exit_code})
""",
        encoding="utf-8",
    )
    bin_path.chmod(bin_path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    return bin_path


def run_factory(
    *args: str,
    env: dict[str, str] | None = None,
    clean_env: bool = False,
) -> subprocess.CompletedProcess[str]:
    base_env = {} if clean_env else dict(os.environ)
    for k in ("TANDEM_SELF", "TANDEM_BIN", "XDG_STATE_HOME", "TANDEM_FACTORY_IDEA"):
        base_env.pop(k, None)
    if clean_env:
        base_env["PATH"] = ""
        base_env["HOME"] = "/nonexistent"
    if env:
        base_env.update(env)
    return subprocess.run(
        [sys.executable, str(SCRIPT), *args],
        capture_output=True,
        text=True,
        env=base_env,
    )


def test_missing_idea_env_exits_2(tmp_path: Path) -> None:
    bin_dir = tmp_path / "bin"
    stub = make_stub_tandem(bin_dir)
    proc = run_factory("show", env={"TANDEM_SELF": str(stub)})
    assert proc.returncode == 2
    combined = (proc.stdout + proc.stderr).strip().splitlines()
    assert len(combined) == 1
    assert "TANDEM_FACTORY_IDEA" in combined[0]


def test_missing_binary_exits_2(tmp_path: Path) -> None:
    proc = run_factory(
        "show",
        env={
            "TANDEM_FACTORY_IDEA": "i-12345678",
            "XDG_STATE_HOME": str(tmp_path / "state"),
            "PATH": str(tmp_path / "empty-path"),
        },
    )
    assert proc.returncode == 2
    combined = (proc.stdout + proc.stderr).strip().splitlines()
    assert len(combined) == 1
    assert "tandem" in combined[0].lower()


def test_binary_resolution_order(tmp_path: Path) -> None:
    self_bin = make_stub_tandem(tmp_path / "self", "tandem-self")
    env_bin = make_stub_tandem(tmp_path / "env", "tandem-bin")
    state_home = tmp_path / "state"
    state_bin = make_stub_tandem(
        state_home / "tandem" / "bin" / "current" / "tandem", "tandem"
    )
    path_dir = tmp_path / "path"
    path_bin = make_stub_tandem(path_dir, "tandem")

    base = {
        "TANDEM_FACTORY_IDEA": "i-12345678",
        "XDG_STATE_HOME": str(state_home),
        "PATH": f"{path_dir}{os.pathsep}/usr/bin{os.pathsep}/bin",
    }

    # 1. TANDEM_SELF wins over all others
    p1 = run_factory(
        "show",
        env={**base, "TANDEM_SELF": str(self_bin), "TANDEM_BIN": str(env_bin)},
    )
    assert p1.returncode == 0, p1.stderr
    assert json.loads(p1.stdout)["bin"] == str(self_bin)

    # 2. TANDEM_BIN wins over state dir and PATH
    p2 = run_factory("show", env={**base, "TANDEM_BIN": str(env_bin)})
    assert p2.returncode == 0, p2.stderr
    assert json.loads(p2.stdout)["bin"] == str(env_bin)

    # 3. XDG_STATE_HOME candidate wins over PATH
    p3 = run_factory("show", env=base)
    assert p3.returncode == 0, p3.stderr
    assert json.loads(p3.stdout)["bin"] == str(state_bin)

    # 4. PATH fallback when state dir candidate does not exist
    p4 = run_factory(
        "show",
        env={**base, "XDG_STATE_HOME": str(tmp_path / "missing-state")},
    )
    assert p4.returncode == 0, p4.stderr
    assert json.loads(p4.stdout)["bin"] == str(path_bin)


def test_subcommand_argv_construction(tmp_path: Path) -> None:
    stub = make_stub_tandem(tmp_path / "bin")
    env = {"TANDEM_SELF": str(stub), "TANDEM_FACTORY_IDEA": "i-deadbeef"}

    cases = [
        (["show"], ["factory", "idea", "show", "i-deadbeef", "--json"]),
        (
            ["describe", "--title", "My Idea", "--text-file", "desc.md"],
            [
                "factory",
                "idea",
                "describe",
                "i-deadbeef",
                "--title",
                "My Idea",
                "--text-file",
                "desc.md",
                "--json",
            ],
        ),
        (
            ["feasibility", "--file", "study.md", "--verdict", "feasible", "--risk", "low"],
            [
                "factory",
                "artifact",
                "put",
                "i-deadbeef",
                "--kind",
                "feasibility",
                "--file",
                "study.md",
                "--verdict",
                "feasible",
                "--risk",
                "low",
                "--json",
            ],
        ),
        (
            ["plan", "--file", "plan.md", "--workflow", "spec-driven"],
            [
                "factory",
                "artifact",
                "put",
                "i-deadbeef",
                "--kind",
                "plan",
                "--file",
                "plan.md",
                "--workflow",
                "spec-driven",
                "--json",
            ],
        ),
        (
            ["assume", "--text", "Postgres 16 is available"],
            ["factory", "idea", "assume", "i-deadbeef", "--text", "Postgres 16 is available", "--json"],
        ),
        (["question-check"], ["factory", "question", "check", "i-deadbeef", "--json"]),
        (["select-direction"], ["factory", "idea", "select-direction", "i-deadbeef", "--json"]),
    ]

    for cli_args, expected_argv in cases:
        proc = run_factory(*cli_args, env=env)
        assert proc.returncode == 0, f"{cli_args}: {proc.stderr}"
        assert json.loads(proc.stdout)["argv"] == expected_argv


def test_passes_exit_code_through(tmp_path: Path) -> None:
    stub = make_stub_tandem(tmp_path / "bin", exit_code=1)
    proc = run_factory(
        "select-direction",
        env={"TANDEM_SELF": str(stub), "TANDEM_FACTORY_IDEA": "i-deadbeef"},
    )
    assert proc.returncode == 1
    assert json.loads(proc.stdout)["argv"] == [
        "factory",
        "idea",
        "select-direction",
        "i-deadbeef",
        "--json",
    ]


def test_no_ready_or_revoke_ready_subcommand(tmp_path: Path) -> None:
    stub = make_stub_tandem(tmp_path / "bin")
    env = {"TANDEM_SELF": str(stub), "TANDEM_FACTORY_IDEA": "i-deadbeef"}
    for forbidden in ("ready", "revoke-ready"):
        proc = run_factory(forbidden, env=env)
        assert proc.returncode == 2


def test_recommend_builds_models_recommend_argv(tmp_path: Path) -> None:
    stub = make_stub_tandem(tmp_path / "bin")
    env = {"TANDEM_SELF": str(stub), "TANDEM_FACTORY_IDEA": "i-12345678"}
    proc = run_factory(
        "recommend", "--run", "r-7k2m9x4q", "--stage", "develop",
        "--complexity", "high", "--required-tier", "standard", env=env,
    )
    assert proc.returncode == 0, proc.stdout + proc.stderr
    argv = json.loads(proc.stdout)["argv"]
    assert argv == [
        "factory", "models", "recommend", "--run", "r-7k2m9x4q", "--stage", "develop",
        "--complexity", "high", "--required-tier", "standard", "--json",
    ]


def test_recommend_minimal_and_validates_choices(tmp_path: Path) -> None:
    stub = make_stub_tandem(tmp_path / "bin")
    env = {"TANDEM_SELF": str(stub), "TANDEM_FACTORY_IDEA": "i-12345678"}
    ok = run_factory("recommend", "--run", "r-7k2m9x4q", "--stage", "review", env=env)
    assert json.loads(ok.stdout)["argv"] == [
        "factory", "models", "recommend", "--run", "r-7k2m9x4q", "--stage", "review", "--json",
    ]
    bad = run_factory("recommend", "--run", "r-7k2m9x4q", "--stage", "review", "--complexity", "huge", env=env)
    assert bad.returncode == 2
