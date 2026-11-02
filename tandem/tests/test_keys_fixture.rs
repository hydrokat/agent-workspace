mod common;

use common::parity::load_fixture_value;
use tandem::tmux::keys::{
    Binding, bind_commands, default_keymap, display_shortcut, help_lines, hint_bar,
    hint_bar_with_ranges, keymap, mouse_commands, option_char, resize_commands, wizard_help_lines,
};

#[test]
fn test_keys_parity_fixture() {
    let fixture = load_fixture_value("keys");

    let km = default_keymap();
    let expected_keymap: Vec<Binding> =
        serde_json::from_value(fixture["default_keymap"].clone()).unwrap();
    assert_eq!(km, expected_keymap);

    let display_cases = fixture["display_shortcut"].as_object().unwrap();
    for platform in ["darwin", "linux"] {
        let cases = display_cases[platform].as_object().unwrap();
        for (input, expected) in cases {
            assert_eq!(
                display_shortcut(input, platform).unwrap(),
                expected.as_str().unwrap(),
                "display_shortcut failed for {input} on {platform}"
            );
        }
    }

    let option_cases = fixture["option_char"].as_object().unwrap();
    for platform in ["darwin", "linux"] {
        let cases = option_cases[platform].as_object().unwrap();
        for (input, expected) in cases {
            let actual = option_char(input, platform);
            let expected_opt = expected.as_str().and_then(|s| s.chars().next());
            assert_eq!(
                actual, expected_opt,
                "option_char failed for {input} on {platform}"
            );
        }
    }

    let hint_bar_cases = fixture["hint_bar"].as_object().unwrap();
    for platform in ["darwin", "linux"] {
        assert_eq!(
            hint_bar(platform, &km),
            hint_bar_cases[platform].as_str().unwrap(),
            "hint_bar mismatch on {platform}"
        );
    }

    let hint_bar_ranged = fixture["hint_bar_with_ranges"].as_object().unwrap();
    for platform in ["darwin", "linux"] {
        assert_eq!(
            hint_bar_with_ranges(platform, &km),
            hint_bar_ranged[platform].as_str().unwrap(),
            "hint_bar_with_ranges mismatch on {platform}"
        );
    }

    let help_cases = fixture["help_lines"].as_object().unwrap();
    for platform in ["darwin", "linux"] {
        let expected_lines: Vec<String> =
            serde_json::from_value(help_cases[platform].clone()).unwrap();
        assert_eq!(
            help_lines(platform, &km),
            expected_lines,
            "help_lines mismatch on {platform}"
        );
    }

    let wizard_help = fixture["wizard_help_lines"].as_object().unwrap();
    for platform in ["darwin", "linux"] {
        let expected_lines: Vec<String> =
            serde_json::from_value(wizard_help[platform].clone()).unwrap();
        assert_eq!(
            wizard_help_lines(platform),
            expected_lines,
            "wizard_help_lines mismatch on {platform}"
        );
    }

    let bind_cases = fixture["bind_commands"].as_object().unwrap();
    for platform in ["darwin", "linux"] {
        let expected_binds: Vec<Vec<String>> =
            serde_json::from_value(bind_cases[platform].clone()).unwrap();
        assert_eq!(
            bind_commands(None, None, platform, Some("/usr/bin/python3"), &km),
            expected_binds,
            "bind_commands mismatch on {platform}"
        );
    }

    let expected_mouse: Vec<Vec<String>> =
        serde_json::from_value(fixture["mouse_commands"].clone()).unwrap();
    assert_eq!(
        mouse_commands(None, None, "linux", Some("/usr/bin/python3"), &km),
        expected_mouse
    );

    let expected_resize: Vec<Vec<String>> =
        serde_json::from_value(fixture["resize_commands"].clone()).unwrap();
    assert_eq!(
        resize_commands(None, None, Some("/usr/bin/python3")),
        expected_resize
    );

    let overrides = fixture["overrides"].as_object().unwrap();
    let expected_valid: Vec<Binding> = serde_json::from_value(overrides["valid"].clone()).unwrap();
    let mut empty_env = std::collections::HashMap::new();
    assert_eq!(keymap(Some(&empty_env)).unwrap(), expected_valid);

    let expected_empty: Vec<Binding> = serde_json::from_value(overrides["empty"].clone()).unwrap();
    empty_env.insert("TANDEM_KEYS".to_string(), "".to_string());
    assert_eq!(keymap(Some(&empty_env)).unwrap(), expected_empty);
}
