import sys
from pathlib import Path
from typing import Any

import pytest

from tandem import cli, commands, state
from tandem.adapters.base import AgentSpec
from tandem.config import Config

SAVED = state.SavedState((AgentSpec("claude", "planner", "opus", 1, False, None),), {"planner": 1}, 40, 10, 1.0)
WIZARD = [AgentSpec("claude", "implementer", "sonnet", 1)]


class FakeLayout:
    def exists(self) -> bool:
        return False


@pytest.fixture
def started(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> dict[str, Any]:
    seen: dict[str, Any] = {}
    monkeypatch.setattr(commands, "make_layout", lambda w, c: FakeLayout())
    monkeypatch.setattr(commands, "start", lambda specs, w, c, saved=None: seen.update(specs=specs, saved=saved))
    monkeypatch.setattr(commands, "resume_specs", lambda s, w: (list(s.agents), []))
    monkeypatch.setattr("tandem.wizard.runner.run_launch_wizard", lambda: WIZARD)
    monkeypatch.setattr(sys.stdin, "isatty", lambda: False)
    return seen


def write_state(ws: Path) -> None:
    state.save(state.state_path(ws), SAVED)


def test_resume_flag_starts_from_saved_state(tmp_path: Path, started: dict[str, Any]) -> None:
    write_state(tmp_path)
    cli._launch(tmp_path, Config.from_env({}), resume=True)
    assert started["specs"] == list(SAVED.agents) and started["saved"] == SAVED


def test_fresh_flag_ignores_saved_state_but_keeps_the_file(tmp_path: Path, started: dict[str, Any]) -> None:
    write_state(tmp_path)
    cli._launch(tmp_path, Config.from_env({}), fresh=True)
    assert started["specs"] == WIZARD and started["saved"] is None
    assert state.load(state.state_path(tmp_path)) == SAVED


def test_resume_without_saved_state_is_an_error(tmp_path: Path, started: dict[str, Any]) -> None:
    with pytest.raises(SystemExit) as exit_info:
        cli._launch(tmp_path, Config.from_env({}), resume=True)
    assert exit_info.value.code == 1


def test_no_terminal_means_blank_start(tmp_path: Path, started: dict[str, Any]) -> None:
    write_state(tmp_path)
    cli._launch(tmp_path, Config.from_env({}))
    assert started["specs"] == WIZARD


@pytest.mark.parametrize(("answer", "expected"), [("", True), ("r", True), ("b", False), ("blank", False)])
def test_prompt_choices(monkeypatch: pytest.MonkeyPatch, answer: str, expected: bool) -> None:
    monkeypatch.setattr(sys.stdin, "isatty", lambda: True)
    monkeypatch.setattr("builtins.input", lambda prompt="": answer)
    assert cli._ask_resume(SAVED) is expected


def test_prompt_quit_exits(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(sys.stdin, "isatty", lambda: True)
    monkeypatch.setattr("builtins.input", lambda prompt="": "q")
    with pytest.raises(SystemExit):
        cli._ask_resume(SAVED)


def test_flags_are_mutually_exclusive() -> None:
    with pytest.raises(SystemExit):
        cli.build_parser().parse_args(["--resume", "--fresh"])


def test_quit_menu_offers_save_update_detach_discard() -> None:
    calls: list[tuple[str, ...]] = []

    class Client:
        def run_checked(self, *args: str) -> str:
            calls.append(args)
            return ""

    class L:
        client = Client()

    commands.quit_menu(L(), "/ws")  # type: ignore[arg-type]
    flat = " | ".join(calls[0])
    assert "save-exit" in flat and "tandem update'" in flat and "detach-client" in flat and "discard-exit" in flat
    assert "TANDEM_WORKSPACE=/ws" in flat


def test_legacy_shared_session_is_mentioned_not_reused(monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]) -> None:
    from tandem.tmux.client import Result, TmuxClient

    monkeypatch.setattr(TmuxClient, "run", lambda self, *a: Result(0 if self.socket == "tandem" else 1, "", ""))
    cli._note_legacy_session()
    assert "kill-server" in capsys.readouterr().err
    monkeypatch.setattr(TmuxClient, "run", lambda self, *a: Result(1, "", ""))
    cli._note_legacy_session()
    assert capsys.readouterr().err == ""
