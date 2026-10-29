import shlex
import pytest

from tandem.tmux import keys
from tandem.tmux.client import ALLOWED

EXPECTED_KEYS = {
    "Alt+h",
    "Ctrl+Alt+n",
    "Alt+Up",
    "Alt+Down",
    "Alt+s",
    "Alt+q",
    "Alt+1",
    "Alt+2",
    "Ctrl+Alt+w",
    "Ctrl+Alt+x",
    "Ctrl+Alt+r",
    "Ctrl+Alt+z",
}


def test_parse_shortcut_human_format() -> None:
    assert keys.parse_shortcut("Alt+h") == (("Alt",), "h")
    assert keys.parse_shortcut("Ctrl+Alt+n") == (("Ctrl", "Alt"), "n")
    assert keys.parse_shortcut("Alt+Shift+F1") == (("Alt", "Shift"), "F1")
    assert keys.parse_shortcut("Ctrl+Alt+Shift+r") == (("Ctrl", "Alt", "Shift"), "r")


def test_parse_shortcut_tmux_format() -> None:
    assert keys.parse_shortcut("M-h") == (("Alt",), "h")
    assert keys.parse_shortcut("C-M-n") == (("Ctrl", "Alt"), "n")
    assert keys.parse_shortcut("M-S-F1") == (("Alt", "Shift"), "F1")
    assert keys.parse_shortcut("C-M-S-r") == (("Ctrl", "Alt", "Shift"), "r")
    assert keys.parse_shortcut("M-Up") == (("Alt",), "Up")
    assert keys.parse_shortcut("M-1") == (("Alt",), "1")


def test_parse_shortcut_textual_format() -> None:
    assert keys.parse_shortcut("alt+h") == (("Alt",), "h")
    assert keys.parse_shortcut("ctrl+alt+n") == (("Ctrl", "Alt"), "n")
    assert keys.parse_shortcut("alt+shift+f1") == (("Alt", "Shift"), "F1")
    assert keys.parse_shortcut("ctrl+alt+shift+r") == (("Ctrl", "Alt", "Shift"), "r")


def test_modifier_order_canonicalization() -> None:
    assert keys.parse_shortcut("Shift+Alt+Ctrl+r") == (("Ctrl", "Alt", "Shift"), "r")
    assert keys.parse_shortcut("Alt+Ctrl+n") == (("Ctrl", "Alt"), "n")
    assert keys.parse_shortcut("Shift+Ctrl+r") == (("Ctrl", "Shift"), "r")


def test_format_shortcut() -> None:
    assert keys.format_shortcut("M-h") == "Alt+h"
    assert keys.format_shortcut("c-m-n") == "Ctrl+Alt+n"
    assert keys.format_shortcut("shift+alt+f1") == "Alt+Shift+F1"
    assert keys.format_shortcut("C-M-S-r") == "Ctrl+Alt+Shift+r"


def test_tmux_key() -> None:
    assert keys.tmux_key("Alt+h") == "M-h"
    assert keys.tmux_key("Ctrl+Alt+n") == "C-M-n"
    assert keys.tmux_key("Alt+Shift+F1") == "M-S-F1"
    assert keys.tmux_key("Ctrl+Alt+Shift+r") == "C-M-S-r"
    assert keys.tmux_key("Alt+Up") == "M-Up"
    assert keys.tmux_key("Alt+1") == "M-1"


def test_textual_key() -> None:
    assert keys.textual_key("Alt+h") == "alt+h"
    assert keys.textual_key("Ctrl+Alt+n") == "ctrl+alt+n"
    assert keys.textual_key("Alt+Shift+F1") == "alt+shift+f1"
    assert keys.textual_key("Ctrl+Alt+Shift+r") == "ctrl+alt+shift+r"
    assert keys.textual_key("Alt+Up") == "alt+up"


def test_reject_zero_modifier_shortcut() -> None:
    for invalid in ("F1", "a", "Enter", "Up", "h", "1"):
        with pytest.raises(ValueError, match="at least one modifier"):
            keys.parse_shortcut(invalid)


def test_reject_duplicate_modifier() -> None:
    for invalid in ("Ctrl+Ctrl+a", "C-C-n", "Alt+M-h", "Shift+shift+x"):
        with pytest.raises(ValueError, match="[Dd]uplicate"):
            keys.parse_shortcut(invalid)


def test_reject_unknown_modifier() -> None:
    for invalid in ("Super+a", "Cmd+x", "Win+1", "Foo+bar"):
        with pytest.raises(ValueError, match="[Uu]nknown"):
            keys.parse_shortcut(invalid)


def test_reject_empty_base_key() -> None:
    for invalid in ("", "Ctrl+", "C-M-", "Alt+", "Ctrl+Alt+"):
        with pytest.raises(ValueError):
            keys.parse_shortcut(invalid)


def test_binding_properties() -> None:
    b = keys.Binding("Ctrl+Alt+n", "Agents", "add an agent", "New agent", "cmd")
    assert b.modifiers == ("Ctrl", "Alt")
    assert b.base_key == "n"
    assert b.tmux_key == "C-M-n"
    assert b.textual_key == "ctrl+alt+n"


def test_keymap_covers_expected_keys() -> None:
    assert {b.key for b in keys.KEYMAP} == EXPECTED_KEYS


def test_keys_are_unique() -> None:
    names = [b.key for b in keys.KEYMAP]
    assert len(names) == len(set(names))


def test_no_binding_can_type_into_or_read_a_pane() -> None:
    for b in keys.KEYMAP:
        for word in ("send-keys", "capture-pane", "pipe-pane", "send-prefix", "paste-buffer"):
            assert word not in b.command, (b.key, word)


def test_bound_tmux_commands_are_on_allow_list_or_run_shell() -> None:
    for b in keys.KEYMAP:
        first = shlex.split(b.command)[0]
        assert first in ALLOWED | {"run-shell"}, (b.key, first)


def test_hint_bar_is_generated_from_keymap() -> None:
    for platform in ("linux", "darwin"):
        bar = keys.hint_bar(platform=platform)
        for b in keys.KEYMAP:
            if b.hint:
                assert f"{b.display_key(platform)} {b.hint}" in bar


def test_help_lines_cover_every_key() -> None:
    for platform in ("linux", "darwin"):
        text = "\n".join(keys.help_lines(platform=platform))
        for b in keys.KEYMAP:
            assert b.display_key(platform) in text


def test_bind_commands_use_root_table_and_tmux_key() -> None:
    cmds = keys.bind_commands(python="/usr/bin/python 3", platform="linux")
    assert len(cmds) == len(keys.KEYMAP)
    expected_tmux = {keys.tmux_key(k) for k in EXPECTED_KEYS}
    for argv in cmds:
        assert argv[:3] == ["bind-key", "-n", argv[2]] and argv[2] in expected_tmux
    joined = " ".join(" ".join(a) for a in cmds)
    assert "'/usr/bin/python 3'" in joined

    cmds_mac = keys.bind_commands(python="/usr/bin/python 3", platform="darwin")
    assert len(cmds_mac) > len(keys.KEYMAP)
    mac_keys = {argv[2] for argv in cmds_mac}
    assert "˙" in mac_keys
    assert "ß" in mac_keys
    assert "œ" in mac_keys


def test_hint_bar_is_plain_text() -> None:
    assert all(ord(c) >= 32 for c in keys.hint_bar())


def test_workspace_is_pinned_in_every_bound_command() -> None:
    cmds = keys.bind_commands(python="/py", workspace="/my ws")
    assert all("TANDEM_WORKSPACE='/my ws'" in c[3] for c in cmds if "run-shell" in c[3])


def test_resize_mode_keys_stay_in_table_and_can_exit() -> None:
    cmds = {c[3]: c[4] for c in keys.resize_commands("/py", "/ws")}
    for arrow in ("Left", "Right", "Up", "Down"):
        assert cmds[arrow].startswith("resize-pane") and "switch-client -T tandem-resize" in cmds[arrow]
    assert {cmds[k] for k in ("Enter", "Escape", "q")} == {"switch-client -T root"}
    assert "reset-layout" in cmds["="] and "TANDEM_WORKSPACE=/ws" in cmds["="]
    assert all(c[:3] == ["bind-key", "-T", "tandem-resize"] for c in keys.resize_commands("/py"))


def test_status_left_swaps_hint_while_resizing() -> None:
    text = keys.status_left()
    assert "client_key_table" in text
    assert "RESIZE" in text
    assert "#[range=user|" in text
    assert keys.resize_hint() in text


def test_resize_is_documented_in_help() -> None:
    assert "Ctrl+Alt+z" in "\n".join(keys.help_lines("linux"))
    assert "Ctrl+Opt+z" in "\n".join(keys.help_lines("darwin"))


def test_alt_name_and_display_shortcut() -> None:
    assert keys.alt_name("darwin") == "Opt"
    assert keys.alt_name("linux") == "Alt"
    assert keys.alt_name("freebsd") == "Alt"

    assert keys.display_shortcut("Alt+h", "darwin") == "Opt+h"
    assert keys.display_shortcut("Ctrl+Alt+r", "darwin") == "Ctrl+Opt+r"
    assert keys.display_shortcut("Alt+h", "linux") == "Alt+h"
    assert keys.display_shortcut("Ctrl+Alt+r", "linux") == "Ctrl+Alt+r"


def test_option_char_and_textual_binding_keys() -> None:
    assert keys.option_char("Alt+h", "darwin") == "˙"
    assert keys.option_char("Alt+s", "darwin") == "ß"
    assert keys.option_char("Alt+q", "darwin") == "œ"
    assert keys.option_char("Alt+1", "darwin") == "¡"
    assert keys.option_char("Alt+2", "darwin") == "™"
    assert keys.option_char("Alt+Up", "darwin") is None
    assert keys.option_char("Ctrl+Alt+r", "darwin") is None
    assert keys.option_char("Alt+h", "linux") is None

    assert keys.textual_binding_keys("Alt+q", "darwin") == "alt+q,œ"
    assert keys.textual_binding_keys("Alt+s", "darwin") == "alt+s,ß"
    assert keys.textual_binding_keys("Alt+q", "linux") == "alt+q"
    assert keys.textual_binding_keys("Ctrl+Alt+r", "darwin") == "ctrl+alt+r"


def test_parse_shortcut_with_mac_modifiers() -> None:
    assert keys.parse_shortcut("Opt+h") == (("Alt",), "h")
    assert keys.parse_shortcut("Option+h") == (("Alt",), "h")
    assert keys.parse_shortcut("⌥+h") == (("Alt",), "h")
    assert keys.parse_shortcut("⌃+⌥+r") == (("Ctrl", "Alt"), "r")
    assert keys.parse_shortcut("⇧+Opt+a") == (("Alt", "Shift"), "a")


def test_expand_command_and_run_shell_with_spaces() -> None:
    ws = "/path with spaces/my project"
    py = "/custom path/python3"
    launcher = keys.launcher_for(py, ws)
    cmd = keys.run_shell(launcher, "add-agent")
    assert cmd.startswith('run-shell -b "')
    assert cmd.endswith('"')
    assert f"TANDEM_WORKSPACE='{ws}'" in cmd
    assert f"'{py}'" in cmd

    pop = keys.popup(launcher, "help", width="80%", height="70%")
    assert pop.startswith('display-popup -E -w 80% -h 70% "')
    assert pop.endswith('"')
    assert f"TANDEM_WORKSPACE='{ws}'" in pop


def test_wizard_help_lines() -> None:
    lines_linux = keys.wizard_help_lines("linux")
    assert any("Alt+a" in line for line in lines_linux)
    assert any("?" in line for line in lines_linux)

    lines_darwin = keys.wizard_help_lines("darwin")
    assert any("Opt+a" in line for line in lines_darwin)
    assert any("?" in line for line in lines_darwin)



def test_tandem_keys_override() -> None:
    env = {"TANDEM_KEYS": "help=Alt+F1,quit=Ctrl+Alt+q"}
    custom = keys.keymap(env)
    assert custom[0].key == "Alt+F1"
    assert custom[9].key == "Ctrl+Alt+q"
    # Unoverridden bindings remain default
    assert custom[1].key == "Ctrl+Alt+n"


def test_click_actions_parallel_to_keymap() -> None:
    assert len(keys.CLICK_ACTIONS) == len(keys.DEFAULT_KEYMAP)
    assert len(set(keys.CLICK_ACTIONS)) == len(keys.CLICK_ACTIONS), "CLICK_ACTIONS must be unique"
    for action in keys.CLICK_ACTIONS:
        assert len(action.encode()) <= 15, f"action '{action}' exceeds 15 bytes"


def test_hint_bar_with_ranges_wraps_each_hint() -> None:
    for platform in ("linux", "darwin"):
        ranged = keys.hint_bar_with_ranges(platform)
        for b, action in zip(keys.KEYMAP, keys.CLICK_ACTIONS):
            if b.hint:
                assert f"#[range=user|{action}]" in ranged
                assert f"{b.display_key(platform)} {b.hint}#[norange]" in ranged


def test_hint_bar_with_ranges_has_no_bare_hints() -> None:
    ranged = keys.hint_bar_with_ranges("linux")
    assert "#[range=user|" in ranged
    assert "#[norange]" in ranged


def test_status_left_resize_arm_has_no_ranges() -> None:
    text = keys.status_left()
    resize_hint = keys.resize_hint()
    assert resize_hint in text
    assert "#[range=user|" not in resize_hint


def test_mouse_commands_contains_expected_bindings() -> None:
    cmds = keys.mouse_commands("/py", "/ws")
    verbs = [c[0] for c in cmds]
    assert verbs.count("bind-key") >= 2
    assert verbs.count("unbind-key") == 3


def test_mouse_commands_mousedown1status_runs_click() -> None:
    cmds = keys.mouse_commands("/py", "/ws")
    click1 = next(c for c in cmds if c[:3] == ["bind-key", "-n", "MouseDown1Status"])
    cmd_str = click1[3]
    assert "run-shell" in cmd_str
    assert "click" in cmd_str
    assert "mouse_status_range" in cmd_str


def test_mouse_commands_mousedown3pane_has_display_menu() -> None:
    cmds = keys.mouse_commands("/py", "/ws")
    click3 = next(c for c in cmds if c[:3] == ["bind-key", "-n", "MouseDown3Pane"])
    cmd_str = " ".join(click3)
    assert "display-menu" in cmd_str
    assert "Focus this pane" in cmd_str
    assert "Restart agent" in cmd_str
    assert "Close agent" in cmd_str
    assert "New agent" in cmd_str
    assert "Help" in cmd_str


def test_mouse_commands_unbinds_unsafe_defaults() -> None:
    cmds = keys.mouse_commands("/py")
    unbind_keys = [c[c.index("-n") + 1] for c in cmds if c[0] == "unbind-key" and "-n" in c]
    assert "MouseDown3Status" in unbind_keys
    assert "MouseDown3StatusLeft" in unbind_keys
    assert "M-MouseDown3Pane" in unbind_keys


def test_mouse_commands_does_not_touch_safe_defaults() -> None:
    cmds = keys.mouse_commands("/py")
    touched_keys = [c[c.index("-n") + 1] for c in cmds if c[0] in ("bind-key", "unbind-key") and "-n" in c]
    for safe in ("MouseDrag1Border", "MouseDown1Pane", "WheelUpPane"):
        assert safe not in touched_keys, f"{safe} must not be touched"


def test_mouse_commands_display_menu_no_forbidden_ops() -> None:
    for platform in ("linux", "darwin"):
        cmds = keys.mouse_commands("/py", "/ws", platform)
        full = " ".join(token for argv in cmds for token in argv)
        for bad in ("send-keys", "capture-pane", "pipe-pane", "paste-buffer", "copy-mode"):
            assert bad not in full, f"forbidden op '{bad}' found in mouse_commands"


def test_mouse_commands_quoting_paths_with_spaces() -> None:
    ws = "/path with spaces/proj"
    py = "/my python/python3"
    cmds = keys.mouse_commands(py, ws)
    full = " ".join(token for argv in cmds for token in argv)
    assert f"TANDEM_WORKSPACE='{ws}'" in full
    assert f"'{py}'" in full


def test_mouse_commands_both_platforms() -> None:
    for platform in ("linux", "darwin"):
        cmds = keys.mouse_commands("/py", "/ws", platform)
        assert len(cmds) == 6


def test_mouse_commands_unbind_uses_quiet_flag() -> None:
    cmds = keys.mouse_commands("/py")
    for c in cmds:
        if c[0] == "unbind-key":
            assert "-q" in c, f"unbind-key missing -q: {c}"


def test_help_lines_include_mouse_group() -> None:
    for platform in ("linux", "darwin"):
        lines = keys.help_lines(platform)
        text = "\n".join(lines)
        assert "Mouse" in text
        assert "right-click" in text
        assert "context menu" in text


def test_mouse_commands_mousedown1border_runs_click_tab() -> None:
    cmds = keys.mouse_commands("/py", "/ws")
    border = next(c for c in cmds if c[:3] == ["bind-key", "-n", "MouseDown1Border"])
    cmd_str = border[3]
    assert "select-pane -t =" in cmd_str
    assert "run-shell" in cmd_str
    assert "click-tab" in cmd_str
    assert "pane_id" in cmd_str
    assert "mouse_x" in cmd_str
    assert "pane_left" in cmd_str


def test_mouse_commands_mousedown1border_is_one_compound_string() -> None:
    cmds = keys.mouse_commands("/py", "/ws")
    border = next(c for c in cmds if c[:3] == ["bind-key", "-n", "MouseDown1Border"])
    # The command must be a single string argument (lessons-learned: compound bind-key commands)
    assert len(border) == 4


def test_help_lines_mention_tab_clicks() -> None:
    for platform in ("linux", "darwin"):
        text = "\n".join(keys.help_lines(platform))
        assert "tab" in text.lower() and "column" in text.lower()
