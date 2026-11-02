use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tandem::tmux::client::ALLOWED;
use tandem::tmux::keys::{
    Binding, CLICK_ACTIONS, KeyParseError, RESIZE_TABLE, alt_name, bind_commands, crossterm_key,
    default_keymap, display_shortcut, expand_command, format_shortcut, help_lines, hint_bar,
    hint_bar_with_ranges, key_for, keymap, launcher_for, matches_crossterm_event, mouse_commands,
    option_char, parse_shortcut, popup, resize_commands, resize_hint, run_shell, status_left,
    tmux_key, wizard_help_lines,
};

const EXPECTED_KEYS: [&str; 12] = [
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
];

#[test]
fn test_parse_shortcut_human_format() {
    assert_eq!(
        parse_shortcut("Alt+h").unwrap(),
        (vec!["Alt".to_string()], "h".to_string())
    );
    assert_eq!(
        parse_shortcut("Ctrl+Alt+n").unwrap(),
        (vec!["Ctrl".to_string(), "Alt".to_string()], "n".to_string())
    );
    assert_eq!(
        parse_shortcut("Alt+Shift+F1").unwrap(),
        (
            vec!["Alt".to_string(), "Shift".to_string()],
            "F1".to_string()
        )
    );
    assert_eq!(
        parse_shortcut("Ctrl+Alt+Shift+r").unwrap(),
        (
            vec!["Ctrl".to_string(), "Alt".to_string(), "Shift".to_string()],
            "r".to_string()
        )
    );
}

#[test]
fn test_parse_shortcut_tmux_format() {
    assert_eq!(
        parse_shortcut("M-h").unwrap(),
        (vec!["Alt".to_string()], "h".to_string())
    );
    assert_eq!(
        parse_shortcut("C-M-n").unwrap(),
        (vec!["Ctrl".to_string(), "Alt".to_string()], "n".to_string())
    );
    assert_eq!(
        parse_shortcut("M-S-F1").unwrap(),
        (
            vec!["Alt".to_string(), "Shift".to_string()],
            "F1".to_string()
        )
    );
    assert_eq!(
        parse_shortcut("C-M-S-r").unwrap(),
        (
            vec!["Ctrl".to_string(), "Alt".to_string(), "Shift".to_string()],
            "r".to_string()
        )
    );
    assert_eq!(
        parse_shortcut("M-Up").unwrap(),
        (vec!["Alt".to_string()], "Up".to_string())
    );
    assert_eq!(
        parse_shortcut("M-1").unwrap(),
        (vec!["Alt".to_string()], "1".to_string())
    );
}

#[test]
fn test_parse_shortcut_lowercase_format() {
    assert_eq!(
        parse_shortcut("alt+h").unwrap(),
        (vec!["Alt".to_string()], "h".to_string())
    );
    assert_eq!(
        parse_shortcut("ctrl+alt+n").unwrap(),
        (vec!["Ctrl".to_string(), "Alt".to_string()], "n".to_string())
    );
    assert_eq!(
        parse_shortcut("alt+shift+f1").unwrap(),
        (
            vec!["Alt".to_string(), "Shift".to_string()],
            "F1".to_string()
        )
    );
    assert_eq!(
        parse_shortcut("ctrl+alt+shift+r").unwrap(),
        (
            vec!["Ctrl".to_string(), "Alt".to_string(), "Shift".to_string()],
            "r".to_string()
        )
    );
}

#[test]
fn test_modifier_order_canonicalization() {
    assert_eq!(
        parse_shortcut("Shift+Alt+Ctrl+r").unwrap(),
        (
            vec!["Ctrl".to_string(), "Alt".to_string(), "Shift".to_string()],
            "r".to_string()
        )
    );
    assert_eq!(
        parse_shortcut("Alt+Ctrl+n").unwrap(),
        (vec!["Ctrl".to_string(), "Alt".to_string()], "n".to_string())
    );
    assert_eq!(
        parse_shortcut("Shift+Ctrl+r").unwrap(),
        (
            vec!["Ctrl".to_string(), "Shift".to_string()],
            "r".to_string()
        )
    );
}

#[test]
fn test_format_shortcut() {
    assert_eq!(format_shortcut("M-h").unwrap(), "Alt+h");
    assert_eq!(format_shortcut("c-m-n").unwrap(), "Ctrl+Alt+n");
    assert_eq!(format_shortcut("shift+alt+f1").unwrap(), "Alt+Shift+F1");
    assert_eq!(format_shortcut("C-M-S-r").unwrap(), "Ctrl+Alt+Shift+r");
}

#[test]
fn test_tmux_key() {
    assert_eq!(tmux_key("Alt+h").unwrap(), "M-h");
    assert_eq!(tmux_key("Ctrl+Alt+n").unwrap(), "C-M-n");
    assert_eq!(tmux_key("Alt+Shift+F1").unwrap(), "M-S-F1");
    assert_eq!(tmux_key("Ctrl+Alt+Shift+r").unwrap(), "C-M-S-r");
    assert_eq!(tmux_key("Alt+Up").unwrap(), "M-Up");
    assert_eq!(tmux_key("Alt+1").unwrap(), "M-1");
}

#[test]
fn test_reject_zero_modifier_shortcut() {
    for invalid in ["F1", "a", "Enter", "Up", "h", "1"] {
        let res = parse_shortcut(invalid);
        assert!(
            matches!(res, Err(KeyParseError::NoModifier(_))),
            "{invalid}"
        );
    }
}

#[test]
fn test_reject_duplicate_modifier() {
    for invalid in ["Ctrl+Ctrl+a", "C-C-n", "Alt+M-h", "Shift+shift+x"] {
        let res = parse_shortcut(invalid);
        assert!(
            matches!(res, Err(KeyParseError::DuplicateModifier(_, _))),
            "{invalid}"
        );
    }
}

#[test]
fn test_reject_unknown_modifier() {
    for invalid in ["Super+a", "Cmd+x", "Win+1", "Foo+bar"] {
        let res = parse_shortcut(invalid);
        assert!(
            matches!(res, Err(KeyParseError::UnknownModifier(_, _))),
            "{invalid}"
        );
    }
}

#[test]
fn test_reject_empty_base_key() {
    for invalid in ["", "   ", "Ctrl+", "C-M-", "Alt+", "Ctrl+Alt+"] {
        let res = parse_shortcut(invalid);
        assert!(res.is_err(), "{invalid}");
    }
}

#[test]
fn test_binding_properties() {
    let b = Binding::new(
        "Ctrl+Alt+n",
        "Agents",
        "add an agent",
        "New agent",
        "display-popup",
    )
    .unwrap();
    assert_eq!(b.modifiers().unwrap(), vec!["Ctrl", "Alt"]);
    assert_eq!(b.base_key().unwrap(), "n");
    assert_eq!(b.tmux_key().unwrap(), "C-M-n");
}

#[test]
fn test_keymap_covers_expected_keys() {
    let km = default_keymap();
    let actual: HashSet<&str> = km.iter().map(|b| b.key.as_str()).collect();
    let expected: HashSet<&str> = EXPECTED_KEYS.iter().copied().collect();
    assert_eq!(actual, expected);
}

#[test]
fn test_keys_are_unique() {
    let km = default_keymap();
    let names: Vec<&str> = km.iter().map(|b| b.key.as_str()).collect();
    let unique: HashSet<&str> = names.iter().copied().collect();
    assert_eq!(names.len(), unique.len());
}

#[test]
fn test_no_binding_can_type_into_or_read_a_pane() {
    let km = default_keymap();
    for b in &km {
        for word in [
            "send-keys",
            "capture-pane",
            "pipe-pane",
            "send-prefix",
            "paste-buffer",
        ] {
            assert!(!b.command.contains(word), "{} contains {}", b.key, word);
        }
    }
}

#[test]
fn test_bound_tmux_commands_are_on_allow_list_or_run_shell() {
    let km = default_keymap();
    for b in &km {
        let first = b.command.split_whitespace().next().unwrap();
        assert!(
            ALLOWED.contains(&first) || first == "run-shell",
            "{} has {}",
            b.key,
            first
        );
    }
}

#[test]
fn test_hint_bar_is_generated_from_keymap() {
    let km = default_keymap();
    for platform in ["linux", "darwin"] {
        let bar = hint_bar(platform, &km);
        for b in &km {
            if !b.hint.is_empty() {
                let disp = b.display_key(platform).unwrap();
                let expected = format!("{disp} {}", b.hint);
                assert!(bar.contains(&expected), "{expected} in {bar}");
            }
        }
    }
}

#[test]
fn test_help_lines_cover_every_key() {
    let km = default_keymap();
    for platform in ["linux", "darwin"] {
        let text = help_lines(platform, &km).join("\n");
        for b in &km {
            let disp = b.display_key(platform).unwrap();
            assert!(text.contains(&disp), "{disp} in help_lines");
        }
    }
}

#[test]
fn test_bind_commands_use_root_table_and_tmux_key() {
    let km = default_keymap();
    let cmds = bind_commands(None, None, "linux", Some("/usr/bin/python3"), &km);
    assert_eq!(cmds.len(), km.len());
    let expected_tmux: HashSet<String> =
        EXPECTED_KEYS.iter().map(|k| tmux_key(k).unwrap()).collect();
    for argv in &cmds {
        assert_eq!(argv[0], "bind-key");
        assert_eq!(argv[1], "-n");
        assert!(expected_tmux.contains(&argv[2]));
    }

    let cmds_mac = bind_commands(None, None, "darwin", Some("/usr/bin/python3"), &km);
    assert!(cmds_mac.len() > km.len());
    let mac_keys: HashSet<&str> = cmds_mac.iter().map(|a| a[2].as_str()).collect();
    assert!(mac_keys.contains("˙"));
    assert!(mac_keys.contains("ß"));
    assert!(mac_keys.contains("œ"));
}

#[test]
fn test_hint_bar_is_plain_text() {
    let km = default_keymap();
    let bar = hint_bar("linux", &km);
    assert!(bar.chars().all(|c| (c as u32) >= 32));
}

#[test]
fn test_workspace_is_pinned_in_every_bound_command() {
    let km = default_keymap();
    let cmds = bind_commands(None, Some("/my ws"), "linux", Some("/usr/bin/python3"), &km);
    for c in &cmds {
        if c[3].contains("run-shell") {
            assert!(c[3].contains("TANDEM_WORKSPACE='/my ws'"));
        }
    }
}

#[test]
fn test_resize_mode_keys_stay_in_table_and_can_exit() {
    let cmds = resize_commands(None, Some("/ws"), Some("/usr/bin/python3"));
    let mut map: HashMap<String, String> = HashMap::new();
    for c in &cmds {
        assert_eq!(c[0], "bind-key");
        assert_eq!(c[1], "-T");
        assert_eq!(c[2], RESIZE_TABLE);
        map.insert(c[3].clone(), c[4].clone());
    }

    for arrow in ["Left", "Right", "Up", "Down"] {
        let val = &map[arrow];
        assert!(val.starts_with("resize-pane"));
        assert!(val.contains("switch-client -T tandem-resize"));
    }
    assert_eq!(map["Enter"], "switch-client -T root");
    assert_eq!(map["Escape"], "switch-client -T root");
    assert_eq!(map["q"], "switch-client -T root");
    assert!(map["="].contains("reset-layout"));
    assert!(map["="].contains("TANDEM_WORKSPACE=/ws"));
}

#[test]
fn test_status_left_swaps_hint_while_resizing() {
    let km = default_keymap();
    let text = status_left("linux", &km);
    assert!(text.contains("client_key_table"));
    assert!(text.contains("RESIZE"));
    assert!(text.contains("#[range=user|"));
    assert!(text.contains(resize_hint()));
}

#[test]
fn test_resize_is_documented_in_help() {
    let km = default_keymap();
    assert!(help_lines("linux", &km).join("\n").contains("Ctrl+Alt+z"));
    assert!(help_lines("darwin", &km).join("\n").contains("Ctrl+Opt+z"));
}

#[test]
fn test_alt_name_and_display_shortcut() {
    assert_eq!(alt_name("darwin"), "Opt");
    assert_eq!(alt_name("linux"), "Alt");
    assert_eq!(alt_name("freebsd"), "Alt");

    assert_eq!(display_shortcut("Alt+h", "darwin").unwrap(), "Opt+h");
    assert_eq!(
        display_shortcut("Ctrl+Alt+r", "darwin").unwrap(),
        "Ctrl+Opt+r"
    );
    assert_eq!(display_shortcut("Alt+h", "linux").unwrap(), "Alt+h");
    assert_eq!(
        display_shortcut("Ctrl+Alt+r", "linux").unwrap(),
        "Ctrl+Alt+r"
    );
}

#[test]
fn test_option_char() {
    assert_eq!(option_char("Alt+h", "darwin"), Some('˙'));
    assert_eq!(option_char("Alt+s", "darwin"), Some('ß'));
    assert_eq!(option_char("Alt+q", "darwin"), Some('œ'));
    assert_eq!(option_char("Alt+1", "darwin"), Some('¡'));
    assert_eq!(option_char("Alt+2", "darwin"), Some('™'));
    assert_eq!(option_char("Alt+Up", "darwin"), None);
    assert_eq!(option_char("Ctrl+Alt+r", "darwin"), None);
    assert_eq!(option_char("Alt+h", "linux"), None);
}

#[test]
fn test_parse_shortcut_with_mac_modifiers() {
    assert_eq!(
        parse_shortcut("Opt+h").unwrap(),
        (vec!["Alt".to_string()], "h".to_string())
    );
    assert_eq!(
        parse_shortcut("Option+h").unwrap(),
        (vec!["Alt".to_string()], "h".to_string())
    );
    assert_eq!(
        parse_shortcut("⌥+h").unwrap(),
        (vec!["Alt".to_string()], "h".to_string())
    );
    assert_eq!(
        parse_shortcut("⌃+⌥+r").unwrap(),
        (vec!["Ctrl".to_string(), "Alt".to_string()], "r".to_string())
    );
    assert_eq!(
        parse_shortcut("⇧+Opt+a").unwrap(),
        (
            vec!["Alt".to_string(), "Shift".to_string()],
            "a".to_string()
        )
    );
}

#[test]
fn test_expand_command_and_run_shell_with_spaces() {
    let ws = "/path with spaces/my project";
    let launcher = launcher_for(Some(&["/custom path/tandem"]), Some(ws), None);
    let cmd = run_shell(&launcher, "add-agent");
    assert!(cmd.starts_with("run-shell -b \""));
    assert!(cmd.ends_with('"'));
    assert!(cmd.contains("/custom path/tandem"));
    assert!(cmd.contains("TANDEM_WORKSPACE="));

    let pop = popup(&launcher, "help", "80%", "70%");
    assert!(pop.starts_with("display-popup -E -w 80% -h 70% \""));
    assert!(pop.ends_with('"'));

    let expanded = expand_command("run-shell -b \"{tandem} cmd\"", &launcher);
    assert!(!expanded.contains("{tandem}"));
}

#[test]
fn test_key_for_lookup() {
    let km = default_keymap();
    assert_eq!(key_for("help", &km), "Alt+h");
    assert_eq!(key_for("quit", &km), "Ctrl+Alt+x");
    assert_eq!(key_for("unknown", &km), "");
}

#[test]
fn test_wizard_help_lines() {
    let lines_linux = wizard_help_lines("linux");
    assert!(lines_linux.iter().any(|l| l.contains("Alt+a")));
    assert!(lines_linux.iter().any(|l| l.contains('?')));

    let lines_darwin = wizard_help_lines("darwin");
    assert!(lines_darwin.iter().any(|l| l.contains("Opt+a")));
    assert!(lines_darwin.iter().any(|l| l.contains('?')));
}

#[test]
fn test_tandem_keys_override() {
    let mut env = HashMap::new();
    env.insert(
        "TANDEM_KEYS".to_string(),
        "help=Alt+F1,quit=Ctrl+Alt+q".to_string(),
    );
    let custom = keymap(Some(&env)).unwrap();
    assert_eq!(custom[0].key, "Alt+F1");
    assert_eq!(custom[9].key, "Ctrl+Alt+q");
    assert_eq!(custom[1].key, "Ctrl+Alt+n");
}

#[test]
fn test_tandem_keys_invalid_override_rejected() {
    let mut env = HashMap::new();
    env.insert("TANDEM_KEYS".to_string(), "help=F1".to_string());
    assert!(keymap(Some(&env)).is_err());
}

#[test]
fn test_click_actions_parallel_to_keymap() {
    let km = default_keymap();
    assert_eq!(CLICK_ACTIONS.len(), km.len());
    let unique: HashSet<&str> = CLICK_ACTIONS.iter().copied().collect();
    assert_eq!(unique.len(), CLICK_ACTIONS.len());
    for action in CLICK_ACTIONS {
        assert!(action.len() <= 15, "action '{action}' exceeds 15 bytes");
    }
}

#[test]
fn test_hint_bar_with_ranges_wraps_each_hint() {
    let km = default_keymap();
    for platform in ["linux", "darwin"] {
        let ranged = hint_bar_with_ranges(platform, &km);
        for (b, action) in km.iter().zip(CLICK_ACTIONS.iter()) {
            if !b.hint.is_empty() {
                assert!(ranged.contains(&format!("#[range=user|{action}]")));
                assert!(ranged.contains(&format!(
                    "{} {}#[norange]",
                    b.display_key(platform).unwrap(),
                    b.hint
                )));
            }
        }
    }
}

#[test]
fn test_status_left_resize_arm_has_no_ranges() {
    let km = default_keymap();
    let text = status_left("linux", &km);
    let rh = resize_hint();
    assert!(text.contains(rh));
    assert!(!rh.contains("#[range=user|"));
}

#[test]
fn test_mouse_commands_contains_expected_bindings() {
    let km = default_keymap();
    let cmds = mouse_commands(None, Some("/ws"), "linux", Some("/usr/bin/python3"), &km);
    let verbs: Vec<&str> = cmds.iter().map(|c| c[0].as_str()).collect();
    assert!(verbs.iter().filter(|v| **v == "bind-key").count() >= 2);
    assert_eq!(verbs.iter().filter(|v| **v == "unbind-key").count(), 3);
}

#[test]
fn test_mouse_commands_mousedown1status_runs_click() {
    let km = default_keymap();
    let cmds = mouse_commands(None, Some("/ws"), "linux", Some("/usr/bin/python3"), &km);
    let click1 = cmds.iter().find(|c| c[2] == "MouseDown1Status").unwrap();
    let cmd_str = &click1[3];
    assert!(cmd_str.contains("run-shell"));
    assert!(cmd_str.contains("click"));
    assert!(cmd_str.contains("mouse_status_range"));
}

#[test]
fn test_mouse_commands_mousedown3pane_has_display_menu() {
    let km = default_keymap();
    let cmds = mouse_commands(None, Some("/ws"), "linux", Some("/usr/bin/python3"), &km);
    let click3 = cmds.iter().find(|c| c[2] == "MouseDown3Pane").unwrap();
    let cmd_str = &click3[3];
    assert!(cmd_str.contains("display-menu"));
    assert!(cmd_str.contains("Focus this pane"));
    assert!(cmd_str.contains("Restart agent"));
    assert!(cmd_str.contains("Close agent"));
    assert!(cmd_str.contains("New agent"));
    assert!(cmd_str.contains("Help"));
}

#[test]
fn test_mouse_commands_unbinds_unsafe_defaults() {
    let km = default_keymap();
    let cmds = mouse_commands(None, None, "linux", Some("/usr/bin/python3"), &km);
    let unbind_keys: Vec<&str> = cmds
        .iter()
        .filter(|c| c[0] == "unbind-key")
        .map(|c| c.last().unwrap().as_str())
        .collect();
    assert!(unbind_keys.contains(&"MouseDown3Status"));
    assert!(unbind_keys.contains(&"MouseDown3StatusLeft"));
    assert!(unbind_keys.contains(&"M-MouseDown3Pane"));
}

#[test]
fn test_mouse_commands_does_not_touch_safe_defaults() {
    let km = default_keymap();
    let cmds = mouse_commands(None, None, "linux", Some("/usr/bin/python3"), &km);
    let touched: Vec<&str> = cmds
        .iter()
        .map(|c| {
            if c[0] == "unbind-key" {
                c.last().unwrap().as_str()
            } else {
                c[2].as_str()
            }
        })
        .collect();
    for safe in ["MouseDrag1Border", "MouseDown1Pane", "WheelUpPane"] {
        assert!(!touched.contains(&safe), "{safe} should not be touched");
    }
}

#[test]
fn test_mouse_commands_display_menu_no_forbidden_ops() {
    let km = default_keymap();
    for platform in ["linux", "darwin"] {
        let cmds = mouse_commands(None, Some("/ws"), platform, Some("/usr/bin/python3"), &km);
        let joined: String = cmds
            .iter()
            .flat_map(|a| a.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");
        for bad in [
            "send-keys",
            "capture-pane",
            "pipe-pane",
            "paste-buffer",
            "copy-mode",
        ] {
            assert!(!joined.contains(bad), "forbidden op '{bad}' found");
        }
    }
}

#[test]
fn test_mouse_commands_mousedown1border_runs_click_tab() {
    let km = default_keymap();
    let cmds = mouse_commands(None, Some("/ws"), "linux", Some("/usr/bin/python3"), &km);
    let border = cmds.iter().find(|c| c[2] == "MouseDown1Border").unwrap();
    let cmd_str = &border[3];
    assert!(cmd_str.contains("select-pane -t ="));
    assert!(cmd_str.contains("run-shell"));
    assert!(cmd_str.contains("click-tab"));
    assert!(cmd_str.contains("pane_id"));
}

#[test]
fn test_mouse_commands_mousedown1border_is_one_compound_string() {
    let km = default_keymap();
    let cmds = mouse_commands(None, Some("/ws"), "linux", Some("/usr/bin/python3"), &km);
    let border = cmds.iter().find(|c| c[2] == "MouseDown1Border").unwrap();
    assert_eq!(border.len(), 4);
}

#[test]
fn test_readme_keymap_matches_keys() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let readme_path = manifest_dir
        .parent()
        .unwrap()
        .join("tandem")
        .join("README.md");
    if readme_path.exists() {
        let readme = std::fs::read_to_string(&readme_path).unwrap();
        let km = default_keymap();
        for b in &km {
            let row = format!("| `{}` | {} |", b.key, b.description);
            assert!(
                readme.contains(&row),
                "README.md missing expected keymap row: {row}"
            );
        }
    }
}

#[test]
fn test_crossterm_key_and_event_matching() {
    let key = crossterm_key("Alt+h").unwrap();
    assert_eq!(key.code, KeyCode::Char('h'));
    assert_eq!(key.modifiers, KeyModifiers::ALT);

    let key_chord = crossterm_key("Ctrl+Alt+n").unwrap();
    assert_eq!(key_chord.code, KeyCode::Char('n'));
    assert_eq!(
        key_chord.modifiers,
        KeyModifiers::CONTROL | KeyModifiers::ALT
    );

    let key_arrow = crossterm_key("Alt+Up").unwrap();
    assert_eq!(key_arrow.code, KeyCode::Up);
    assert_eq!(key_arrow.modifiers, KeyModifiers::ALT);

    let ev = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::ALT);
    assert!(matches_crossterm_event("Alt+h", &ev, "linux"));
    assert!(matches_crossterm_event("Alt+h", &ev, "darwin"));

    let ev_mac_glyph = KeyEvent::new(KeyCode::Char('˙'), KeyModifiers::empty());
    assert!(matches_crossterm_event("Alt+h", &ev_mac_glyph, "darwin"));
    assert!(!matches_crossterm_event("Alt+h", &ev_mac_glyph, "linux"));
}
