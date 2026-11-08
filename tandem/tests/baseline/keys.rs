use crate::common::assert_golden;
use tandem::tmux::keys::{
    bind_commands, default_keymap, help_lines, mouse_commands, resize_commands, status_left,
};

#[test]
fn test_golden_keys_bind_commands() {
    let km = default_keymap();
    let bin_args = ["/usr/local/bin/tandem"];
    let bin = Some(&bin_args[..]);
    let ws = Some("/mock/workspace");

    let linux_cmds = bind_commands(bin, ws, "linux", &km);
    let linux_text = linux_cmds
        .into_iter()
        .map(|args| args.join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    assert_golden("keys/bind_commands_linux.txt", &linux_text);

    let darwin_cmds = bind_commands(bin, ws, "darwin", &km);
    let darwin_text = darwin_cmds
        .into_iter()
        .map(|args| args.join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    assert_golden("keys/bind_commands_darwin.txt", &darwin_text);
}

#[test]
fn test_golden_keys_mouse_commands() {
    let km = default_keymap();
    let bin_args = ["/usr/local/bin/tandem"];
    let bin = Some(&bin_args[..]);
    let ws = Some("/mock/workspace");

    let linux_cmds = mouse_commands(bin, ws, "linux", &km);
    let linux_text = linux_cmds
        .into_iter()
        .map(|args| args.join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    assert_golden("keys/mouse_commands_linux.txt", &linux_text);

    let darwin_cmds = mouse_commands(bin, ws, "darwin", &km);
    let darwin_text = darwin_cmds
        .into_iter()
        .map(|args| args.join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    assert_golden("keys/mouse_commands_darwin.txt", &darwin_text);
}

#[test]
fn test_golden_keys_resize_commands() {
    let bin_args = ["/usr/local/bin/tandem"];
    let bin = Some(&bin_args[..]);
    let ws = Some("/mock/workspace");

    let cmds = resize_commands(bin, ws);
    let text = cmds
        .into_iter()
        .map(|args| args.join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    assert_golden("keys/resize_commands.txt", &text);
}

#[test]
fn test_golden_keys_help_and_status() {
    let km = default_keymap();

    let linux_help = help_lines("linux", &km).join("\n");
    assert_golden("keys/help_lines_linux.txt", &linux_help);

    let darwin_help = help_lines("darwin", &km).join("\n");
    assert_golden("keys/help_lines_darwin.txt", &darwin_help);

    let linux_status = status_left("linux", &km);
    assert_golden("keys/status_left_linux.txt", &linux_status);

    let darwin_status = status_left("darwin", &km);
    assert_golden("keys/status_left_darwin.txt", &darwin_status);
}
