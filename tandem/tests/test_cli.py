import pytest

from tandem import __version__
from tandem.cli import main


def test_version_flag(capsys: pytest.CaptureFixture[str]) -> None:
    with pytest.raises(SystemExit) as exc:
        main(["--version"])
    assert exc.value.code == 0
    assert __version__ in capsys.readouterr().out


def test_help_flag(capsys: pytest.CaptureFixture[str]) -> None:
    with pytest.raises(SystemExit) as exc:
        main(["--help"])
    assert exc.value.code == 0
    assert "tandem" in capsys.readouterr().out


def test_unknown_flag_exits_nonzero() -> None:
    with pytest.raises(SystemExit) as exc:
        main(["--nope"])
    assert exc.value.code != 0


def test_importing_tandem_commands_does_not_load_hashlib() -> None:
    import subprocess
    import sys

    code = "import sys, tandem.cli, tandem.commands, tandem.status.app; sys.exit('hashlib' in sys.modules)"
    assert subprocess.run([sys.executable, "-c", code]).returncode == 0


def test_click_subcommand_in_help() -> None:
    with pytest.raises(SystemExit) as exc:
        main(["--help"])
    assert exc.value.code == 0


def test_click_unknown_action_is_no_op(tmp_path: pytest.TempPathFactory) -> None:
    from tandem.commands import click
    from tandem.config import Config
    from tandem.tmux.client import TmuxClient, ToSViolation
    from tandem.tmux.layout import Layout

    calls: list[tuple[str, ...]] = []

    class FakeClient(TmuxClient):
        def run(self, *args: str) -> object:  # type: ignore[override]
            calls.append(args)
            from tandem.tmux.client import Result
            return Result(0, "", "")

    layout = Layout(FakeClient(), python="/py", workspace=None)
    config = Config.from_env({})
    click(layout, config, "")
    click(layout, config, "BADACTION")
    click(layout, config, "send-keys")
    assert calls == [], "unknown actions must produce no tmux calls"


def test_click_allow_list_matches_click_actions() -> None:
    from tandem.tmux.keys import CLICK_ACTIONS

    allowed = frozenset(CLICK_ACTIONS)
    assert "help" in allowed
    assert "add-agent" in allowed
    assert "quit" in allowed
    assert "send-keys" not in allowed
    assert "capture-pane" not in allowed
    assert "" not in allowed
