from pathlib import Path

from tandem.adapters.base import AgentSpec
from tandem.commands import agent_launch, placeholder_launch, status_launch


def test_agent_launch_builds_argv_env_and_label() -> None:
    spec = AgentSpec("claude", "implementer", "sonnet", index=2, cwd="/w/wt")
    launch = agent_launch(spec, coord_dir="/c", workspace=Path("/w"))
    assert launch.argv[:3] == ["claude", "--permission-mode", "auto"]
    assert launch.env["TANDEM_SESSION_ID"] == "implementer-2" and launch.env["TANDEM_COORD_DIR"] == "/c"
    assert launch.cwd == "/w/wt" and launch.label == "claude·sonnet" and launch.kind == "agent"
    assert launch.session_id == "implementer-2" and launch.index == 2


def test_agent_launch_defaults_cwd_to_workspace() -> None:
    launch = agent_launch(AgentSpec("agy", "planner", "m"), coord_dir="/c", workspace=Path("/w"))
    assert launch.cwd == "/w"


def test_placeholders_are_tandem_owned_python_processes() -> None:
    p = placeholder_launch("planner", python="/py")
    assert p.kind == "own" and p.argv == ["/py", "-m", "tandem", "placeholder", "planner"]
    s = status_launch(python="/py")
    assert s.kind == "own" and s.role == "status" and s.argv[-1] == "status"
