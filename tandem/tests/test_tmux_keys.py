import shlex

from tandem.tmux import keys
from tandem.tmux.client import ALLOWED

EXPECTED = {"F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12"}


def test_keymap_covers_documented_keys() -> None:
    assert {b.key for b in keys.KEYMAP} == EXPECTED


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


def test_no_chord_keys_that_claude_or_agy_commonly_use() -> None:
    for b in keys.KEYMAP:
        assert b.key.startswith("F") and not b.key.startswith(("C-", "M-")), b.key


def test_hint_bar_is_generated_from_keymap() -> None:
    bar = keys.hint_bar()
    for b in keys.KEYMAP:
        if b.hint:
            assert f"{b.key} {b.hint}" in bar


def test_help_lines_cover_every_key() -> None:
    text = "\n".join(keys.help_lines())
    for b in keys.KEYMAP:
        assert b.key in text


def test_bind_commands_use_root_table_and_the_python_launcher() -> None:
    cmds = keys.bind_commands(python="/usr/bin/python 3")
    assert len(cmds) == len(keys.KEYMAP)
    for argv in cmds:
        assert argv[:3] == ["bind-key", "-n", argv[2]] and argv[2] in EXPECTED
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
    assert "F12" in "\n".join(keys.help_lines())
