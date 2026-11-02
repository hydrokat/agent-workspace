mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::TempDir;
use tandem::adapters::AgentSpec;
use tandem::cli::parse_from;
use tandem::commands::quit_menu;
use tandem::state::{self, SavedState};
use tandem::tmux::client::{Result as TmuxResult, TmuxClient, TmuxRunner};
use tandem::tmux::layout::Layout;

struct MockTmuxRunner {
    calls: Mutex<Vec<Vec<String>>>,
    has_session_code: i32,
}

impl MockTmuxRunner {
    fn new(has_session_code: i32) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            has_session_code,
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl TmuxRunner for MockTmuxRunner {
    fn run(&self, argv: &[String]) -> TmuxResult {
        self.calls.lock().unwrap().push(argv.to_vec());
        let code = if argv.iter().any(|a| a == "has-session") {
            self.has_session_code
        } else {
            0
        };
        TmuxResult {
            code,
            out: String::new(),
            err: String::new(),
        }
    }
}

fn make_saved() -> SavedState {
    let mut active = BTreeMap::new();
    active.insert("planner".to_string(), 1);
    SavedState::new(
        vec![AgentSpec::new("claude", "planner", "opus")],
        active,
        Some(40),
        Some(10),
        1.0,
    )
}

#[test]
fn test_flags_are_mutually_exclusive() {
    let res = parse_from(["tandem", "--resume", "--fresh"]);
    assert!(res.is_err());
}

#[test]
fn test_saved_state_round_trip() {
    let tmp = TempDir::new();
    let saved = make_saved();
    let sp = state::state_path(tmp.path());
    state::save(&sp, &saved).unwrap();

    let loaded = state::load(&sp).unwrap();
    assert_eq!(loaded.agents.len(), 1);
    assert_eq!(loaded.agents[0].cli, "claude");
    assert_eq!(loaded.columns_percent, Some(40));
    assert_eq!(loaded.status_rows, Some(10));
}

#[test]
fn test_quit_menu_offers_save_update_detach_discard() {
    let runner = Arc::new(MockTmuxRunner::new(0));
    let client = TmuxClient::with_runner("tandem-test", None, runner.clone());
    let mut layout = Layout::new(&client);
    layout.self_cmd = Some(vec!["/bin/tandem".to_string()]);

    quit_menu(&layout, Some("/ws"));

    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    let flat = calls[0].join(" ");
    assert!(flat.contains("display-menu"));
    assert!(flat.contains("save-exit"));
    assert!(flat.contains("tandem update") || flat.contains("update"));
    assert!(flat.contains("detach-client"));
    assert!(flat.contains("discard-exit"));
    assert!(flat.contains("TANDEM_WORKSPACE=/ws"));
}

#[test]
fn test_legacy_session_check_runner() {
    let runner_found = Arc::new(MockTmuxRunner::new(0));
    let client_found = TmuxClient::with_runner("tandem", None, runner_found.clone());
    let res_found = client_found.run(&["has-session", "-t", "tandem"]).unwrap();
    assert_eq!(res_found.code, 0);

    let runner_not_found = Arc::new(MockTmuxRunner::new(1));
    let client_not_found = TmuxClient::with_runner("tandem", None, runner_not_found.clone());
    let res_not_found = client_not_found
        .run(&["has-session", "-t", "tandem"])
        .unwrap();
    assert_eq!(res_not_found.code, 1);
}
