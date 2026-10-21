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
    bar = keys.hint_bar()
    for b in keys.KEYMAP:
        if b.hint:
            assert f"{b.key} {b.hint}" in bar


def test_help_lines_cover_every_key() -> None:
    text = "\n".join(keys.help_lines())
    for b in keys.KEYMAP:
        assert b.key in text


def test_bind_commands_use_root_table_and_tmux_key() -> None:
    cmds = keys.bind_commands(python="/usr/bin/python 3")
    assert len(cmds) == len(keys.KEYMAP)
    expected_tmux = {keys.tmux_key(k) for k in EXPECTED_KEYS}
    for argv in cmds:
        assert argv[:3] == ["bind-key", "-n", argv[2]] and argv[2] in expected_tmux
    joined = " ".join(" ".join(a) for a in cmds)
    assert "'/usr/bin/python 3'" in joined


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
    assert "client_key_table" in text and "RESIZE" in text and keys.hint_bar() in text


def test_resize_is_documented_in_help() -> None:
    assert "Ctrl+Alt+z" in "\n".join(keys.help_lines())


def test_tandem_keys_override() -> None:
    env = {"TANDEM_KEYS": "help=Alt+F1,quit=Ctrl+Alt+q"}
    custom = keys.keymap(env)
    assert custom[0].key == "Alt+F1"
    assert custom[9].key == "Ctrl+Alt+q"
    # Unoverridden bindings remain default
    assert custom[1].key == "Ctrl+Alt+n"
