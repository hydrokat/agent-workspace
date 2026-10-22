import os
import sys
from pathlib import Path

import pytest

import tandem
from tandem import updater
from tandem.config import Config


class Recorder:
    def __init__(self, codes: dict[str, int] | None = None) -> None:
        self.calls: list[list[str]] = []
        self.codes = codes or {}

    def __call__(self, argv: list[str], cwd: Path, timeout: float) -> tuple[int, str]:
        self.calls.append(argv)
        return self.codes.get(argv[0], 0), "9.9.9" if argv[-1] == "--version" else ""


def reloader(log: list[int]):  # type: ignore[no-untyped-def]
    def go() -> bool:
        log.append(1)
        return True
    return go


def have(*names: str):  # type: ignore[no-untyped-def]
    return lambda n: f"/bin/{n}" if n in names else None


def installed_workspace(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    ws = tmp_path / "ws"
    (ws / ".agent-workspace" / "tandem").mkdir(parents=True)
    monkeypatch.setattr(updater, "running_from_workspace", lambda w: True)
    return ws


def test_plan_mentions_the_ai_cli_update(tmp_path: Path) -> None:
    text = "\n".join(updater.plan(tmp_path, "o/r"))
    assert "AI CLIs" in text and "claude update" in text


def test_runs_update_sync_then_reload_in_order(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    ws = installed_workspace(tmp_path, monkeypatch)
    rec, quiet = Recorder(), Recorder()
    reloads: list[int] = []
    result = updater.run_update(ws, "o/r", reloader(reloads), "py", rec, quiet, have("npx", "uv"))
    assert [c[0] for c in rec.calls] == ["npx", "uv"]
    assert rec.calls[0] == ["npx", "--yes", "o/r", "update", "--yes", "--no-link"]
    assert rec.calls[1][:2] == ["uv", "sync"] and "--frozen" in rec.calls[1]
    assert result.ok and result.reloaded and reloads == [1] and "9.9.9" in result.message


def test_failed_workspace_update_restarts_nothing(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    ws = installed_workspace(tmp_path, monkeypatch)
    reloads: list[int] = []
    result = updater.run_update(ws, "o/r", reloader(reloads), "py", Recorder({"npx": 1}),
                                Recorder(), have("npx", "uv"))
    assert not result.ok and "Nothing was restarted" in result.message and reloads == []


def test_failed_sync_keeps_old_code_running(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    ws = installed_workspace(tmp_path, monkeypatch)
    reloads: list[int] = []
    result = updater.run_update(ws, "o/r", reloader(reloads), "py", Recorder({"uv": 2}),
                                Recorder(), have("npx", "uv"))
    assert not result.ok and "old code" in result.message and reloads == []


def test_missing_npx_is_explained(tmp_path: Path) -> None:
    result = updater.run_update(tmp_path, "o/r", lambda: True, "py", Recorder(), Recorder(), have())
    assert not result.ok and "Node.js" in result.message


def test_checkout_copy_only_updates_the_workspace(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(updater, "running_from_workspace", lambda w: False)
    rec = Recorder()
    reloads: list[int] = []
    result = updater.run_update(tmp_path, "o/r", reloader(reloads), "py", rec, Recorder(), have("npx"))
    assert result.ok and [c[0] for c in rec.calls] == ["npx"] and reloads == [] and "checkout" in result.message


def test_min_versions_change_is_called_out(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    ws = installed_workspace(tmp_path, monkeypatch)
    min_file = ws / ".agent-workspace" / "tandem" / "min-versions.env"
    min_file.write_text("TANDEM_MIN_TMUX=3.3\n")

    def rewrite(argv: list[str], cwd: Path, timeout: float) -> tuple[int, str]:
        if argv[0] == "npx":
            min_file.write_text("TANDEM_MIN_TMUX=3.4\n")
        return 0, ""

    result = updater.run_update(ws, "o/r", lambda: True, "py", rewrite, Recorder(), have("npx", "uv"))
    assert "Minimum dependency versions changed" in result.message


def test_plan_lists_every_step_and_promises_agents_keep_running(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    installed_workspace(tmp_path, monkeypatch)
    text = "\n".join(updater.plan(tmp_path / "ws", "o/r"))
    assert "npx --yes o/r update" in text and "uv sync" in text and "agents keep running" in text


def test_spec_from_env_is_validated() -> None:
    assert Config.from_env({"TANDEM_UPDATE_SPEC": "me/fork#dev"}).update_spec == "me/fork#dev"
    assert Config.from_env({}).update_spec == "hydrokat/agent-workspace"
    for bad in ("-rf/x", "a b/c", "$(x)/y", "https://evil/x", "owner/repo;rm"):
        with pytest.raises(ValueError):
            Config.from_env({"TANDEM_UPDATE_SPEC": bad})


def test_version_is_importable() -> None:
    assert tandem.__version__


class EnvRecorder(Recorder):
    def __init__(self) -> None:
        super().__init__()
        self.sync_env: str | None = None

    def __call__(self, argv: list[str], cwd: Path, timeout: float) -> tuple[int, str]:
        if argv[:2] == ["uv", "sync"]:
            self.sync_env = os.environ.get("UV_PROJECT_ENVIRONMENT")
        return super().__call__(argv, cwd, timeout)


def test_sync_targets_the_running_virtualenv(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    ws = installed_workspace(tmp_path, monkeypatch)
    monkeypatch.delenv("UV_PROJECT_ENVIRONMENT", raising=False)
    monkeypatch.setattr(sys, "prefix", "/state/tandem/venv-ws")
    monkeypatch.setattr(sys, "base_prefix", "/usr/local")
    rec = EnvRecorder()
    updater.run_update(ws, "o/r", reloader([]), "py", rec, Recorder(), have("npx", "uv"))
    assert rec.sync_env == "/state/tandem/venv-ws"


def test_sync_keeps_an_explicit_environment(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    ws = installed_workspace(tmp_path, monkeypatch)
    monkeypatch.setenv("UV_PROJECT_ENVIRONMENT", "/explicit/venv")
    monkeypatch.setattr(sys, "prefix", "/state/tandem/venv-ws")
    monkeypatch.setattr(sys, "base_prefix", "/usr/local")
    rec = EnvRecorder()
    updater.run_update(ws, "o/r", reloader([]), "py", rec, Recorder(), have("npx", "uv"))
    assert rec.sync_env == "/explicit/venv"


def test_sync_leaves_the_environment_alone_outside_a_virtualenv(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    ws = installed_workspace(tmp_path, monkeypatch)
    monkeypatch.delenv("UV_PROJECT_ENVIRONMENT", raising=False)
    monkeypatch.setattr(sys, "prefix", "/usr/local")
    monkeypatch.setattr(sys, "base_prefix", "/usr/local")
    rec = EnvRecorder()
    updater.run_update(ws, "o/r", reloader([]), "py", rec, Recorder(), have("npx", "uv"))
    assert rec.sync_env is None
