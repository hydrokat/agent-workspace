import hashlib
import re
import subprocess
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

_PANE_ID = re.compile(r"^%\d+$")

# Default-deny allow-list. Anything that can type into or read from a pane (send-keys, capture-pane,
# pipe-pane, buffers, copy-mode, run-shell) is deliberately absent; the two own-pane helpers below are
# the only way to reach send-keys / capture-pane, and they refuse AI panes.
ALLOWED = frozenset({
    "new-session", "new-window", "split-window", "swap-pane", "select-pane", "resize-pane", "kill-pane",
    "move-pane", "join-pane", "break-pane", "respawn-pane", "select-layout", "select-window",
    "set-option", "set-window-option", "set-hook", "bind-key", "unbind-key", "display-message",
    "display-popup", "display-menu", "confirm-before", "list-panes", "list-windows", "has-session",
    "kill-session", "attach-session", "detach-client", "switch-client", "source-file", "show-options",
})
_OWN_ONLY = {"send-keys", "capture-pane"}


def socket_for(workspace: Path) -> str:
    """One tmux server (socket) per workspace, so projects run side by side without sharing a session."""
    digest = hashlib.sha256(str(workspace.resolve()).encode()).hexdigest()[:10]
    return f"tandem-{digest}"


class ToSViolation(Exception):
    """An operation would type into or read from an AI CLI pane, or is not on the allow-list."""


class TmuxError(RuntimeError):
    """tmux exited non-zero."""


@dataclass(frozen=True)
class Result:
    code: int
    out: str
    err: str


Runner = Callable[[list[str]], Result]


def subprocess_runner(argv: list[str]) -> Result:
    done = subprocess.run(argv, capture_output=True, text=True, stdin=subprocess.DEVNULL)
    return Result(done.returncode, done.stdout, done.stderr)


class TmuxClient:
    """Thin wrapper over the tmux CLI on a dedicated socket, with a default-deny command allow-list."""

    def __init__(self, socket: str = "tandem", conf: Path | None = None, runner: Runner = subprocess_runner) -> None:
        self.socket = socket
        self.conf = conf
        self._runner = runner

    def _argv(self, args: tuple[str, ...]) -> list[str]:
        argv = ["tmux", "-L", self.socket]
        if self.conf is not None:
            argv += ["-f", str(self.conf)]
        return argv + list(args)

    def run(self, *args: str) -> Result:
        if not args or args[0] not in ALLOWED:
            raise ToSViolation(f"tmux subcommand not allowed: {args[0] if args else '(none)'}")
        return self._runner(self._argv(args))

    def run_checked(self, *args: str) -> str:
        result = self.run(*args)
        if result.code != 0:
            raise TmuxError(f"tmux {args[0]} failed: {result.err.strip()}")
        return result.out

    def pane_kind(self, pane_id: str) -> str:
        if not _PANE_ID.match(pane_id):
            raise ValueError("invalid pane id")
        return self.run("display-message", "-p", "-t", pane_id, "#{@tandem_kind}").out.strip()

    def _require_own(self, pane_id: str) -> None:
        if self.pane_kind(pane_id) != "own":
            raise ToSViolation("refusing to touch a pane that is not tandem-owned")

    def send_keys_own(self, pane_id: str, *keys: str) -> None:
        self._require_own(pane_id)
        self._runner(self._argv(("send-keys", "-t", pane_id, *keys)))

    def capture_own(self, pane_id: str) -> str:
        self._require_own(pane_id)
        return self._runner(self._argv(("capture-pane", "-p", "-t", pane_id))).out
