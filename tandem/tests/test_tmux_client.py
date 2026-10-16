from pathlib import Path

import pytest

from tandem.tmux.client import ToSViolation, TmuxClient, TmuxError, Result


class Fake:
    def __init__(self, responses: dict[str, Result] | None = None) -> None:
        self.calls: list[list[str]] = []
        self.responses = responses or {}

    def __call__(self, argv: list[str]) -> Result:
        self.calls.append(argv)
        for key, value in self.responses.items():
            if key in argv:
                return value
        return Result(0, "", "")


def client(fake: Fake) -> TmuxClient:
    return TmuxClient(socket="tandem-test", conf=Path("/c/tandem.conf"), runner=fake)


def test_prefixes_dedicated_socket_and_config() -> None:
    fake = Fake()
    client(fake).run("has-session", "-t", "tandem")
    assert fake.calls == [["tmux", "-L", "tandem-test", "-f", "/c/tandem.conf", "has-session", "-t", "tandem"]]


@pytest.mark.parametrize("sub", ["send-keys", "capture-pane", "pipe-pane", "run-shell", "send-prefix",
                                 "save-buffer", "show-buffer", "paste-buffer", "load-buffer", "copy-mode",
                                 "if-shell", "wait-for", "totally-unknown"])
def test_forbidden_or_unknown_subcommands_rejected(sub: str) -> None:
    fake = Fake()
    with pytest.raises(ToSViolation):
        client(fake).run(sub, "-t", "%1")
    assert fake.calls == []


@pytest.mark.parametrize("sub", ["new-session", "new-window", "split-window", "swap-pane", "select-pane",
                                 "resize-pane", "kill-pane", "set-option", "bind-key", "display-popup",
                                 "list-panes", "move-pane", "select-layout", "kill-session"])
def test_spawn_move_focus_commands_allowed(sub: str) -> None:
    fake = Fake()
    client(fake).run(sub, "-t", "%1")
    assert len(fake.calls) == 1


def test_run_checked_raises_on_failure() -> None:
    fake = Fake({"kill-pane": Result(1, "", "can't find pane")})
    with pytest.raises(TmuxError):
        client(fake).run_checked("kill-pane", "-t", "%9")


def test_send_keys_own_requires_own_kind() -> None:
    fake = Fake({"display-message": Result(0, "agent\n", "")})
    with pytest.raises(ToSViolation):
        client(fake).send_keys_own("%1", "Enter")
    assert not any("send-keys" in c for c in fake.calls)


def test_send_keys_own_fails_closed_on_unknown_kind() -> None:
    fake = Fake({"display-message": Result(0, "\n", "")})
    with pytest.raises(ToSViolation):
        client(fake).send_keys_own("%1", "Enter")


def test_send_keys_own_allowed_for_own_panes() -> None:
    fake = Fake({"display-message": Result(0, "own\n", "")})
    client(fake).send_keys_own("%1", "Enter")
    assert any(c[5:7] == ["send-keys", "-t"] or "send-keys" in c for c in fake.calls)


def test_capture_own_refuses_agent_panes() -> None:
    fake = Fake({"display-message": Result(0, "agent\n", "")})
    with pytest.raises(ToSViolation):
        client(fake).capture_own("%1")


def test_capture_own_reads_own_panes() -> None:
    class Cap(Fake):
        def __call__(self, argv: list[str]) -> Result:
            self.calls.append(argv)
            if "display-message" in argv:
                return Result(0, "own\n", "")
            return Result(0, "hello", "")

    assert client(Cap()).capture_own("%2") == "hello"


def test_pane_id_must_look_like_a_pane_id() -> None:
    fake = Fake({"display-message": Result(0, "own\n", "")})
    with pytest.raises(ValueError):
        client(fake).send_keys_own("%1; kill-server", "Enter")


def test_each_workspace_gets_its_own_tmux_socket(tmp_path: Path) -> None:
    from tandem.tmux.client import socket_for

    a, b = tmp_path / "a", tmp_path / "b"
    a.mkdir()
    b.mkdir()
    assert socket_for(a) != socket_for(b)
    assert socket_for(a) == socket_for(a) == socket_for(tmp_path / "a" / ".." / "a")
    assert socket_for(a).startswith("tandem-") and "/" not in socket_for(a)
