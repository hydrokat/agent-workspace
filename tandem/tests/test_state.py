import json
import os
from pathlib import Path

import pytest

from tandem import state
from tandem.adapters.base import AgentSpec

A = AgentSpec("claude", "planner", "sonnet", 1, False, "/w")
B = AgentSpec("agy", "implementer", "gemini-3", 2, True, None)


def sample() -> state.SavedState:
    return state.SavedState((A, B), {"planner": 1, "implementer": 2}, 40, 10, 1000.0)


def test_round_trip(tmp_path: Path) -> None:
    path = tmp_path / "ws" / ".agent-workspace" / "tandem-session.json"
    state.save(path, sample())
    assert state.load(path) == sample()


def test_file_is_owner_only_json(tmp_path: Path) -> None:
    path = tmp_path / "s.json"
    state.save(path, sample())
    assert os.stat(path).st_mode & 0o777 == 0o600
    assert json.loads(path.read_text())["version"] == state.STATE_VERSION
    assert [p.name for p in tmp_path.iterdir()] == ["s.json"]


def test_missing_corrupt_and_wrong_version_load_as_none(tmp_path: Path) -> None:
    assert state.load(tmp_path / "nope.json") is None
    (tmp_path / "bad.json").write_text("{not json")
    assert state.load(tmp_path / "bad.json") is None
    assert state.parse({"version": 99, "agents": []}) is None
    assert state.parse([1, 2]) is None


def test_failed_save_keeps_previous_file(tmp_path: Path) -> None:
    path = tmp_path / "s.json"
    state.save(path, sample())
    with pytest.raises(ValueError):
        state.save(path, state.SavedState((A,), saved_at=float("nan")))
    assert state.load(path) == sample()
    assert [p.name for p in tmp_path.iterdir()] == ["s.json"]


def test_invalid_agents_are_dropped_not_trusted() -> None:
    good = {"cli": "claude", "role": "planner", "model": "opus", "index": 1}
    bad = [
        {**good, "cli": "bash"}, {**good, "role": "boss"}, {**good, "model": "--rm -rf"}, {**good, "model": 5},
        {**good, "index": 0}, {**good, "index": True}, {**good, "skip_permissions": True},
        {**good, "cwd": "relative/dir"}, {**good, "cwd": 3}, "str", None,
    ]
    parsed = state.parse({"version": 1, "agents": [*bad, good, {**good}]})
    assert parsed is not None and parsed.agents == (AgentSpec("claude", "planner", "opus", 1, False, None),)


def test_no_valid_agents_means_no_state() -> None:
    assert state.parse({"version": 1, "agents": [{"cli": "x"}]}) is None


def test_active_and_layout_values_are_bounded() -> None:
    data = state.to_json(sample())
    data["active"] = {"planner": 1, "implementer": 7, "bogus": 1}
    data["layout"] = {"columns_percent": 500, "status_rows": "10"}
    parsed = state.parse(data)
    assert parsed is not None
    assert parsed.active == {"planner": 1} and parsed.columns_percent is None and parsed.status_rows is None


def test_describe_lists_agents_by_role() -> None:
    text = state.describe(sample(), now=1000.0 + 600)
    assert "10 min ago" in text and "claude·sonnet" in text and "agy·gemini-3" in text


def test_resume_specs_skip_missing_cli_and_fix_deleted_folders(tmp_path: Path) -> None:
    from tandem import commands

    saved = state.SavedState((AgentSpec("claude", "planner", "opus", 1, False, str(tmp_path)),
                              AgentSpec("claude", "planner", "opus", 2, False, str(tmp_path / "gone")),
                              AgentSpec("agy", "implementer", "g", 1, False, None)))
    specs, notes = commands.resume_specs(saved, tmp_path, which=lambda c: "/bin/" + c if c == "claude" else None)
    assert [(s.role, s.index, s.cwd) for s in specs] == [("planner", 1, str(tmp_path)), ("planner", 2, None)]
    assert any("agy is not installed" in n for n in notes) and any("no longer exists" in n for n in notes)


CONV = "46f2fc12-7f1f-4ca2-b714-06cbbfd669e9"


def test_conversations_and_clean_flag_round_trip(tmp_path: Path) -> None:
    path = tmp_path / "s.json"
    saved = state.SavedState((AgentSpec("claude", "planner", "opus", 1, False, None, CONV),), {"planner": 1},
                             None, None, 5.0, clean=False)
    state.save(path, saved)
    assert state.load(path) == saved
    assert json.loads(path.read_text())["version"] == state.STATE_VERSION


def test_version_1_files_still_load_without_conversations() -> None:
    data = {"version": 1, "saved_at": 1.0, "agents": [{"cli": "claude", "role": "planner", "model": "opus", "index": 1}]}
    loaded = state.parse(data)
    assert loaded is not None and loaded.agents[0].conversation is None and loaded.clean


def test_bad_conversation_is_dropped_but_agent_kept() -> None:
    data = {"version": 2, "agents": [{"cli": "agy", "role": "planner", "model": "m", "index": 1,
                                      "conversation": "--resume ../../etc"}]}
    loaded = state.parse(data)
    assert loaded is not None and loaded.agents[0].conversation is None


def test_describe_flags_unclean_exit_and_saved_chats() -> None:
    saved = state.SavedState((AgentSpec("claude", "planner", "opus", 1, False, None, CONV),
                              AgentSpec("agy", "implementer", "m", 1)), clean=False, saved_at=0.0)
    text = state.describe(saved)
    assert "did not exit cleanly" in text and "claude·opus (conversation)" in text and "agy·m," not in text
    assert "agy·m (conversation)" not in text
