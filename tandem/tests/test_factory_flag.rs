mod common;

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use common::TempDir;
use tandem::adapters::AgentSpec;
use tandem::commands::{
    agent_launch, mode_mismatch_message, sidebar_launch, status_launch, with_factory_env,
};
use tandem::config::Config;
use tandem::registry::{
    WorkspaceEntry, default_registry_path, factory_enabled, load_registry, register_or_heartbeat,
    register_or_heartbeat_with_mode,
};
use tandem::state::{SavedState, discard, load, save, state_path, state_path_for};

#[test]
fn test_config_factory_only_when_exact_one() {
    let empty = HashMap::new();
    assert!(!Config::from_env_source(&empty).unwrap().factory);

    for val in ["0", "true", "yes", "2", ""] {
        let mut env = HashMap::new();
        env.insert("TANDEM_FACTORY".to_string(), val.to_string());
        assert!(
            !Config::from_env_source(&env).unwrap().factory,
            "TANDEM_FACTORY={val:?} should not enable factory"
        );
    }

    let mut enabled = HashMap::new();
    enabled.insert("TANDEM_FACTORY".to_string(), "1".to_string());
    assert!(Config::from_env_source(&enabled).unwrap().factory);
}

#[test]
fn test_state_path_for_separates_modes() {
    let ws = Path::new("/tmp/my-workspace");
    assert_eq!(state_path_for(ws, false), state_path(ws));
    assert_eq!(
        state_path_for(ws, false),
        ws.join(".agent-workspace").join("tandem-session.json")
    );
    assert_eq!(
        state_path_for(ws, true),
        ws.join(".agent-workspace")
            .join("tandem-session-factory.json")
    );
}

#[test]
fn test_factory_and_normal_saved_sessions_do_not_collide() {
    let tmp = TempDir::new();
    let ws = tmp.path();

    let mut active = BTreeMap::new();
    active.insert("planner".to_string(), 1);

    let normal_state = SavedState::new(
        vec![AgentSpec {
            cli: "claude".to_string(),
            role: "planner".to_string(),
            model: "sonnet".to_string(),
            index: 1,
            skip_permissions: false,
            cwd: None,
            conversation: Some("conv-normal".to_string()),
            name: None,
            idea: None,
        }],
        active.clone(),
        Some(50),
        Some(4),
        100.0,
    );

    let factory_state = SavedState::new(
        vec![AgentSpec {
            cli: "claude".to_string(),
            role: "planner".to_string(),
            model: "opus".to_string(),
            index: 1,
            skip_permissions: false,
            cwd: None,
            conversation: Some("conv-factory".to_string()),
            name: None,
            idea: None,
        }],
        active,
        Some(50),
        Some(4),
        200.0,
    );

    save(&state_path_for(ws, false), &normal_state).unwrap();
    save(&state_path_for(ws, true), &factory_state).unwrap();

    let loaded_normal = load(&state_path_for(ws, false)).expect("normal state");
    let loaded_factory = load(&state_path_for(ws, true)).expect("factory state");
    assert_eq!(loaded_normal.agents[0].model, "sonnet");
    assert_eq!(loaded_factory.agents[0].model, "opus");

    discard(&state_path_for(ws, true)).unwrap();
    assert!(load(&state_path_for(ws, true)).is_none());
    assert!(load(&state_path_for(ws, false)).is_some());
}

#[test]
fn test_registry_entry_factory_serialization_and_defaults() {
    let normal = WorkspaceEntry {
        id: "tandem-1".to_string(),
        name: "proj".to_string(),
        path: "/tmp/proj".to_string(),
        socket: "tandem-1".to_string(),
        updated_at: 100,
        pid: Some(1),
        factory: false,
    };
    let normal_json = serde_json::to_string(&normal).unwrap();
    assert!(
        !normal_json.contains("factory"),
        "normal entry must omit factory key: {normal_json}"
    );
    let parsed_normal: WorkspaceEntry = serde_json::from_str(&normal_json).unwrap();
    assert!(!parsed_normal.factory);

    let factory = WorkspaceEntry {
        factory: true,
        ..normal
    };
    let factory_json = serde_json::to_string(&factory).unwrap();
    assert!(factory_json.contains("\"factory\":true"));
    let parsed_factory: WorkspaceEntry = serde_json::from_str(&factory_json).unwrap();
    assert!(parsed_factory.factory);
}

#[test]
fn test_registry_factory_enabled_and_heartbeat_preservation() {
    let state_tmp = TempDir::new();
    let ws_tmp = TempDir::new();
    let state_dir = state_tmp.path();
    let ws = ws_tmp.path();

    assert!(!factory_enabled(state_dir, ws));

    let entry = register_or_heartbeat_with_mode(state_dir, ws, Some(101), true).unwrap();
    assert!(entry.factory);
    assert!(factory_enabled(state_dir, ws));

    let hb = register_or_heartbeat(state_dir, ws, Some(102)).unwrap();
    assert!(hb.factory, "heartbeat must preserve existing factory flag");
    assert!(factory_enabled(state_dir, ws));

    let normal_entry = register_or_heartbeat_with_mode(state_dir, ws, Some(103), false).unwrap();
    assert!(!normal_entry.factory);
    assert!(!factory_enabled(state_dir, ws));

    let reg = load_registry(&default_registry_path(state_dir)).unwrap();
    assert_eq!(reg.workspaces.len(), 1);
    assert!(!reg.workspaces[0].factory);
}

#[test]
fn test_with_factory_env_marks_launches_only_in_factory_mode() {
    let ws = Path::new("/tmp/ws");
    let spec = AgentSpec {
        cli: "claude".to_string(),
        role: "planner".to_string(),
        model: "sonnet".to_string(),
        index: 1,
        skip_permissions: false,
        cwd: None,
        conversation: None,
        name: None,
        idea: None,
    };

    let status_normal = with_factory_env(status_launch(None, Some(ws)), false);
    assert!(!status_normal.env.contains_key("TANDEM_FACTORY"));

    let status_factory = with_factory_env(status_launch(None, Some(ws)), true);
    assert_eq!(
        status_factory.env.get("TANDEM_FACTORY").map(String::as_str),
        Some("1")
    );

    let sidebar_factory = with_factory_env(sidebar_launch(None, Some(ws)), true);
    assert_eq!(
        sidebar_factory
            .env
            .get("TANDEM_FACTORY")
            .map(String::as_str),
        Some("1")
    );

    let agent_factory = with_factory_env(agent_launch(&spec, "/tmp/coord", ws, false, None), true);
    assert_eq!(
        agent_factory.env.get("TANDEM_FACTORY").map(String::as_str),
        Some("1")
    );
}

#[test]
fn test_mode_mismatch_messages() {
    assert_eq!(
        mode_mismatch_message(true, false),
        Some(
            "A normal tandem session is running in this workspace. Quit it first (Settings → Quit), then run ./run.sh --factory."
        )
    );
    assert_eq!(
        mode_mismatch_message(false, true),
        Some(
            "A Factory Mode session is running in this workspace. Quit it first, or run ./run.sh --factory to attach."
        )
    );
    assert_eq!(mode_mismatch_message(true, true), None);
    assert_eq!(mode_mismatch_message(false, false), None);
}
