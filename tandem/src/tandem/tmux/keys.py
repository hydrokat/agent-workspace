import os
import re
import shlex
from collections.abc import Mapping
from dataclasses import dataclass, replace

MODIFIERS: tuple[str, ...] = ("Ctrl", "Alt", "Shift")

_MODIFIER_MAP: dict[str, str] = {
    "ctrl": "Ctrl",
    "control": "Ctrl",
    "c": "Ctrl",
    "alt": "Alt",
    "meta": "Alt",
    "m": "Alt",
    "shift": "Shift",
    "s": "Shift",
}

_TMUX_MOD_MAP: dict[str, str] = {
    "Ctrl": "C-",
    "Alt": "M-",
    "Shift": "S-",
}

_TEXTUAL_MOD_MAP: dict[str, str] = {
    "Ctrl": "ctrl",
    "Alt": "alt",
    "Shift": "shift",
}

_NAMED_KEYS: dict[str, str] = {
    "up": "Up",
    "down": "Down",
    "left": "Left",
    "right": "Right",
    "enter": "Enter",
    "escape": "Escape",
    "esc": "Escape",
    "tab": "Tab",
    "space": "Space",
    "backspace": "Backspace",
}


def _canonical_base_key(base: str) -> str:
    cleaned = base.strip()
    if not cleaned:
        raise ValueError("Shortcut base key cannot be empty")
    low = cleaned.lower()
    if low in _NAMED_KEYS:
        return _NAMED_KEYS[low]
    if low.startswith("f") and low[1:].isdigit():
        return f"F{low[1:]}"
    if len(cleaned) == 1 and cleaned.isalpha():
        return cleaned.lower()
    return cleaned


def parse_shortcut(raw: str) -> tuple[tuple[str, ...], str]:
    trimmed = raw.strip()
    if not trimmed:
        raise ValueError("Shortcut cannot be empty")

    special_base: str | None = None
    work = trimmed
    if work.endswith("++"):
        special_base = "+"
        work = work[:-2]
    elif work.endswith("+-") or work.endswith("--"):
        special_base = "-"
        work = work[:-2]

    tokens: list[str] = []
    pattern = re.compile(r"^([A-Za-z0-9_]+)[\+\-]")
    while True:
        m = pattern.match(work)
        if not m:
            break
        tokens.append(m.group(1))
        work = work[m.end():]
        if not work:
            break

    if special_base is not None:
        base_candidate = special_base
        if work:
            tokens.append(work)
    else:
        base_candidate = work

    if not tokens:
        raise ValueError(f"Shortcut must have at least one modifier key (Ctrl, Alt, Shift): '{raw}'")

    if not base_candidate or not base_candidate.strip():
        raise ValueError(f"Shortcut has no base key: '{raw}'")

    found_mods: set[str] = set()
    for token in tokens:
        tok_clean = token.strip()
        if not tok_clean:
            raise ValueError(f"Invalid empty modifier token in shortcut: '{raw}'")
        low = tok_clean.lower()
        if low not in _MODIFIER_MAP:
            raise ValueError(f"Unknown modifier '{token}' in shortcut '{raw}'")
        mod = _MODIFIER_MAP[low]
        if mod in found_mods:
            raise ValueError(f"Duplicate modifier '{token}' in shortcut '{raw}'")
        found_mods.add(mod)

    canonical_mods = tuple(m for m in MODIFIERS if m in found_mods)
    canonical_base = _canonical_base_key(base_candidate)
    return canonical_mods, canonical_base


def format_shortcut(raw: str) -> str:
    mods, base = parse_shortcut(raw)
    return "+".join((*mods, base))


def tmux_key(raw: str) -> str:
    mods, base = parse_shortcut(raw)
    return "".join(_TMUX_MOD_MAP[m] for m in mods) + base


def textual_key(raw: str) -> str:
    mods, base = parse_shortcut(raw)
    return "+".join([*(_TEXTUAL_MOD_MAP[m] for m in mods), base.lower()])


@dataclass(frozen=True)
class Binding:
    key: str
    group: str
    description: str
    hint: str
    command: str
    """tmux command; ``{tandem}`` expands to the quoted tandem launcher."""

    def __post_init__(self) -> None:
        canonical = format_shortcut(self.key)
        object.__setattr__(self, "key", canonical)

    @property
    def modifiers(self) -> tuple[str, ...]:
        mods, _ = parse_shortcut(self.key)
        return mods

    @property
    def base_key(self) -> str:
        _, base = parse_shortcut(self.key)
        return base

    @property
    def tmux_key(self) -> str:
        return tmux_key(self.key)

    @property
    def textual_key(self) -> str:
        return textual_key(self.key)


DEFAULT_KEYMAP: tuple[Binding, ...] = (
    Binding("Alt+h", "Help", "open this help", "Help",
            "display-popup -E -w 70% -h 70% {tandem} help"),
    Binding("Ctrl+Alt+n", "Agents", "add an agent", "New agent",
            "display-popup -E -w 90% -h 90% {tandem} add-agent"),
    Binding("Alt+Up", "Tabs", "previous agent in this column", "Prev",
            "run-shell -b '{tandem} tab prev'"),
    Binding("Alt+Down", "Tabs", "next agent in this column", "Next",
            "run-shell -b '{tandem} tab next'"),
    Binding("Alt+s", "Status", "focus the status window", "Status",
            "run-shell -b '{tandem} focus status'"),
    Binding("Alt+q", "Status", "open the Queries tab", "Queries",
            "run-shell -b '{tandem} focus queries'"),
    Binding("Alt+1", "Columns", "focus planners", "Planners",
            "run-shell -b '{tandem} focus planner'"),
    Binding("Alt+2", "Columns", "focus implementers", "Implementers",
            "run-shell -b '{tandem} focus implementer'"),
    Binding("Ctrl+Alt+w", "Agents", "close the focused agent (asks first)", "Close",
            "confirm-before -p 'Close this agent? (y/n)' \"run-shell -b '{tandem} close-agent'\""),
    Binding("Ctrl+Alt+x", "Quit", "exit menu: save and exit, detach, update tandem, discard", "Quit",
            "run-shell -b '{tandem} quit'"),
    Binding("Ctrl+Alt+r", "Agents", "restart an exited agent", "Restart",
            "run-shell -b '{tandem} restart'"),
    Binding("Ctrl+Alt+z", "Layout", "resize mode: arrows resize the focused pane, = resets, Enter exits", "Resize",
            "switch-client -T tandem-resize"),
)

ACTION_MAP: dict[str, int] = {
    "help": 0,
    "add-agent": 1,
    "new-agent": 1,
    "tab-prev": 2,
    "prev": 2,
    "tab-next": 3,
    "next": 3,
    "status": 4,
    "focus-status": 4,
    "queries": 5,
    "focus-queries": 5,
    "planner": 6,
    "focus-planner": 6,
    "planners": 6,
    "implementer": 7,
    "focus-implementer": 7,
    "implementers": 7,
    "close": 8,
    "close-agent": 8,
    "quit": 9,
    "restart": 10,
    "resize": 11,
}


def keymap(env: Mapping[str, str] | None = None) -> tuple[Binding, ...]:
    source = os.environ if env is None else env
    raw = source.get("TANDEM_KEYS", "").strip()
    if not raw:
        return DEFAULT_KEYMAP

    bindings = list(DEFAULT_KEYMAP)
    for part in raw.split(","):
        item = part.strip()
        if not item:
            continue
        delim = "=" if "=" in item else (":" if ":" in item else "")
        if not delim:
            continue
        action_name, shortcut = item.split(delim, 1)
        action_clean = action_name.strip().lower()
        if action_clean in ACTION_MAP:
            idx = ACTION_MAP[action_clean]
            bindings[idx] = replace(bindings[idx], key=shortcut.strip())
    return tuple(bindings)


KEYMAP: tuple[Binding, ...] = keymap()


def key_for(action: str, km: tuple[Binding, ...] = KEYMAP) -> str:
    clean_action = action.strip().lower()
    if clean_action in ACTION_MAP:
        return km[ACTION_MAP[clean_action]].key
    return ""


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
        lines.append(f"  {label:<9} {b.key:<12} {b.description}")
    resize_k = key_for("resize") or "Ctrl+Alt+z"
    lines += ["", "  Mouse     click a pane to focus it; drag a border to resize; scroll wheel scrolls",
              f"  Resize    {resize_k} then arrows (←→ {RESIZE_STEP_COLS} columns, ↑↓ {RESIZE_STEP_ROWS} rows) on the focused pane; = resets; Enter exits",
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
        result.append(["bind-key", "-n", b.tmux_key, *_split(b.command.replace("{tandem}", launcher))])
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
