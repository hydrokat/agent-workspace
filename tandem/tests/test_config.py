from pathlib import Path

import pytest

from tandem.config import Config


def test_defaults_when_env_empty() -> None:
    cfg = Config.from_env({})
    assert cfg.coord_dir is None
    assert cfg.color is True


def test_reads_coord_dir() -> None:
    cfg = Config.from_env({"TANDEM_COORD_DIR": "/tmp/coord"})
    assert cfg.coord_dir == Path("/tmp/coord")


def test_no_color_disables_color() -> None:
    assert Config.from_env({"NO_COLOR": "1"}).color is False


def test_empty_no_color_is_ignored() -> None:
    assert Config.from_env({"NO_COLOR": ""}).color is True


def test_relative_coord_dir_rejected() -> None:
    with pytest.raises(ValueError):
        Config.from_env({"TANDEM_COORD_DIR": "relative/path"})


def test_does_not_leak_unrelated_env() -> None:
    cfg = Config.from_env({"ANTHROPIC_API_KEY": "sk-secret"})
    assert "sk-secret" not in repr(cfg)


def test_tmux_pane_and_state_dir() -> None:
    cfg = Config.from_env({"TMUX_PANE": "%4", "XDG_STATE_HOME": "/s"})
    assert cfg.tmux_pane == "%4" and cfg.state_dir == Path("/s/tandem")


def test_relative_xdg_state_home_ignored() -> None:
    assert Config.from_env({"XDG_STATE_HOME": "rel"}).state_dir.is_absolute()


def test_workspace_override_must_be_absolute() -> None:
    assert Config.from_env({"TANDEM_WORKSPACE": "/w"}).workspace == Path("/w")
    assert Config.from_env({"TANDEM_WORKSPACE": "w"}).workspace is None
