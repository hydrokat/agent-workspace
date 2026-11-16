mod common;

use std::collections::BTreeMap;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use common::TempDir;
use tandem::adapters::AgentSpec;
use tandem::adapters::base::{
    LABEL_MAX_CHARS, LABEL_RULE_TEXT, is_label_char, is_valid_label, normalize_label,
};
use tandem::commands::{rename_agent_with_layout, rename_workspace_in};
use tandem::config::Config;
use tandem::registry::{default_registry_path, load_registry, register_or_heartbeat};
use tandem::state::{SavedState, save, state_path_for, write_json_atomic};
use tandem::tmux::client::{Result as TmuxResult, TmuxClient, TmuxRunner};
use tandem::tmux::layout::Layout;

#[derive(Default)]
struct RecordingRunner {
    calls: Mutex<Vec<Vec<String>>>,
}

impl TmuxRunner for RecordingRunner {
    fn run(&self, argv: &[String]) -> TmuxResult {
        self.calls.lock().unwrap().push(argv.to_vec());
        if argv.iter().any(|a| a == "list-panes") {
            let line = "%3\tmain\tagent\tplanner\t1\tclaude·opus\t0\t\tplanner-1\t\t\t";
            return TmuxResult {
                code: 0,
                out: format!("{line}\n"),
                err: String::new(),
            };
        }
        TmuxResult {
            code: 0,
            out: String::new(),
            err: String::new(),
        }
    }
}

#[test]
fn test_label_accept_table() {
    let valid = [
        "a",
        "A1",
        "Fleet Mgmt - Backend",
        "HTN Reg",
        "POS v2.1",
        "api_worker",
        "planner-1",
        "12345678901234567890123456789012",
    ];
    for label in valid {
        assert!(is_valid_label(label), "expected valid label: {label:?}");
    }
    assert_eq!(LABEL_MAX_CHARS, 32);
    assert!(!LABEL_RULE_TEXT.is_empty());
    assert!(is_label_char(' '));
    assert!(is_label_char('-'));
    assert!(is_label_char('_'));
    assert!(is_label_char('.'));
    assert!(!is_label_char('#'));
}

#[test]
fn test_label_reject_table() {
    let invalid = [
        "",
        " lead",
        "lead ",
        "two  spaces",
        "-x",
        "--help",
        ".env",
        "_x",
        "123456789012345678901234567890123",
        "a#b",
        "a{b}",
        "a,b",
        "a;b",
        "a|b",
        "a$b",
        "a`b",
        "a'b",
        "a\"b",
        "a\\b",
        "a/b",
        "a\tb",
        "a\nb",
        "a\u{0}b",
        "café",
        "名前",
        "a\u{200b}b",
    ];
    for label in invalid {
        assert!(!is_valid_label(label), "expected invalid label: {label:?}");
    }
}

#[test]
fn test_normalize_label() {
    assert_eq!(normalize_label("  a   b "), "a b");
    assert_eq!(normalize_label("   "), "");
    assert_eq!(normalize_label("lead "), "lead");
    assert!(is_valid_label(&normalize_label("lead ")));
    assert_eq!(normalize_label("two  spaces"), "two spaces");
    assert!(is_valid_label(&normalize_label("two  spaces")));
    assert_eq!(
        normalize_label("  Fleet  Mgmt - Backend "),
        "Fleet Mgmt - Backend"
    );
}

#[test]
fn test_write_json_atomic_concurrent_readers_never_see_invalid_json() {
    let tmp = TempDir::new();
    let target = tmp.path().join("session.json");
    let initial = serde_json::json!({ "version": 3, "count": 0 });
    write_json_atomic(&target, &initial).expect("initial write");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    let stop = Arc::new(AtomicBool::new(false));
    let stop_reader = Arc::clone(&stop);
    let target_reader = target.clone();
    let reader = std::thread::spawn(move || {
        let mut reads = 0usize;
        while !stop_reader.load(Ordering::Relaxed) {
            let bytes = fs::read(&target_reader).expect("read target");
            let val: serde_json::Value =
                serde_json::from_slice(&bytes).expect("reader must always parse valid JSON");
            assert_eq!(val["version"], 3);
            reads += 1;
        }
        reads
    });

    for i in 1..=200 {
        let payload = serde_json::json!({
            "version": 3,
            "count": i,
            "workspace_name": "Fleet Mgmt - Backend",
        });
        write_json_atomic(&target, &payload).expect("atomic write");
    }
    stop.store(true, Ordering::Relaxed);
    let reads = reader.join().expect("reader thread");
    assert!(reads > 0);
}

#[test]
fn test_rename_workspace_in_updates_registry_and_session_file() {
    let state_dir = TempDir::new();
    let ws_dir = TempDir::new();

    register_or_heartbeat(state_dir.path(), ws_dir.path(), Some(1234)).expect("register");

    let resolved = tandem::tmux::client::resolve_path(ws_dir.path());
    let session_file = state_path_for(&resolved, false);
    let saved = SavedState::new(
        vec![AgentSpec::new("claude", "planner", "opus")],
        BTreeMap::new(),
        None,
        None,
        1.0,
    );
    save(&session_file, &saved).expect("save session");

    let res = rename_workspace_in(
        state_dir.path(),
        ws_dir.path(),
        Some("  Fleet Mgmt  - Backend "),
    )
    .expect("rename workspace");
    assert_eq!(res["ok"], true);
    assert_eq!(res["name"], "Fleet Mgmt - Backend");

    let reg = load_registry(&default_registry_path(state_dir.path())).expect("load registry");
    assert_eq!(reg.workspaces[0].name, "Fleet Mgmt - Backend");

    let session_json: serde_json::Value =
        serde_json::from_slice(&fs::read(&session_file).unwrap()).unwrap();
    assert_eq!(session_json["workspace_name"], "Fleet Mgmt - Backend");

    let reset_res = rename_workspace_in(state_dir.path(), ws_dir.path(), Some("   "))
        .expect("reset workspace name");
    assert_eq!(reset_res["ok"], true);
    let session_after: serde_json::Value =
        serde_json::from_slice(&fs::read(&session_file).unwrap()).unwrap();
    assert!(session_after.get("workspace_name").is_none());
}

#[test]
fn test_rename_agent_with_layout_passes_single_argv_element_and_persists() {
    let state_dir = TempDir::new();
    let ws_dir = TempDir::new();
    let coord_dir = ws_dir.path().join(".agent-workspace").join("coordination");
    fs::create_dir_all(coord_dir.join("sessions")).unwrap();

    let resolved = tandem::tmux::client::resolve_path(ws_dir.path());
    let session_file = state_path_for(&resolved, false);
    let saved = SavedState::new(
        vec![AgentSpec::new("claude", "planner", "opus")],
        BTreeMap::new(),
        None,
        None,
        1.0,
    );
    save(&session_file, &saved).expect("save session");

    let coord_session_file = coord_dir.join("sessions").join("planner-1.json");
    write_json_atomic(
        &coord_session_file,
        &serde_json::json!({ "id": "planner-1", "role": "planner" }),
    )
    .unwrap();

    let runner = Arc::new(RecordingRunner::default());
    let client = TmuxClient::with_runner("tandem-test", None, runner.clone());
    let mut layout = Layout::new(&client);
    layout.workspace = Some(resolved.to_string_lossy().to_string());

    let cfg = Config {
        state_dir: state_dir.path().to_path_buf(),
        coord_dir: Some(coord_dir.clone()),
        ..Default::default()
    };

    let res = rename_agent_with_layout(
        ws_dir.path(),
        &cfg,
        &layout,
        "%3",
        Some("  API worker - v2.1 "),
    )
    .expect("rename agent");
    assert_eq!(res["ok"], true);
    assert_eq!(res["name"], "API worker - v2.1");

    let calls = runner.calls.lock().unwrap();
    let set_opt = calls
        .iter()
        .find(|argv| {
            argv.iter().any(|a| a == "set-option") && argv.iter().any(|a| a == "@tandem_name")
        })
        .expect("set-option @tandem_name call");
    assert_eq!(
        set_opt.last().map(String::as_str),
        Some("API worker - v2.1")
    );

    let session_json: serde_json::Value =
        serde_json::from_slice(&fs::read(&session_file).unwrap()).unwrap();
    assert_eq!(session_json["agents"][0]["name"], "API worker - v2.1");

    let coord_json: serde_json::Value =
        serde_json::from_slice(&fs::read(&coord_session_file).unwrap()).unwrap();
    assert_eq!(coord_json["name"], "API worker - v2.1");
}
