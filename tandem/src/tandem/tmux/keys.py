import os
import re
import shlex
import sys
from collections.abc import Mapping
from dataclasses import dataclass, replace

MODIFIERS: tuple[str, ...] = ("Ctrl", "Alt", "Shift")

_MODIFIER_MAP: dict[str, str] = {
    "ctrl": "Ctrl",
    "control": "Ctrl",
    "c": "Ctrl",
    "alt": "Alt",
    "opt": "Alt",
    "option": "Alt",
    "⌥": "Alt",
    "⌃": "Ctrl",
    "⇧": "Shift",
    "meta": "Alt",
    "m": "Alt",
    "shift": "Shift",
    "s": "Shift",
}

MACOS_OPTION_CHARS: dict[str, str] = {
    "h": "˙", "s": "ß", "q": "œ", "1": "¡", "2": "™", "g": "©", "r": "®",
    "w": "∑", "f": "ƒ", "a": "å", "d": "∂", "x": "≈", "z": "Ω",
}
"""What a macOS terminal sends for Option+<key> when "Use Option as Meta" is off."""

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
    pattern = re.compile(r"^([A-Za-z0-9_⌥⌃⇧]+)[\+\-]")
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


def alt_name(platform: str = sys.platform) -> str:
    return "Opt" if platform == "darwin" else "Alt"


def display_shortcut(raw: str, platform: str = sys.platform) -> str:
    mods, base = parse_shortcut(raw)
    return "+".join((*(alt_name(platform) if m == "Alt" else m for m in mods), base))


def option_char(raw: str, platform: str = sys.platform) -> str | None:
    """The character macOS types for a lone Option+<key> shortcut, or None where it does not apply."""
    mods, base = parse_shortcut(raw)
    if platform == "darwin" and mods == ("Alt",):
        return MACOS_OPTION_CHARS.get(base.lower())
    return None


def textual_binding_keys(raw: str, platform: str = sys.platform) -> str:
    """Textual binding string: the alt chord, plus the macOS Option character when there is one."""
    char = option_char(raw, platform)
    return f"{textual_key(raw)},{char}" if char else textual_key(raw)


def _dq(text: str) -> str:
    """Escape for tmux's double-quoted strings, so shlex single quotes inside survive untouched."""
    return text.replace("\\", "\\\\").replace('"', '\\"').replace("$", "\\$")


def run_shell(launcher: str, subcommand: str) -> str:
    return f'run-shell -b "{_dq(launcher)} {subcommand}"'


def popup(launcher: str, subcommand: str, width: str, height: str) -> str:
    return f'display-popup -E -w {width} -h {height} "{_dq(launcher)} {subcommand}"'


def expand_command(command: str, launcher: str) -> str:
    """``{tandem}`` is the launcher for one tmux parse; ``{tandem2}`` for a command nested inside another."""
    return command.replace("{tandem2}", _dq(_dq(launcher))).replace("{tandem}", _dq(launcher))


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

    def display_key(self, platform: str = sys.platform) -> str:
        return display_shortcut(self.key, platform)

    def option_char(self, platform: str = sys.platform) -> str | None:
        return option_char(self.key, platform)


DEFAULT_KEYMAP: tuple[Binding, ...] = (
    Binding("Alt+h", "Help", "open this help", "Help",
            "display-popup -E -w 70% -h 70% \"{tandem} help\""),
    Binding("Ctrl+Alt+n", "Agents", "add an agent", "New agent",
            "display-popup -E -w 90% -h 90% \"{tandem} add-agent\""),
    Binding("Alt+Up", "Tabs", "previous agent in this column", "Prev",
            'run-shell -b "{tandem} tab prev"'),
    Binding("Alt+Down", "Tabs", "next agent in this column", "Next",
            'run-shell -b "{tandem} tab next"'),
    Binding("Alt+s", "Status", "focus the status window", "Status",
            'run-shell -b "{tandem} focus status"'),
    Binding("Alt+q", "Status", "open the Queries tab", "Queries",
            'run-shell -b "{tandem} focus queries"'),
    Binding("Alt+1", "Columns", "focus planners", "Planners",
            'run-shell -b "{tandem} focus planner"'),
    Binding("Alt+2", "Columns", "focus implementers", "Implementers",
            'run-shell -b "{tandem} focus implementer"'),
    Binding("Ctrl+Alt+w", "Agents", "close the focused agent (asks first)", "Close",
            "confirm-before -p 'Close this agent? (y/n)' \"run-shell -b \\\"{tandem2} close-agent\\\"\""),
    Binding("Ctrl+Alt+x", "Quit", "exit menu: save and exit, detach, update tandem, discard", "Quit",
            'run-shell -b "{tandem} quit"'),
    Binding("Ctrl+Alt+r", "Agents", "restart an exited agent", "Restart",
            'run-shell -b "{tandem} restart"'),
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


STATUS_REFRESH = "Alt+r"
GO_TO_AGENT = "Alt+g"
WITHDRAW = "Alt+w"
WIZARD_ADD = "Alt+a"
WIZARD_DELETE = "Alt+d"
WIZARD_FILTER = "Alt+f"
WIZARD_HELP = "Alt+h"


CLICK_ACTIONS: tuple[str, ...] = (
    "help",        # 0: Alt+h
    "add-agent",   # 1: Ctrl+Alt+n
    "prev",        # 2: Alt+Up
    "next",        # 3: Alt+Down
    "status",      # 4: Alt+s
    "queries",     # 5: Alt+q
    "planner",     # 6: Alt+1
    "implementer", # 7: Alt+2
    "close-agent", # 8: Ctrl+Alt+w
    "quit",        # 9: Ctrl+Alt+x
    "restart",     # 10: Ctrl+Alt+r
    "resize",      # 11: Ctrl+Alt+z
)
"""Canonical action name for each KEYMAP entry; also the allow-list for ``tandem click``."""


def hint_bar(platform: str = sys.platform) -> str:
    return "  ".join(f"{b.display_key(platform)} {b.hint}" for b in KEYMAP if b.hint)


def hint_bar_with_ranges(platform: str = sys.platform,
                         km: tuple[Binding, ...] = KEYMAP) -> str:
    """Hint bar where each hint is wrapped in a tmux ``range=user|<action>`` click target."""
    parts = []
    for b, action in zip(km, CLICK_ACTIONS):
        if b.hint:
            parts.append(f"#[range=user|{action}]{b.display_key(platform)} {b.hint}#[norange]")
    return "  ".join(parts)


def resize_hint() -> str:
    return "RESIZE  ←→ width  ↑↓ height  = reset  Enter/Esc done"


def status_left() -> str:
    """Hint bar (with click ranges), swapped for the resize help while the resize key table is active."""
    return f" #{{?#{{==:#{{client_key_table}},{RESIZE_TABLE}}},{resize_hint()},{hint_bar_with_ranges()}}} "


def _mouse3_menu_cmd(launcher: str) -> str:
    """Single-string command for the right-click pane binding.

    Built as one string so that tmux stores it as a compound command rather than
    treating ``;`` as a live command separator during ``bind-key`` processing.
    """
    close_cmd = expand_command(KEYMAP[ACTION_MAP["close"]].command, launcher)
    add_agent_cmd = expand_command(KEYMAP[ACTION_MAP["add-agent"]].command, launcher)
    help_cmd = expand_command(KEYMAP[ACTION_MAP["help"]].command, launcher)

    def q(s: str) -> str:
        return f'"{_dq(s)}"'

    items = [
        f'{q("Focus this pane")} f {q("select-pane -t =")}',
        f'{q("#{?pane_dead,,-}Restart agent")} r {q(run_shell(launcher, "restart"))}',
        f'{q("Close agent…")} w {q(close_cmd)}',
        '""',
        f'{q("New agent")} n {q(add_agent_cmd)}',
        f'{q("Status")} s {q(run_shell(launcher, "focus status"))}',
        f'{q("Queries")} q {q(run_shell(launcher, "focus queries"))}',
        f'{q("Resize mode")} z {q(f"switch-client -T {RESIZE_TABLE}")}',
        f'{q("Help")} h {q(help_cmd)}',
    ]
    menu = "display-menu -O -T tandem -t = -x M -y M " + " ".join(items)
    return f"select-pane -t = ; {menu}"


def mouse_commands(python: str, workspace: str | None = None,
                   platform: str = sys.platform) -> list[list[str]]:
    """Bind/unbind argv lists for clickable status-bar hints and the right-click pane context menu."""
    launcher = launcher_for(python, workspace)

    click1_status: list[str] = [
        "bind-key", "-n", "MouseDown1Status",
        f'run-shell -b "{_dq(launcher)} click #{{q:mouse_status_range}}"',
    ]

    click3_pane: list[str] = [
        "bind-key", "-n", "MouseDown3Pane",
        _mouse3_menu_cmd(launcher),
    ]

    border_sub = "click-tab #{q:pane_id}"
    click1_border: list[str] = [
        "bind-key", "-n", "MouseDown1Border",
        f"select-pane -t = ; {run_shell(launcher, border_sub)}",
    ]

    return [
        click1_status,
        click3_pane,
        click1_border,
        ["unbind-key", "-q", "-n", "MouseDown3Status"],
        ["unbind-key", "-q", "-n", "MouseDown3StatusLeft"],
        ["unbind-key", "-q", "-n", "M-MouseDown3Pane"],
    ]


def _global_help_lines(platform: str) -> list[str]:
    grouped: dict[str, list[Binding]] = {}
    for b in KEYMAP:
        grouped.setdefault(b.group, []).append(b)
    lines = ["Global (tmux)"]
    for group, items in grouped.items():
        for position, b in enumerate(items):
            lines.append(f"  {group if position == 0 else '':<9} {b.display_key(platform):<12} {b.description}")
    return lines


def wizard_help_lines(platform: str = sys.platform) -> list[str]:
    def d(raw: str) -> str:
        return display_shortcut(raw, platform)

    return [f"{d(WIZARD_FILTER)} focus the model filter", f"{d(WIZARD_ADD)} add another agent",
            f"{d(WIZARD_DELETE)} delete the highlighted agent", "Shift+L launch all agents",
            "Esc back or cancel", f"? or {d(WIZARD_HELP)} this help"]


def help_lines(platform: str = sys.platform) -> list[str]:
    def d(raw: str) -> str:
        return display_shortcut(raw, platform)

    resize_k = d(key_for("resize") or "Ctrl+Alt+z")
    return [
        "tandem keys (planners = left column, implementers = right column)", "",
        *_global_help_lines(platform), "",
        "Status & Queries window",
        "  Status    Tab          switch between the Status and Queries tabs",
        f"            {d(STATUS_REFRESH):<12} refresh the status now",
        f"            {d(GO_TO_AGENT):<12} go to the highlighted or asking agent's pane",
        "  Queries   Enter        answer the selected open question",
        "            1-9          fill the answer with a numbered option",
        "            Ctrl+S       send the answer",
        f"            {d(WITHDRAW):<12} mark the selected question withdrawn",
        "            Esc          clear the draft and return to the list", "",
        "Launch / add-agent wizard",
        f"  Model     {d(WIZARD_FILTER):<12} focus the model filter",
        f"  Summary   {d(WIZARD_ADD):<12} add another agent",
        f"            {d(WIZARD_DELETE):<12} delete the highlighted agent",
        "            Shift+L      launch all configured agents",
        "            Esc          back or cancel", "",
        "Mouse",
        "            click a status-bar hint = press its key",
        "            click a column title to pick one of its agents from a menu",
        "            right-click a pane = context menu (focus, restart, close, new agent, …)",
        "            click a pane to focus it; drag a border to resize; scroll wheel scrolls",
        f"  Resize    {resize_k} then arrows (←→ {RESIZE_STEP_COLS} columns, ↑↓ {RESIZE_STEP_ROWS} rows) on the focused pane; = resets; Enter exits",
        f"  Custom    set TANDEM_KEYS=\"help={d('Ctrl+Alt+h')},quit={d('Ctrl+Alt+q')}\" (each needs at least one modifier)",
        "  Detach    your agents keep running; run `tandem` again to re-attach",
    ]


def launcher_for(python: str, workspace: str | None = None, module: str = "tandem") -> str:
    """Shell snippet that runs tandem; pins the workspace so panes in other worktrees still agree."""
    base = f"{shlex.quote(python)} -m {module}"
    return f"env TANDEM_WORKSPACE={shlex.quote(workspace)} {base}" if workspace else base


def bind_commands(python: str, workspace: str | None = None, platform: str = sys.platform) -> list[list[str]]:
    """tmux ``bind-key`` argv lists (root table) for every binding, plus macOS Option characters on macOS."""
    launcher = launcher_for(python, workspace)
    result: list[list[str]] = []
    for b in KEYMAP:
        command = expand_command(b.command, launcher)
        result.append(["bind-key", "-n", b.tmux_key, *_split(command)])
        char = b.option_char(platform)
        if char:
            result.append(["bind-key", "-n", char, *_split(command)])
    return result


def resize_commands(python: str, workspace: str | None = None) -> list[list[str]]:
    """``bind-key`` argv lists for the resize key table; every arrow stays in the table until Enter/Esc."""
    stay = f"switch-client -T {RESIZE_TABLE}"
    bindings = {
        "Left": f"resize-pane -L {RESIZE_STEP_COLS} ; {stay}", "Right": f"resize-pane -R {RESIZE_STEP_COLS} ; {stay}",
        "Up": f"resize-pane -U {RESIZE_STEP_ROWS} ; {stay}", "Down": f"resize-pane -D {RESIZE_STEP_ROWS} ; {stay}",
        "=": f"{run_shell(launcher_for(python, workspace), 'reset-layout')} ; {stay}",
        "Enter": "switch-client -T root", "Escape": "switch-client -T root", "q": "switch-client -T root",
    }
    return [["bind-key", "-T", RESIZE_TABLE, key, command] for key, command in bindings.items()]


def _split(command: str) -> list[str]:
    """tmux accepts a bound command as one string argument."""
    return [command]
