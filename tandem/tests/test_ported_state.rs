mod common;

use std::collections::BTreeMap;
use std::fs;

use common::TempDir;
use tandem::adapters::base::AgentSpec;
use tandem::state::{STATE_VERSION, SavedState, describe, load, parse, save, to_json};

fn spec_a() -> AgentSpec {
    AgentSpec {
        cli: "claude".to_string(),
        role: "planner".to_string(),
        model: "sonnet".to_string(),
        index: 1,
        skip_permissions: false,
        cwd: Some("/w".to_string()),
        conversation: None,
    }
}

fn spec_b() -> AgentSpec {
    AgentSpec {
        cli: "agy".to_string(),
        role: "implementer".to_string(),
        model: "gemini-3".to_string(),
        index: 2,
        skip_permissions: true,
        cwd: None,
        conversation: None,
    }
}

fn sample() -> SavedState {
    let mut active = BTreeMap::new();
    active.insert("planner".to_string(), 1);
    active.insert("implementer".to_string(), 2);
    SavedState::new(vec![spec_a(), spec_b()], active, Some(40), Some(10), 1000.0)
}

#[test]
fn test_round_trip() {
    let tmp = TempDir::new();
    let path = tmp.path().join("ws/.agent-workspace/tandem-session.json");
    save(&path, &sample()).unwrap();
    assert_eq!(load(&path), Some(sample()));
}

#[test]
fn test_file_is_owner_only_json() {
    let tmp = TempDir::new();
    let path = tmp.path().join("s.json");
    save(&path, &sample()).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    let raw: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(raw["version"], STATE_VERSION);

    let entries: Vec<_> = fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].file_name(), "s.json");
}

#[test]
fn test_missing_corrupt_and_wrong_version_load_as_none() {
    let tmp = TempDir::new();
    assert!(load(&tmp.path().join("nope.json")).is_none());

    let bad = tmp.path().join("bad.json");
    fs::write(&bad, "{not json").unwrap();
    assert!(load(&bad).is_none());

    let wrong_v = serde_json::json!({
        "version": 99,
        "agents": [],
    });
    assert!(parse(&wrong_v).is_none());
    assert!(parse(&serde_json::json!([1, 2])).is_none());
}

#[test]
fn test_failed_save_keeps_previous_file() {
    let tmp = TempDir::new();
    let path = tmp.path().join("s.json");
    save(&path, &sample()).unwrap();

    let mut bad_state = sample();
    bad_state.saved_at = f64::NAN;
    assert!(save(&path, &bad_state).is_err());

    assert_eq!(load(&path), Some(sample()));
    let entries: Vec<_> = fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].file_name(), "s.json");
}

#[test]
fn test_invalid_agents_are_dropped_not_trusted() {
    let good = serde_json::json!({
        "cli": "claude",
        "role": "planner",
        "model": "opus",
        "index": 1,
    });
    let bad = vec![
        serde_json::json!({"cli": "bash", "role": "planner", "model": "opus", "index": 1}),
        serde_json::json!({"cli": "claude", "role": "boss", "model": "opus", "index": 1}),
        serde_json::json!({"cli": "claude", "role": "planner", "model": "--rm -rf", "index": 1}),
        serde_json::json!({"cli": "claude", "role": "planner", "model": 5, "index": 1}),
        serde_json::json!({"cli": "claude", "role": "planner", "model": "opus", "index": 0}),
        serde_json::json!({"cli": "claude", "role": "planner", "model": "opus", "index": true}),
        serde_json::json!({"cli": "claude", "role": "planner", "model": "opus", "index": 1, "skip_permissions": true}),
        serde_json::json!({"cli": "claude", "role": "planner", "model": "opus", "index": 1, "cwd": "relative/dir"}),
        serde_json::json!({"cli": "claude", "role": "planner", "model": "opus", "index": 1, "cwd": 3}),
        serde_json::Value::String("str".to_string()),
        serde_json::Value::Null,
    ];

    let mut agents = bad;
    agents.push(good.clone());
    agents.push(good);

    let parsed = parse(&serde_json::json!({
        "version": 1,
        "agents": agents,
    }))
    .unwrap();

    assert_eq!(parsed.agents.len(), 1);
    assert_eq!(parsed.agents[0].cli, "claude");
    assert_eq!(parsed.agents[0].role, "planner");
    assert_eq!(parsed.agents[0].model, "opus");
    assert_eq!(parsed.agents[0].index, 1);
}

#[test]
fn test_no_valid_agents_means_no_state() {
    let data = serde_json::json!({
        "version": 1,
        "agents": [{"cli": "x"}],
    });
    assert!(parse(&data).is_none());
}

#[test]
fn test_active_and_layout_values_are_bounded() {
    let mut data = to_json(&sample());
    data["active"] = serde_json::json!({"planner": 1, "implementer": 7, "bogus": 1});
    data["layout"] = serde_json::json!({"columns_percent": 500, "status_rows": "10"});

    let parsed = parse(&data).unwrap();
    assert_eq!(parsed.active.len(), 1);
    assert_eq!(parsed.active.get("planner"), Some(&1));
    assert!(parsed.columns_percent.is_none());
    assert!(parsed.status_rows.is_none());
}

#[test]
fn test_describe_lists_agents_by_role() {
    let text = describe(&sample(), Some(1000.0 + 600.0));
    assert!(text.contains("10 min ago"));
    assert!(text.contains("claude·sonnet"));
    assert!(text.contains("agy·gemini-3"));
}

const CONV: &str = "46f2fc12-7f1f-4ca2-b714-06cbbfd669e9";

#[test]
fn test_conversations_and_clean_flag_round_trip() {
    let tmp = TempDir::new();
    let path = tmp.path().join("s.json");
    let mut active = BTreeMap::new();
    active.insert("planner".to_string(), 1);

    let mut saved = SavedState::new(
        vec![AgentSpec {
            cli: "claude".to_string(),
            role: "planner".to_string(),
            model: "opus".to_string(),
            index: 1,
            skip_permissions: false,
            cwd: None,
            conversation: Some(CONV.to_string()),
        }],
        active,
        None,
        None,
        5.0,
    );
    saved.clean = false;

    save(&path, &saved).unwrap();
    assert_eq!(load(&path), Some(saved));
    let raw: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(raw["version"], STATE_VERSION);
}

#[test]
fn test_version_1_files_still_load_without_conversations() {
    let data = serde_json::json!({
        "version": 1,
        "saved_at": 1.0,
        "agents": [{"cli": "claude", "role": "planner", "model": "opus", "index": 1}],
    });
    let loaded = parse(&data).unwrap();
    assert_eq!(loaded.agents[0].conversation, None);
    assert!(loaded.clean);
}

#[test]
fn test_bad_conversation_is_dropped_but_agent_kept() {
    let data = serde_json::json!({
        "version": 2,
        "agents": [{
            "cli": "agy",
            "role": "planner",
            "model": "m",
            "index": 1,
            "conversation": "--resume ../../etc",
        }],
    });
    let loaded = parse(&data).unwrap();
    assert_eq!(loaded.agents[0].conversation, None);
}

#[test]
fn test_describe_flags_unclean_exit_and_saved_chats() {
    let mut saved = SavedState::new(
        vec![
            AgentSpec {
                cli: "claude".to_string(),
                role: "planner".to_string(),
                model: "opus".to_string(),
                index: 1,
                skip_permissions: false,
                cwd: None,
                conversation: Some(CONV.to_string()),
            },
            AgentSpec {
                cli: "agy".to_string(),
                role: "implementer".to_string(),
                model: "m".to_string(),
                index: 1,
                skip_permissions: false,
                cwd: None,
                conversation: None,
            },
        ],
        BTreeMap::new(),
        None,
        None,
        0.0,
    );
    saved.clean = false;
    let text = describe(&saved, None);
    assert!(text.contains("did not exit cleanly"));
    assert!(text.contains("claude·opus (conversation)"));
    assert!(text.contains("agy·m"));
    assert!(!text.contains("agy·m (conversation)"));
}

#[test]
fn discard_removes_saved_session_and_tolerates_missing_file() {
    let dir = TempDir::new();
    let path = tandem::state::state_path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{}").unwrap();
    tandem::state::discard(&path).unwrap();
    assert!(!path.exists());
    tandem::state::discard(&path).unwrap();
}
