mod common;

use std::collections::{BTreeMap, HashMap};
use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use common::TempDir;
use tandem::adapters::AgentSpec;
use tandem::commands::{
    ModeConflictChoice, agent_launch, ask_mode_conflict_with, mode_mismatch_message,
    mode_mismatch_prompt, parse_mode_conflict_choice, sidebar_launch, status_launch,
    terminate_for_mode_switch, with_factory_env,
};
use tandem::config::Config;
use tandem::registry::{
    WorkspaceEntry, default_registry_path, factory_enabled, load_registry, register_or_heartbeat,
    register_or_heartbeat_with_mode,
};
use tandem::state::{SavedState, discard, load, save, state_path, state_path_for};
use tandem::tmux::client::{Result as TmuxResult, TmuxClient, TmuxRunner};
use tandem::tmux::layout::Layout;

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

struct MockSwitchTmuxRunner {
    alive: AtomicBool,
    panes_output: String,
    calls: Mutex<Vec<Vec<String>>>,
}

impl MockSwitchTmuxRunner {
    fn new(panes_output: &str) -> Self {
        Self {
            alive: AtomicBool::new(true),
            panes_output: panes_output.to_string(),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl TmuxRunner for MockSwitchTmuxRunner {
    fn run(&self, argv: &[String]) -> TmuxResult {
        self.calls.lock().unwrap().push(argv.to_vec());
        if argv.iter().any(|a| a == "has-session") {
            let code = if self.alive.load(Ordering::SeqCst) {
                0
            } else {
                1
            };
            return TmuxResult {
                code,
                out: String::new(),
                err: String::new(),
            };
        }
        if argv.iter().any(|a| a == "kill-session") {
            self.alive.store(false, Ordering::SeqCst);
            return TmuxResult {
                code: 0,
                out: String::new(),
                err: String::new(),
            };
        }
        let out = if argv.iter().any(|a| a == "list-panes") {
            self.panes_output.clone()
        } else {
            String::new()
        };
        TmuxResult {
            code: 0,
            out,
            err: String::new(),
        }
    }
}

fn agent_spec_for(role: &str, cli: &str, model: &str) -> AgentSpec {
    AgentSpec {
        cli: cli.to_string(),
        role: role.to_string(),
        model: model.to_string(),
        index: 1,
        skip_permissions: false,
        cwd: None,
        conversation: Some(format!("conv-{role}")),
        name: None,
        idea: None,
    }
}

#[test]
fn test_mode_conflict_prompt_and_aliases() {
    assert_eq!(mode_mismatch_prompt(true, true), None);
    assert_eq!(mode_mismatch_prompt(false, false), None);

    let to_factory = mode_mismatch_prompt(true, false).expect("tandem -> factory");
    assert!(to_factory.contains("[T]erminate tandem"));
    assert!(to_factory.contains("[l]aunch tandem"));

    let to_tandem = mode_mismatch_prompt(false, true).expect("factory -> tandem");
    assert!(to_tandem.contains("[T]erminate Factory Mode"));
    assert!(to_tandem.contains("[l]aunch Factory Mode"));

    for raw in ["", "t", "terminate", "y", "yes", "1"] {
        assert_eq!(
            parse_mode_conflict_choice(raw),
            Some(ModeConflictChoice::TerminateAndLaunch)
        );
    }
    for raw in ["l", "launch", "a", "attach", "m", "manual", "2"] {
        assert_eq!(
            parse_mode_conflict_choice(raw),
            Some(ModeConflictChoice::AttachRunning)
        );
    }
    for raw in ["q", "quit", "n", "no", "c", "cancel", "3"] {
        assert_eq!(
            parse_mode_conflict_choice(raw),
            Some(ModeConflictChoice::Cancel)
        );
    }
    assert_eq!(parse_mode_conflict_choice("invalid"), None);

    let mut non_tty_out = Vec::new();
    let mut non_tty_in = Cursor::new(b"t\n");
    assert_eq!(
        ask_mode_conflict_with(true, false, false, &mut non_tty_in, &mut non_tty_out),
        None
    );
    assert!(non_tty_out.is_empty());

    let mut tty_out = Vec::new();
    let mut tty_in = Cursor::new(b"oops\n\n");
    assert_eq!(
        ask_mode_conflict_with(true, false, true, &mut tty_in, &mut tty_out),
        Some(ModeConflictChoice::TerminateAndLaunch)
    );
    let rendered = String::from_utf8(tty_out).unwrap();
    assert_eq!(rendered.matches("[T]erminate tandem").count(), 2);

    let mut attach_out = Vec::new();
    let mut attach_in = Cursor::new(b"launch\n");
    assert_eq!(
        ask_mode_conflict_with(false, true, true, &mut attach_in, &mut attach_out),
        Some(ModeConflictChoice::AttachRunning)
    );
}

#[test]
fn test_terminate_for_mode_switch_saves_active_mode_without_cross_contaminating() {
    let ws_tmp = TempDir::new();
    let state_tmp = TempDir::new();
    let ws = ws_tmp.path();
    let state_dir = state_tmp.path();

    register_or_heartbeat_with_mode(state_dir, ws, Some(101), false).unwrap();

    let existing_factory_state = SavedState::new(
        vec![agent_spec_for("orchestrator", "claude", "opus")],
        BTreeMap::from([("orchestrator".to_string(), 1)]),
        Some(26),
        Some(14),
        100.0,
    );
    save(&state_path_for(ws, true), &existing_factory_state).unwrap();

    let panes = "%1\tmain\tagent\tplanner\t1\tclaude·sonnet\t0\t\tplanner-1\t{\"cli\":\"claude\",\"model\":\"sonnet\"}\t1.0";
    let runner = Arc::new(MockSwitchTmuxRunner::new(panes));
    let client = TmuxClient::with_runner("tandem-test", None, runner.clone());
    let layout = Layout::new(&client);

    let saved = terminate_for_mode_switch(&layout, ws, state_dir, false).unwrap();
    assert_eq!(saved, 1);
    assert!(!layout.exists());

    let saved_normal = load(&state_path_for(ws, false)).expect("normal state saved");
    assert_eq!(saved_normal.agents.len(), 1);
    assert_eq!(saved_normal.agents[0].role, "planner");
    assert_eq!(saved_normal.agents[0].model, "sonnet");

    let preserved_factory = load(&state_path_for(ws, true)).expect("factory state untouched");
    assert_eq!(preserved_factory.agents[0].role, "orchestrator");
    assert_eq!(preserved_factory.agents[0].model, "opus");

    assert!(
        load_registry(&default_registry_path(state_dir))
            .unwrap()
            .workspaces
            .is_empty()
    );
    let calls = runner.calls();
    assert!(
        calls
            .iter()
            .any(|args| args.windows(2).any(|w| w == ["@tandem_closing", "1"]))
    );
    assert!(
        calls
            .iter()
            .any(|args| args.iter().any(|a| a == "kill-session"))
    );
}

#[test]
fn test_terminate_for_mode_switch_reverse_direction_and_zero_agents() {
    let ws_tmp = TempDir::new();
    let state_tmp = TempDir::new();
    let ws = ws_tmp.path();
    let state_dir = state_tmp.path();

    register_or_heartbeat_with_mode(state_dir, ws, Some(202), true).unwrap();
    let existing_normal_state = SavedState::new(
        vec![agent_spec_for("planner", "agy", "flash")],
        BTreeMap::from([("planner".to_string(), 1)]),
        Some(26),
        Some(10),
        100.0,
    );
    save(&state_path_for(ws, false), &existing_normal_state).unwrap();

    let factory_panes = "%2\tmain\tagent\torchestrator\t1\tclaude·opus\t0\t\torchestrator-1\t{\"cli\":\"claude\",\"model\":\"opus\"}\t1.0";
    let factory_runner = Arc::new(MockSwitchTmuxRunner::new(factory_panes));
    let factory_layout = Layout::new(&TmuxClient::with_runner(
        "tandem-test",
        None,
        factory_runner,
    ));

    let saved = terminate_for_mode_switch(&factory_layout, ws, state_dir, true).unwrap();
    assert_eq!(saved, 1);
    assert!(!factory_layout.exists());

    let saved_factory = load(&state_path_for(ws, true)).expect("factory state saved");
    assert_eq!(saved_factory.agents[0].role, "orchestrator");
    assert_eq!(saved_factory.agents[0].model, "opus");

    let preserved_normal = load(&state_path_for(ws, false)).expect("normal state untouched");
    assert_eq!(preserved_normal.agents[0].role, "planner");
    assert_eq!(preserved_normal.agents[0].model, "flash");

    register_or_heartbeat_with_mode(state_dir, ws, Some(303), false).unwrap();
    let placeholder_only = "%3\tmain\town\tsidebar\t0\tsidebar\t0\t\t\t\t1.0";
    let empty_runner = Arc::new(MockSwitchTmuxRunner::new(placeholder_only));
    let empty_layout = Layout::new(&TmuxClient::with_runner("tandem-test", None, empty_runner));

    let zero_saved = terminate_for_mode_switch(&empty_layout, ws, state_dir, true).unwrap();
    assert_eq!(zero_saved, 0);
    assert!(!empty_layout.exists());
    assert!(
        load(&state_path_for(ws, true)).is_some(),
        "zero-agent switch must not discard previously saved factory state"
    );
    assert!(
        load_registry(&default_registry_path(state_dir))
            .unwrap()
            .workspaces
            .is_empty()
    );
}
