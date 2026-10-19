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


CONV = "46f2fc12-7f1f-4ca2-b714-06cbbfd669e9"


def _spec_json(launch: object) -> dict[str, object]:
    import json

    return json.loads(launch.spec)  # type: ignore[attr-defined, no-any-return]


def test_new_claude_agent_gets_its_own_conversation_id(tmp_path: Path) -> None:
    one = agent_launch(AgentSpec("claude", "planner", "opus"), coord_dir=str(tmp_path), workspace=tmp_path)
    two = agent_launch(AgentSpec("claude", "planner", "opus"), coord_dir=str(tmp_path), workspace=tmp_path)
    conv = _spec_json(one)["conversation"]
    assert isinstance(conv, str) and one.argv[-2:] == ["--session-id", conv]
    assert conv != _spec_json(two)["conversation"]


def test_claude_resume_reopens_only_a_saved_transcript(tmp_path: Path) -> None:
    claude_dir = tmp_path / "claude"
    spec = AgentSpec("claude", "planner", "opus", conversation=CONV)
    launch = agent_launch(spec, str(tmp_path), tmp_path, resume=True, claude_dir=claude_dir)
    assert launch.argv[-2:] == ["--session-id", CONV]
    (claude_dir / "projects" / "p").mkdir(parents=True)
    (claude_dir / "projects" / "p" / f"{CONV}.jsonl").write_text("{}\n")
    launch = agent_launch(spec, str(tmp_path), tmp_path, resume=True, claude_dir=claude_dir)
    assert launch.argv[-2:] == ["--resume", CONV] and _spec_json(launch)["conversation"] == CONV


def test_agy_launch_logs_per_agent_and_resumes_known_conversation(tmp_path: Path) -> None:
    from tandem import conversations

    log = conversations.agy_log_path(tmp_path, "implementer-1")
    log.parent.mkdir(parents=True)
    log.write_text(f"Created conversation {CONV}\n")
    resumed = agent_launch(AgentSpec("agy", "implementer", "m", conversation=CONV), str(tmp_path), tmp_path, resume=True)
    assert resumed.argv[-4:] == ["--log-file", str(log), "--conversation", CONV] and log.exists()
    fresh = agent_launch(AgentSpec("agy", "implementer", "m", conversation=CONV), str(tmp_path), tmp_path)
    assert "--conversation" not in fresh.argv and _spec_json(fresh)["conversation"] is None
    assert not log.exists(), "a new chat must not inherit the previous agent's log"


def test_resume_specs_drop_claude_chat_when_its_folder_is_gone(tmp_path: Path) -> None:
    from tandem import state
    from tandem.commands import resume_specs

    saved = state.SavedState((AgentSpec("claude", "planner", "opus", 1, False, str(tmp_path / "gone"), CONV),
                              AgentSpec("agy", "implementer", "m", 1, False, str(tmp_path / "gone"), CONV)))
    specs, notes = resume_specs(saved, tmp_path, which=lambda c: "/bin/" + c)
    assert [(s.cwd, s.conversation) for s in specs] == [(None, None), (None, CONV)] and len(notes) == 2
