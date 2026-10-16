import shlex
from dataclasses import dataclass


@dataclass(frozen=True)
class Binding:
    key: str
    group: str
    description: str
    hint: str
    command: str
    """tmux command; ``{tandem}`` expands to the quoted tandem launcher."""


KEYMAP: tuple[Binding, ...] = (
    Binding("F1", "Help", "open this help", "Help",
            "display-popup -E -w 70% -h 70% {tandem} help"),
    Binding("F2", "Agents", "add an agent", "New agent",
            "display-popup -E -w 90% -h 90% {tandem} add-agent"),
    Binding("F3", "Tabs", "previous agent in this column", "Prev",
            "run-shell -b '{tandem} tab prev'"),
    Binding("F4", "Tabs", "next agent in this column", "Next",
            "run-shell -b '{tandem} tab next'"),
    Binding("F5", "Status", "focus the status window", "Status",
            "run-shell -b '{tandem} focus status'"),
    Binding("F6", "Status", "open the Queries tab", "Queries",
            "run-shell -b '{tandem} focus queries'"),
    Binding("F7", "Columns", "focus planners", "Planners",
            "run-shell -b '{tandem} focus planner'"),
    Binding("F8", "Columns", "focus implementers", "Implementers",
            "run-shell -b '{tandem} focus implementer'"),
    Binding("F9", "Agents", "close the focused agent (asks first)", "Close",
            "confirm-before -p 'Close this agent? (y/n)' \"run-shell -b '{tandem} close-agent'\""),
    Binding("F10", "Quit", "exit menu: save and exit, detach, update tandem, kill", "Quit",
            "run-shell -b '{tandem} quit'"),
    Binding("F11", "Agents", "restart an exited agent", "Restart",
            "run-shell -b '{tandem} restart'"),
    Binding("F12", "Layout", "resize mode: arrows resize the focused pane, = resets, Enter exits", "Resize",
            "switch-client -T tandem-resize"),
)

RESIZE_TABLE = "tandem-resize"
RESIZE_STEP_COLS = 5
RESIZE_STEP_ROWS = 2


def hint_bar() -> str:
    return "  ".join(f"{b.key} {b.hint}" for b in KEYMAP if b.hint)


def resize_hint() -> str:
    return "RESIZE  ←→ width  ↑↓ height  = reset  Enter/Esc done"


def status_left() -> str:
    """Hint bar, swapped for the resize help while the resize key table is active."""
    return f" #{{?#{{==:#{{client_key_table}},{RESIZE_TABLE}}},{resize_hint()},{hint_bar()}}} "


def help_lines() -> list[str]:
    lines = ["tandem keys (planners = left column, implementers = right column)", ""]
    group = ""
    for b in KEYMAP:
        label = b.group if b.group != group else ""
        group = b.group
        lines.append(f"  {label:<9} {b.key:<4} {b.description}")
    lines += ["", "  Mouse     click a pane to focus it; drag a border to resize; scroll wheel scrolls",
              f"  Resize    F12 then arrows (←→ {RESIZE_STEP_COLS} columns, ↑↓ {RESIZE_STEP_ROWS} rows) on the focused pane; = resets; Enter exits",
              "  Detach    your agents keep running; run `tandem` again to re-attach"]
    return lines


def launcher_for(python: str, workspace: str | None = None, module: str = "tandem") -> str:
    """Shell snippet that runs tandem; pins the workspace so panes in other worktrees still agree."""
    base = f"{shlex.quote(python)} -m {module}"
    return f"env TANDEM_WORKSPACE={shlex.quote(workspace)} {base}" if workspace else base


def bind_commands(python: str, workspace: str | None = None) -> list[list[str]]:
    """tmux ``bind-key`` argv lists (root table) for every binding."""
    launcher = launcher_for(python, workspace)
    result: list[list[str]] = []
    for b in KEYMAP:
        result.append(["bind-key", "-n", b.key, *_split(b.command.replace("{tandem}", launcher))])
    return result


def resize_commands(python: str, workspace: str | None = None) -> list[list[str]]:
    """``bind-key`` argv lists for the resize key table; every arrow stays in the table until Enter/Esc."""
    stay = f"switch-client -T {RESIZE_TABLE}"
    bindings = {
        "Left": f"resize-pane -L {RESIZE_STEP_COLS} ; {stay}", "Right": f"resize-pane -R {RESIZE_STEP_COLS} ; {stay}",
        "Up": f"resize-pane -U {RESIZE_STEP_ROWS} ; {stay}", "Down": f"resize-pane -D {RESIZE_STEP_ROWS} ; {stay}",
        "=": f"run-shell -b '{launcher_for(python, workspace)} reset-layout' ; {stay}",
        "Enter": "switch-client -T root", "Escape": "switch-client -T root", "q": "switch-client -T root",
    }
    return [["bind-key", "-T", RESIZE_TABLE, key, command] for key, command in bindings.items()]


def _split(command: str) -> list[str]:
    """tmux accepts a bound command as one string argument."""
    return [command]
