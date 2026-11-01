import json
import os
import shlex
import stat
import sys
from pathlib import Path

import pytest
from tandem import selfexec
from tandem.adapters.base import AgentSpec
from tandem.adapters.claude import ClaudeAdapter
from tandem.tmux import keys


def test_is_frozen_false_by_default(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delattr(sys, "frozen", raising=False)
    assert not selfexec.is_frozen()


def test_is_frozen_true_when_sys_frozen_set(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(sys, "frozen", True, raising=False)
    assert selfexec.is_frozen()


def test_self_argv_non_frozen(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    monkeypatch.delattr(sys, "frozen", raising=False)
    fake_exe = str(tmp_path / "fake_py")
    monkeypatch.setattr(sys, "executable", fake_exe)

    fake_self = tmp_path / "tandem_bin"
    fake_self.write_text("binary")
    fake_self.chmod(fake_self.stat().st_mode | stat.S_IXUSR)
    monkeypatch.setenv("TANDEM_SELF", str(fake_self))

    assert selfexec.self_argv() == [fake_exe, "-m", "tandem"]
    assert selfexec.self_command() == f"{shlex.quote(fake_exe)} -m tandem"
    assert selfexec.self_argv(python="/custom/py") == ["/custom/py", "-m", "tandem"]


def test_self_argv_frozen_with_valid_tandem_self(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    monkeypatch.setattr(sys, "frozen", True, raising=False)
    fake_exe = str(tmp_path / "pyinstaller_exe")
    monkeypatch.setattr(sys, "executable", fake_exe)

    fake_self = tmp_path / "tandem_current"
    fake_self.write_text("binary")
    fake_self.chmod(fake_self.stat().st_mode | stat.S_IXUSR)
    monkeypatch.setenv("TANDEM_SELF", str(fake_self))

    assert selfexec.self_argv() == [str(fake_self)]
    assert selfexec.self_command() == shlex.quote(str(fake_self))


def test_self_argv_frozen_fallback_when_tandem_self_invalid(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(sys, "frozen", True, raising=False)
    fake_exe = str(tmp_path / "pyinstaller_exe")
    monkeypatch.setattr(sys, "executable", fake_exe)

    monkeypatch.delenv("TANDEM_SELF", raising=False)
    assert selfexec.self_argv() == [fake_exe]

    monkeypatch.setenv("TANDEM_SELF", str(tmp_path / "does_not_exist"))
    assert selfexec.self_argv() == [fake_exe]

    not_exec = tmp_path / "not_exec"
    not_exec.write_text("binary")
    not_exec.chmod(stat.S_IRUSR | stat.S_IWUSR)
    monkeypatch.setenv("TANDEM_SELF", str(not_exec))
    assert selfexec.self_argv() == [fake_exe]


def test_self_command_with_custom_argv() -> None:
    custom = ["/custom/bin", "arg with space"]
    assert selfexec.self_command(custom) == "/custom/bin 'arg with space'"
    assert selfexec.self_command("/already/quoted 'str'") == "/already/quoted 'str'"


def test_frozen_mode_bindings_and_statusline(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    fake_bin = tmp_path / "tandem_dist"
    fake_bin.write_text("binary")
    fake_bin.chmod(fake_bin.stat().st_mode | stat.S_IXUSR)

    monkeypatch.setattr(sys, "frozen", True, raising=False)
    monkeypatch.setattr(sys, "executable", str(fake_bin))
    monkeypatch.delenv("TANDEM_SELF", raising=False)

    launcher = keys.launcher_for(workspace=str(tmp_path))
    assert "-m tandem" not in launcher
    assert launcher == f"env TANDEM_WORKSPACE={shlex.quote(str(tmp_path))} {shlex.quote(str(fake_bin))}"

    binds = keys.bind_commands(workspace=str(tmp_path), platform="linux")
    assert all("-m tandem" not in " ".join(b) for b in binds)
    assert any(str(fake_bin) in " ".join(b) for b in binds)

    adapter = ClaudeAdapter()
    spec = AgentSpec(cli="claude", role="planner", model="opus", index=1)
    argv = adapter.build_argv(spec)
    settings = json.loads(argv[argv.index("--settings") + 1])
    cmd = settings["statusLine"]["command"]
    assert "-m tandem" not in cmd
    assert cmd == f"{shlex.quote(str(fake_bin))} quota-feed"
