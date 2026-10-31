import re
from dataclasses import dataclass

from tandem.tmux import keys

FORMAT = ("#{pane_id}\t#{window_name}\t#{@tandem_kind}\t#{@tandem_role}\t#{@tandem_index}\t"
          "#{@tandem_label}\t#{pane_dead}\t#{pane_dead_status}\t#{@tandem_session}\t#{@tandem_spec}\t#{@tandem_cli_version}")
_UNSAFE = re.compile(r"[^A-Za-z0-9._:·\- ]")
_COLUMN_NAMES = {"planner": "PLANNERS", "implementer": "IMPLEMENTERS"}


@dataclass(frozen=True)
class PaneInfo:
    id: str
    window: str
    kind: str
    role: str
    index: int
    label: str
    dead: bool
    dead_status: int | None
    session: str = ""
    spec: str = ""
    cli_version: str = ""


def parse_panes(text: str) -> list[PaneInfo]:
    """Parse ``list-panes -F FORMAT`` output; lines for panes tandem did not tag are skipped."""
    panes: list[PaneInfo] = []
    for line in text.splitlines():
        parts = line.split("\t")
        if len(parts) not in (9, 10, 11) or not parts[2] or not parts[3]:
            continue
        pid, window, kind, role, index, label, dead, status, session = parts[:9]
        spec = parts[9] if len(parts) > 9 else ""
        cli_version = parts[10] if len(parts) == 11 else ""
        panes.append(PaneInfo(pid, window, kind, role, int(index) if index.isdigit() else 0, label,
                              dead == "1", int(status) if status.lstrip("-").isdigit() else None, session, spec, cli_version))
    return panes


def visible_pane(panes: list[PaneInfo], role: str) -> PaneInfo | None:
    return next((p for p in panes if p.role == role and p.window == "main"), None)


def agents_in(panes: list[PaneInfo], role: str) -> list[PaneInfo]:
    return sorted((p for p in panes if p.role == role and p.kind == "agent"), key=lambda p: p.index)


def neighbor(panes: list[PaneInfo], role: str, step: int) -> PaneInfo | None:
    """The agent ``step`` tabs away from the visible one in ``role``'s column (wrapping)."""
    agents = agents_in(panes, role)
    if not agents:
        return None
    current = visible_pane(panes, role)
    position = next((i for i, p in enumerate(agents) if current and p.id == current.id), 0)
    return agents[(position + step) % len(agents)]


def _safe(text: str) -> str:
    return _UNSAFE.sub("?", text)


def tab_text(pane: PaneInfo) -> str:
    """Label of one agent as shown in its column title: index, label, and the exit status if it exited."""
    text = f"{pane.index} {_safe(pane.label)}"
    if pane.dead:
        text = f"⚠{text}(exited {pane.dead_status if pane.dead_status is not None else '?'})"
    return text


def render_title(panes: list[PaneInfo], role: str) -> str:
    """tmux ``pane-border-format`` text for a column; the focus marker is a tmux conditional."""
    name = _COLUMN_NAMES.get(role, role.upper())
    agents = agents_in(panes, role)
    head = f"─ #{{?pane_active,▶ ,}}{name} ─"
    if not agents:
        return f"{head} No {role}s yet. Press {keys.display_shortcut(keys.key_for('add-agent'))} to add one"
    current = visible_pane(panes, role)
    tabs: list[str] = []
    for p in agents:
        text = tab_text(p)
        tabs.append(f"[{text}]" if current and p.id == current.id else text)
    return f"{head} {'  '.join(tabs)}"
