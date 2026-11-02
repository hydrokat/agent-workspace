mod common;

use common::TempDir;
use common::parity::load_fixture_value;
use serde_json::Value;
use std::path::Path;
use tandem::adapters::{Adapter, AgentSpec, AgyAdapter, ClaudeAdapter, statusline_settings};
use tandem::conversations::{from_agy_log, valid_id};
use tandem::coordination::{Claim, Query, Session, decode_query_fields, encode_query_fields};
use tandem::notice::{headline, whats_new};
use tandem::quota::{ClaudeQuota, Window, tmux_safe};
use tandem::sources::conflicts::find_conflicts;
use tandem::state::{parse, to_json};
use tandem::status::render::{age, bar};
use tandem::tmux::keys::{Binding, default_keymap, display_shortcut, hint_bar, option_char};
use tandem::tmux::layout::conf_text;
use tandem::update_check::{CheckState, due};
use tandem::updater::plan;
use tandem::versions::parse_version;

#[test]
fn test_parity_adapters() {
    let fixture = load_fixture_value("adapters");
    let custom_settings = statusline_settings(Some(&["/usr/local/bin/tandem"]));
    assert_eq!(
        custom_settings,
        fixture["statusline_settings"]["custom"].as_str().unwrap()
    );

    let claude_models = ClaudeAdapter::new().list_models().unwrap();
    let expected_claude: Vec<String> =
        serde_json::from_value(fixture["claude_models"].clone()).unwrap();
    let ids: Vec<String> = claude_models.into_iter().map(|m| m.id).collect();
    assert_eq!(ids, expected_claude);

    let matrix = fixture["build_argv_matrix"].as_array().unwrap();
    assert_eq!(matrix.len(), 192);

    for case in matrix {
        let cli = case["cli"].as_str().unwrap();
        let role = case["role"].as_str().unwrap();
        let model = case["model"].as_str().unwrap();
        let skip_permissions = case["skip_permissions"].as_bool().unwrap();
        let resume = case["resume"].as_bool().unwrap();
        let log_file = case["log_file"].as_str();
        let conversation = case["conversation"].as_str().map(|s| s.to_string());

        let spec = AgentSpec {
            cli: cli.to_string(),
            role: role.to_string(),
            model: model.to_string(),
            index: 1,
            skip_permissions,
            cwd: Some("/workspace".to_string()),
            conversation,
        };

        let (adapter_claude, adapter_agy);
        let adapter: &dyn Adapter = if cli == "claude" {
            adapter_claude = ClaudeAdapter::with_self_cmd(vec!["tandem".to_string()]);
            &adapter_claude
        } else {
            adapter_agy = AgyAdapter::new();
            &adapter_agy
        };

        let result = adapter.build_argv(&spec, resume, log_file);
        if case["error"].is_null() {
            let expected_argv: Vec<String> = serde_json::from_value(case["argv"].clone()).unwrap();
            assert_eq!(
                result.unwrap(),
                expected_argv,
                "Mismatch for case: {:?}",
                case
            );
        } else {
            assert!(result.is_err(), "Expected error for case: {:?}", case);
        }
    }
}

#[test]
fn test_parity_conversations() {
    let fixture = load_fixture_value("conversations");
    let valid_id_map = fixture["valid_id"].as_object().unwrap();
    for (input, expected) in valid_id_map {
        let actual = valid_id(input);
        if expected.is_null() {
            assert!(actual.is_none());
        } else {
            assert_eq!(actual.as_deref(), expected.as_str());
        }
    }

    let temp_dir = TempDir::new();
    let sample_file = temp_dir.path().join("sample.log");
    std::fs::write(
        &sample_file,
        "2026-11-01 info: starting agent\n\
         2026-11-01 info: Created conversation c0a80101-0000-0000-0000-000000000002\n\
         2026-11-01 info: running message\n",
    )
    .unwrap();
    assert_eq!(
        from_agy_log(&sample_file).as_deref(),
        fixture["from_agy_log_sample"].as_str()
    );
}

#[test]
fn test_parity_coordination() {
    let fixture = load_fixture_value("coordination");
    let expected_encoded = fixture["encode_query_fields"].as_str().unwrap();
    let decoded_fields = decode_query_fields(expected_encoded).unwrap();
    let re_encoded = encode_query_fields(&decoded_fields);
    let re_decoded = decode_query_fields(&re_encoded).unwrap();
    assert_eq!(re_decoded, decoded_fields);

    let claim_raw = fixture["claim_from_dict"].as_object().unwrap();
    let claim = Claim::from_dict(claim_raw).unwrap();
    assert_eq!(claim.phase, "p-001");
    assert_eq!(claim.task, "t-002");
    assert_eq!(claim.session, "s-1");

    let session_raw = fixture["session_from_dict"].as_object().unwrap();
    let session = Session::from_dict(session_raw).unwrap();
    assert_eq!(session.id, "s-1");
    assert_eq!(session.cli, "claude");

    let query_raw = fixture["query_from_dict"].as_object().unwrap();
    let query = Query::from_dict(query_raw).unwrap();
    assert_eq!(query.id, "q-1");
    assert_eq!(query.status, "open");
}

#[test]
fn test_parity_keys() {
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
                expected.as_str().unwrap()
            );
        }
    }

    let option_cases = fixture["option_char"].as_object().unwrap();
    for platform in ["darwin", "linux"] {
        let cases = option_cases[platform].as_object().unwrap();
        for (input, expected) in cases {
            let actual = option_char(input, platform);
            let expected_opt = expected.as_str().and_then(|s| s.chars().next());
            assert_eq!(actual, expected_opt);
        }
    }

    let hint_bar_cases = fixture["hint_bar"].as_object().unwrap();
    for platform in ["darwin", "linux"] {
        assert_eq!(
            hint_bar(platform, &km),
            hint_bar_cases[platform].as_str().unwrap()
        );
    }
}

#[test]
fn test_parity_notice() {
    let fixture = load_fixture_value("notice");
    let h_bold = headline("- **Major Rust port foundations**: initial work.");
    assert_eq!(h_bold, fixture["whats_new_headline_bold"].as_str().unwrap());

    let h_period = headline("- Some sentence. More text.");
    assert_eq!(
        h_period,
        fixture["whats_new_headline_period"].as_str().unwrap()
    );

    let sample_changelog = "\
# Changelog

## [1.5.0] - 2026-11-01
- **Major Rust port foundations**: initial work.
- Second bullet item without bold.
";
    let list = whats_new(sample_changelog, "1.4.0", "1.5.0", None);
    let expected: Vec<String> = serde_json::from_value(fixture["whats_new_text"].clone()).unwrap();
    assert_eq!(list, expected);
}

#[test]
fn test_parity_quota() {
    let fixture = load_fixture_value("quota");
    let tmux_safe_cases = fixture["tmux_safe"].as_object().unwrap();
    for (input, expected) in tmux_safe_cases {
        assert_eq!(tmux_safe(input), expected.as_str().unwrap());
    }

    let now = 1700000000.0;
    let parse_cases = fixture["parse_statusline"].as_object().unwrap();
    let q_full: ClaudeQuota =
        serde_json::from_value(parse_cases["full"].clone()).expect("parse full");
    assert_eq!(
        q_full.five_hour,
        Some(Window {
            used_percentage: 35.0,
            resets_at: (now + 3600.0) as i64,
        })
    );
    assert_eq!(
        q_full.seven_day,
        Some(Window {
            used_percentage: 80.0,
            resets_at: (now + 86400.0 * 2.0) as i64,
        })
    );
}

#[test]
fn test_parity_render() {
    let fixture = load_fixture_value("render");
    let age_cases = fixture["age"].as_object().unwrap();
    assert_eq!(age(12.0), age_cases["12s"].as_str().unwrap());
    assert_eq!(age(840.0), age_cases["840s"].as_str().unwrap());
    assert_eq!(age(7200.0), age_cases["7200s"].as_str().unwrap());

    let bar_cases = fixture["bar"].as_object().unwrap();
    assert_eq!(bar(0, 0, 4), bar_cases["0_of_0_w4"].as_str().unwrap());
    assert_eq!(bar(1, 2, 4), bar_cases["1_of_2_w4"].as_str().unwrap());
    assert_eq!(bar(3, 3, 6), bar_cases["3_of_3_w6"].as_str().unwrap());
}

#[test]
fn test_parity_sources() {
    let _fixture = load_fixture_value("sources");
    let claims = vec![
        Claim {
            phase: "p-001".to_string(),
            task: "t-001".to_string(),
            session: "sess-1".to_string(),
            branch: String::new(),
            worktree: String::new(),
            paths: vec!["src/main.rs".to_string()],
            heartbeat: 100.0,
        },
        Claim {
            phase: "p-001".to_string(),
            task: "t-002".to_string(),
            session: "sess-2".to_string(),
            branch: String::new(),
            worktree: String::new(),
            paths: vec!["src/main.rs".to_string()],
            heartbeat: 100.0,
        },
    ];
    let conflicts = find_conflicts(&claims);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].first, "t-001");
}

#[test]
fn test_parity_state() {
    let fixture = load_fixture_value("state");
    let parse_cases = fixture["parse"].as_object().unwrap();
    let good_state = parse(&parse_cases["good"]).expect("parse good state");
    assert_eq!(good_state.agents.len(), 2);
    assert_eq!(good_state.agents[0].cli, "claude");
    assert_eq!(good_state.agents[1].cli, "agy");
    assert_eq!(good_state.active.get("planner"), Some(&1));

    let json_val = to_json(&good_state);
    assert_eq!(json_val, fixture["to_json"]);
}

#[test]
fn test_parity_tmux() {
    let fixture = load_fixture_value("tmux");
    let conf_cases = fixture["conf_text"].as_object().unwrap();
    assert_eq!(conf_text(50000), conf_cases["default"].as_str().unwrap());
    assert_eq!(
        conf_text(10000),
        conf_cases["history_10000"].as_str().unwrap()
    );
}

#[test]
fn test_parity_update_check() {
    let fixture = load_fixture_value("update_check");
    let due_cases = fixture["due"].as_array().unwrap();
    for case in due_cases {
        let last = case["last"].as_f64().unwrap();
        let now = case["now"].as_f64().unwrap();
        let interval = case["interval"].as_i64().unwrap();
        let expected = case["expected"].as_bool().unwrap();

        let state = CheckState {
            last,
            latest: String::new(),
            skip: String::new(),
        };
        assert_eq!(due(&state, now, interval), expected);
    }
}

#[test]
fn test_parity_updater() {
    let fixture_str = include_str!("fixtures/parity/updater.json");
    let fixture: Value = serde_json::from_str(fixture_str).unwrap();

    let dummy_ws = Path::new("/dummy/ws");
    let actual_frozen = plan(dummy_ws, "1.5.0", Some(true));
    let expected_frozen: Vec<String> = fixture["plan_frozen"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(actual_frozen, expected_frozen);

    let actual_not_frozen = plan(dummy_ws, "1.5.0", Some(false));
    let expected_not_frozen: Vec<String> = fixture["plan_not_frozen"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(actual_not_frozen, expected_not_frozen);
}

#[test]
fn test_parity_versions() {
    let fixture = load_fixture_value("versions");
    let parse_version_cases = fixture["parse_version"]
        .as_object()
        .expect("parse_version map");
    for (input, expected) in parse_version_cases {
        let actual = parse_version(input);
        if expected.is_null() {
            assert_eq!(actual, None);
        } else {
            let actual = actual.unwrap();
            let expected_parts: Vec<u32> = expected["parts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect();
            let expected_suffix = expected["suffix"].as_str().unwrap();
            assert_eq!(actual.parts, expected_parts);
            assert_eq!(actual.suffix, expected_suffix);
        }
    }
}
