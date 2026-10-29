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


def test_session_id_is_validated() -> None:
    assert Config.from_env({"TANDEM_SESSION_ID": "planner-1"}).session_id == "planner-1"
    assert Config.from_env({"TANDEM_SESSION_ID": "../../x"}).session_id == ""


# ---------------------------------------------------------------------------
# update_interval and update_check (t-007)
# ---------------------------------------------------------------------------


def test_update_interval_default() -> None:
    assert Config.from_env({}).update_interval == 14400


def test_update_interval_parsed_from_env() -> None:
    assert Config.from_env({"TANDEM_UPDATE_INTERVAL": "3600"}).update_interval == 3600


def test_update_interval_minimum_enforced() -> None:
    assert Config.from_env({"TANDEM_UPDATE_INTERVAL": "10"}).update_interval == 600


def test_update_interval_invalid_uses_default() -> None:
    assert Config.from_env({"TANDEM_UPDATE_INTERVAL": "not-a-number"}).update_interval == 14400


def test_update_check_off_when_no_workspace() -> None:
    assert Config.from_env({}).update_check is False


def test_update_check_disabled_by_env() -> None:
    assert Config.from_env({"TANDEM_UPDATE_CHECK": "0"}).update_check is False


def test_update_check_disabled_when_no_manifest(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    ws.mkdir()
    cfg = Config.from_env({"TANDEM_WORKSPACE": str(ws)})
    assert cfg.update_check is False


def test_update_check_enabled_when_manifest_exists(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    (ws / ".agent-workspace").mkdir(parents=True)
    (ws / ".agent-workspace" / "manifest.json").write_text('{"version": "1.3.0"}')
    cfg = Config.from_env({"TANDEM_WORKSPACE": str(ws)})
    assert cfg.update_check is True


def test_update_check_env_zero_wins_over_manifest(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    (ws / ".agent-workspace").mkdir(parents=True)
    (ws / ".agent-workspace" / "manifest.json").write_text('{"version": "1.3.0"}')
    cfg = Config.from_env({"TANDEM_WORKSPACE": str(ws), "TANDEM_UPDATE_CHECK": "0"})
    assert cfg.update_check is False
