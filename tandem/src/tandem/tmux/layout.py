import json
import sys
from dataclasses import dataclass, field
from pathlib import Path

from tandem.coordination.store import CoordStore
from tandem.tmux import keys
from tandem.tmux.client import TmuxClient, TmuxError
from tandem.tmux.tabs import FORMAT, PaneInfo, agents_in, neighbor, parse_panes, render_title, visible_pane

SESSION = "tandem"
STATUS_ROWS = 10
STACK_BELOW_COLS = 100
ROLES = ("planner", "implementer")


@dataclass(frozen=True)
class Launch:
    """One pane to create. ``argv`` is exec'd directly by tmux (no shell)."""

    argv: list[str]
    role: str
    index: int
    label: str
    kind: str = "agent"
    env: dict[str, str] = field(default_factory=dict)
    cwd: str | None = None
    session_id: str = ""


def conf_text(history: int = 50000) -> str:
    """tandem-owned tmux config; the user's ~/.tmux.conf is never loaded."""
    return "\n".join([
        "set-option -g mouse on",
        "set-option -g prefix None",
        "set-option -g prefix2 None",
        "set-option -g escape-time 10",
        f"set-option -g history-limit {history}",
        "set-option -g pane-border-status top",
        "set-option -g status on",
        "set-option -g status-position bottom",
        "set-window-option -g remain-on-exit on",
        "",
    ])


def _spawn_args(launch: Launch) -> list[str]:
    args = ["-P", "-F", "#{pane_id}"]
    if launch.cwd:
        args += ["-c", launch.cwd]
    for key, value in launch.env.items():
        args += ["-e", f"{key}={value}"]
    return args + launch.argv


class Layout:
    """Builds and mutates the tandem tmux session: two columns, a status pane, hidden stash windows."""

    def __init__(self, client: TmuxClient, python: str = sys.executable, coord: CoordStore | None = None,
                 workspace: str | None = None) -> None:
        self.client = client
        self.python = python
        self.coord = coord
        self.workspace = workspace

    def exists(self) -> bool:
        return self.client.run("has-session", "-t", SESSION).code == 0

    def panes(self) -> list[PaneInfo]:
        return parse_panes(self.client.run_checked("list-panes", "-a", "-s", "-t", SESSION, "-F", FORMAT))

    def _tag(self, pane_id: str, launch: Launch) -> None:
        for name, value in (("kind", launch.kind), ("role", launch.role), ("index", str(launch.index)),
                            ("label", launch.label), ("session", launch.session_id),
                            ("argv", json.dumps(launch.argv))):
            self.client.run_checked("set-option", "-p", "-t", pane_id, f"@tandem_{name}", value)

    def build(self, width: int, height: int, left: Launch, right: Launch, status: Launch) -> dict[str, str]:
        c = self.client
        a = c.run_checked("new-session", "-d", "-s", SESSION, "-n", "main", "-x", str(width), "-y", str(height),
                          *_spawn_args(left)).strip()
        b = c.run_checked("split-window", "-h", "-t", a, *_spawn_args(right)).strip()
        s = c.run_checked("split-window", "-v", "-f", "-l", str(STATUS_ROWS), "-t", f"{SESSION}:main",
                          *_spawn_args(status)).strip()
        for pane_id, launch in ((a, left), (b, right), (s, status)):
            self._tag(pane_id, launch)
            self._register(launch, pane_id)
        self._apply_chrome()
        self.refresh()
        return {"left": a, "right": b, "status": s}

    def _apply_chrome(self) -> None:
        c = self.client
        c.run_checked("set-option", "-g", "status-left-length", "400")
        c.run_checked("set-option", "-g", "status-left", f" {keys.hint_bar()} ")
        c.run_checked("set-option", "-g", "status-right", "")
        c.run_checked("set-option", "-g", "window-status-format", "")
        c.run_checked("set-option", "-g", "window-status-current-format", "")
        for argv in keys.bind_commands(self.python, self.workspace):
            c.run_checked(*argv)
        launcher = keys.launcher_for(self.python, self.workspace)
        c.run_checked("set-hook", "-g", "client-resized", f"run-shell -b '{launcher} reflow'")
        c.run_checked("set-hook", "-g", "client-attached", f"run-shell -b '{launcher} first-run'")
        c.run_checked("set-hook", "-g", "pane-died", f"run-shell -b '{launcher} refresh'")

    def _register(self, launch: Launch, pane_id: str) -> None:
        if self.coord is not None and launch.kind == "agent" and launch.session_id:
            self.coord.write_session(launch.session_id, launch.argv[0], launch.role, pane_id,
                                     launch.cwd or "", launch.label)

    def refresh(self) -> None:
        panes = self.panes()
        for role in ROLES:
            shown = visible_pane(panes, role)
            if shown is not None:
                self.client.run_checked("set-option", "-p", "-t", shown.id, "pane-border-format",
                                        render_title(panes, role))

    def add_agent(self, launch: Launch, activate: bool = True) -> str:
        """Spawn in a hidden window, then swap it into its column (replacing a placeholder)."""
        c = self.client
        new = c.run_checked("new-window", "-d", "-t", SESSION, "-n", f"stash-{launch.role}-{launch.index}",
                            *_spawn_args(launch)).strip()
        self._tag(new, launch)
        self._register(launch, new)
        panes = self.panes()
        shown = visible_pane(panes, launch.role)
        if shown is not None and (activate or shown.kind != "agent"):
            c.run_checked("swap-pane", "-d", "-s", new, "-t", shown.id)
            if shown.kind != "agent":
                c.run_checked("kill-pane", "-t", shown.id)
        self.refresh()
        return new

    def select(self, role: str, index: int) -> None:
        panes = self.panes()
        target = next((p for p in agents_in(panes, role) if p.index == index), None)
        self._show(panes, role, target)

    def step(self, role: str, delta: int) -> None:
        panes = self.panes()
        self._show(panes, role, neighbor(panes, role, delta))

    def _show(self, panes: list[PaneInfo], role: str, target: PaneInfo | None) -> None:
        shown = visible_pane(panes, role)
        if target is None or shown is None or target.id == shown.id:
            return
        self.client.run_checked("swap-pane", "-d", "-s", target.id, "-t", shown.id)
        self.client.run_checked("select-pane", "-t", target.id)
        self.refresh()

    def close_agent(self, pane_id: str, placeholder: Launch) -> None:
        panes = self.panes()
        pane = next((p for p in panes if p.id == pane_id and p.kind == "agent"), None)
        if pane is None:
            return
        others = [p for p in agents_in(panes, pane.role) if p.id != pane.id]
        if pane.window == "main" and not others:
            self.client.run_checked("respawn-pane", "-k", "-t", pane.id, *placeholder.argv)
            self._tag(pane.id, placeholder)
        else:
            if pane.window == "main":
                self.client.run_checked("swap-pane", "-d", "-s", others[0].id, "-t", pane.id)
            self.client.run_checked("kill-pane", "-t", pane.id)
        if self.coord is not None and pane.session:
            self.coord.remove_session(pane.session)
        self.refresh()

    def restart(self, pane_id: str, coord_dir: str) -> None:
        out = self.client.run_checked("display-message", "-p", "-t", pane_id,
                                      "#{pane_dead}\t#{@tandem_argv}\t#{@tandem_session}\t#{@tandem_role}\t#{@tandem_cli}").rstrip("\n")
        dead, raw, session, role, _ = (out.split("\t") + [""] * 5)[:5]
        if dead != "1":
            return
        argv = json.loads(raw)
        if not isinstance(argv, list) or not all(isinstance(a, str) for a in argv):
            raise TmuxError("pane has no restartable command")
        env = ["-e", f"TANDEM_SESSION_ID={session}", "-e", f"TANDEM_ROLE={role}",
               "-e", f"TANDEM_CLI={argv[0]}", "-e", f"TANDEM_COORD_DIR={coord_dir}"]
        self.client.run_checked("respawn-pane", "-k", "-t", pane_id, *env, *argv)
        self.refresh()

    def focus(self, role: str) -> None:
        panes = self.panes()
        target = visible_pane(panes, role)
        if target is not None:
            self.client.run_checked("select-pane", "-t", target.id)

    def reflow(self, width: int) -> None:
        panes = self.panes()
        left, right = visible_pane(panes, "planner"), visible_pane(panes, "implementer")
        if left is None or right is None:
            return
        out = self.client.run_checked("list-panes", "-t", f"{SESSION}:main", "-F", "#{pane_id} #{pane_left}")
        lefts = dict(line.split(" ", 1) for line in out.splitlines() if " " in line)
        stacked = lefts.get(left.id) == lefts.get(right.id)
        if width < STACK_BELOW_COLS and not stacked:
            self.client.run_checked("move-pane", "-v", "-s", right.id, "-t", left.id)
        elif width >= STACK_BELOW_COLS and stacked:
            self.client.run_checked("move-pane", "-h", "-s", right.id, "-t", left.id)

    def write_conf(self, path: Path) -> Path:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(conf_text())
        return path
