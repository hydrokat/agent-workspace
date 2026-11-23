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


def test_idea_flag_overrides_env_before_or_after_subcommand(tmp_path: Path) -> None:
    stub = make_stub_tandem(tmp_path / "bin")
    env = {"TANDEM_SELF": str(stub), "TANDEM_FACTORY_IDEA": "i-oldidea1"}

    p_before = run_factory("--idea", "i-newidea2", "show", env=env)
    assert p_before.returncode == 0, p_before.stderr
    assert json.loads(p_before.stdout)["argv"] == [
        "factory",
        "idea",
        "show",
        "i-newidea2",
        "--json",
    ]

    p_after = run_factory("select-direction", "--idea", "i-newidea3", env={"TANDEM_SELF": str(stub)})
    assert p_after.returncode == 0, p_after.stderr
    assert json.loads(p_after.stdout)["argv"] == [
        "factory",
        "idea",
        "select-direction",
        "i-newidea3",
        "--json",
    ]


def test_new_subcommand_builds_idea_new_argv_without_requiring_idea_env(tmp_path: Path) -> None:
    stub = make_stub_tandem(tmp_path / "bin")
    env = {"TANDEM_SELF": str(stub)}

    p_bind = run_factory("new", "--title", "Fresh Idea", "--text-file", "desc.md", env=env)
    assert p_bind.returncode == 0, p_bind.stderr
    assert json.loads(p_bind.stdout)["argv"] == [
        "factory",
        "idea",
        "new",
        "--title",
        "Fresh Idea",
        "--text-file",
        "desc.md",
        "--start-discovery",
        "--bind-current",
        "--json",
    ]

    p_nobind = run_factory(
        "new", "--title", "Unbound Idea", "--text-file", "desc.md", "--no-bind", env=env
    )
    assert p_nobind.returncode == 0, p_nobind.stderr
    assert json.loads(p_nobind.stdout)["argv"] == [
        "factory",
        "idea",
        "new",
        "--title",
        "Unbound Idea",
        "--text-file",
        "desc.md",
        "--start-discovery",
        "--json",
    ]


def make_stateful_stub_tandem(directory: Path, show_payload: dict | None = None, current_payload: dict | None = None) -> Path:
    directory.mkdir(parents=True, exist_ok=True)
    bin_path = directory / "tandem"
    show_repr = repr(show_payload)
    current_repr = repr(current_payload)
    bin_path.write_text(
        f"""#!/usr/bin/env python3
import json
import sys

argv = sys.argv[1:]
show_payload = {show_repr}
current_payload = {current_repr}

if argv[:3] == ["factory", "idea", "current"]:
    if current_payload is not None:
        print(json.dumps(current_payload))
        sys.exit(0)
    print(json.dumps({{"idea": None, "source": "none"}}))
    sys.exit(0)

if argv[:3] == ["factory", "idea", "show"] and show_payload is not None:
    print(json.dumps(show_payload))
    sys.exit(0)

if argv[:3] == ["factory", "idea", "describe"]:
    idea_id = argv[3]
    print(json.dumps({{"id": idea_id, "title": "Updated Title", "text": "Updated body", "state": "ideation", "argv": argv}}))
    sys.exit(0)

if argv[:3] == ["factory", "idea", "new"]:
    print(json.dumps({{"id": "i-created9", "title": "Created Title", "text": "Created body", "state": "ideation", "argv": argv}}))
    sys.exit(0)

print(json.dumps({{"argv": argv}}))
sys.exit(0)
""",
        encoding="utf-8",
    )
    bin_path.chmod(bin_path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    return bin_path


def test_ensure_idea_reuses_blank_inbox_and_creates_when_described_or_unbound(tmp_path: Path) -> None:
    blank_show = {
        "idea": {
            "id": "i-blank111",
            "title": "New idea",
            "text": "   ",
            "state": "inbox",
            "feasibility": None,
            "plan": None,
        }
    }
    stub_blank = make_stateful_stub_tandem(tmp_path / "bin-blank", show_payload=blank_show)
    p_reuse = run_factory(
        "ensure-idea",
        "--title",
        "Export CSV",
        "--text-file",
        "body.md",
        env={"TANDEM_SELF": str(stub_blank), "TANDEM_FACTORY_IDEA": "i-blank111"},
    )
    assert p_reuse.returncode == 0, p_reuse.stderr
    reuse_out = json.loads(p_reuse.stdout)
    assert reuse_out["action"] == "reused"
    assert reuse_out["idea_id"] == "i-blank111"
    assert reuse_out["idea"]["state"] == "ideation"

    described_show = {
        "idea": {
            "id": "i-active22",
            "title": "Existing Idea",
            "text": "Already has a description",
            "state": "planning",
            "feasibility": {"version": 1},
            "plan": {"version": 1},
        }
    }
    stub_active = make_stateful_stub_tandem(tmp_path / "bin-active", show_payload=described_show)
    p_create = run_factory(
        "ensure-idea",
        "--title",
        "Second Idea",
        "--text-file",
        "body2.md",
        env={"TANDEM_SELF": str(stub_active), "TANDEM_FACTORY_IDEA": "i-active22"},
    )
    assert p_create.returncode == 0, p_create.stderr
    create_out = json.loads(p_create.stdout)
    assert create_out["action"] == "created"
    assert create_out["idea_id"] == "i-created9"

    p_unbound = run_factory(
        "ensure-idea",
        "--title",
        "First Chat Idea",
        "--text-file",
        "body3.md",
        env={"TANDEM_SELF": str(stub_active)},
    )
    assert p_unbound.returncode == 0, p_unbound.stderr
    unbound_out = json.loads(p_unbound.stdout)
    assert unbound_out["action"] == "created"
    assert unbound_out["idea_id"] == "i-created9"


def test_ensure_idea_keeps_a_description_the_user_wrote_when_starting_the_idea(tmp_path: Path) -> None:
    described_inbox = {
        "idea": {
            "id": "i-userdesc",
            "title": "Export reports",
            "text": "Weekly CSV export with totals.",
            "state": "inbox",
            "feasibility": None,
            "plan": None,
        }
    }
    stub = make_stateful_stub_tandem(tmp_path / "bin-described", show_payload=described_inbox)
    proc = run_factory(
        "ensure-idea",
        "--title",
        "Different title",
        "--text-file",
        "body.md",
        env={"TANDEM_SELF": str(stub), "TANDEM_FACTORY_IDEA": "i-userdesc"},
    )
    assert proc.returncode == 0, proc.stderr
    out = json.loads(proc.stdout)
    assert out["action"] == "reused"
    assert out["idea_id"] == "i-userdesc"
    assert out["kept_description"] is True
    assert out["idea"]["text"] == "Weekly CSV export with totals."


def test_pane_current_idea_resolves_when_tmux_pane_set(tmp_path: Path) -> None:
    stub = make_stateful_stub_tandem(
        tmp_path / "bin-pane",
        current_payload={"idea": "i-panebnd1", "source": "pane"},
    )
    proc = run_factory(
        "show",
        env={"TANDEM_SELF": str(stub), "TMUX_PANE": "%4", "TANDEM_FACTORY_IDEA": "i-stale000"},
    )
    assert proc.returncode == 0, proc.stderr
    assert json.loads(proc.stdout)["argv"] == [
        "factory",
        "idea",
        "show",
        "i-panebnd1",
        "--json",
    ]


def test_target_recommend_builds_argv_and_passes_idea_through(tmp_path: Path) -> None:
    stub = make_stub_tandem(tmp_path / "bin")
    env = {"TANDEM_SELF": str(stub), "TANDEM_FACTORY_IDEA": "i-12345678"}

    p_env = run_factory(
        "target-recommend",
        "--repo",
        "api-service",
        "--reason-file",
        "reason.txt",
        env=env,
    )
    assert p_env.returncode == 0, p_env.stdout + p_env.stderr
    assert json.loads(p_env.stdout)["argv"] == [
        "factory",
        "idea",
        "target-recommend",
        "i-12345678",
        "--repo",
        "api-service",
        "--reason-file",
        "reason.txt",
        "--json",
    ]

    p_override = run_factory(
        "target-recommend",
        "--idea",
        "i-override1",
        "--repo",
        "web-client",
        "--reason-file",
        "r.md",
        env={"TANDEM_SELF": str(stub)},
    )
    assert p_override.returncode == 0, p_override.stdout + p_override.stderr
    assert json.loads(p_override.stdout)["argv"] == [
        "factory",
        "idea",
        "target-recommend",
        "i-override1",
        "--repo",
        "web-client",
        "--reason-file",
        "r.md",
        "--json",
    ]

    p_missing = run_factory(
        "target-recommend",
        "--repo",
        "web-client",
        "--reason-file",
        "r.md",
        env={"TANDEM_SELF": str(stub)},
    )
    assert p_missing.returncode == 2
