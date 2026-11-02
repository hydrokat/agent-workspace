mod common;

use common::parity::load_fixture_value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tandem::tmux::client::{Result, TmuxClient, TmuxError, TmuxRunner, ToSViolation, socket_for};

struct FakeRunner {
    calls: Mutex<Vec<Vec<String>>>,
    responses: HashMap<String, Result>,
}

impl FakeRunner {
    fn new(responses: HashMap<&str, Result>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            responses: responses
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl TmuxRunner for FakeRunner {
    fn run(&self, argv: &[String]) -> Result {
        self.calls.lock().unwrap().push(argv.to_vec());
        for (key, val) in &self.responses {
            if argv.iter().any(|arg| arg == key) {
                return val.clone();
            }
        }
        Result {
            code: 0,
            out: String::new(),
            err: String::new(),
        }
    }
}

fn create_client(runner: std::sync::Arc<FakeRunner>) -> TmuxClient {
    TmuxClient::with_runner("tandem-test", Some(PathBuf::from("/c/tandem.conf")), runner)
}

#[test]
fn test_prefixes_dedicated_socket_and_config() {
    let fake = std::sync::Arc::new(FakeRunner::new(HashMap::new()));
    let client = create_client(fake.clone());
    let _ = client.run(&["has-session", "-t", "tandem"]);
    assert_eq!(
        fake.calls(),
        vec![vec![
            "tmux",
            "-L",
            "tandem-test",
            "-f",
            "/c/tandem.conf",
            "has-session",
            "-t",
            "tandem"
        ]]
    );
}

#[test]
fn test_forbidden_or_unknown_subcommands_rejected() {
    let forbidden = [
        "send-keys",
        "capture-pane",
        "pipe-pane",
        "run-shell",
        "send-prefix",
        "save-buffer",
        "show-buffer",
        "paste-buffer",
        "load-buffer",
        "copy-mode",
        "if-shell",
        "wait-for",
        "totally-unknown",
    ];
    for sub in forbidden {
        let fake = std::sync::Arc::new(FakeRunner::new(HashMap::new()));
        let client = create_client(fake.clone());
        let res = client.run(&[sub, "-t", "%1"]);
        assert!(matches!(res, Err(ToSViolation::SubcommandNotAllowed(_))));
        assert!(fake.calls().is_empty());
    }
}

#[test]
fn test_spawn_move_focus_commands_allowed() {
    let allowed = [
        "new-session",
        "new-window",
        "split-window",
        "swap-pane",
        "select-pane",
        "resize-pane",
        "kill-pane",
        "set-option",
        "bind-key",
        "display-popup",
        "list-panes",
        "move-pane",
        "select-layout",
        "kill-session",
    ];
    for sub in allowed {
        let fake = std::sync::Arc::new(FakeRunner::new(HashMap::new()));
        let client = create_client(fake.clone());
        let res = client.run(&[sub, "-t", "%1"]);
        assert!(res.is_ok());
        assert_eq!(fake.calls().len(), 1);
    }
}

#[test]
fn test_run_checked_raises_on_failure() {
    let fake = std::sync::Arc::new(FakeRunner::new(HashMap::from([(
        "kill-pane",
        Result {
            code: 1,
            out: String::new(),
            err: "can't find pane".to_string(),
        },
    )])));
    let client = create_client(fake);
    let res = client.run_checked(&["kill-pane", "-t", "%9"]);
    assert!(matches!(res, Err(TmuxError::Failed { .. })));
}

#[test]
fn test_send_keys_own_requires_own_kind() {
    let fake = std::sync::Arc::new(FakeRunner::new(HashMap::from([(
        "display-message",
        Result {
            code: 0,
            out: "agent\n".to_string(),
            err: String::new(),
        },
    )])));
    let client = create_client(fake.clone());
    let res = client.send_keys_own("%1", &["Enter"]);
    assert_eq!(res, Err(ToSViolation::NotTandemOwned));
    assert!(
        !fake
            .calls()
            .iter()
            .any(|c| c.contains(&"send-keys".to_string()))
    );
}

#[test]
fn test_send_keys_own_fails_closed_on_unknown_kind() {
    let fake = std::sync::Arc::new(FakeRunner::new(HashMap::from([(
        "display-message",
        Result {
            code: 0,
            out: "\n".to_string(),
            err: String::new(),
        },
    )])));
    let client = create_client(fake);
    let res = client.send_keys_own("%1", &["Enter"]);
    assert_eq!(res, Err(ToSViolation::NotTandemOwned));
}

#[test]
fn test_send_keys_own_allowed_for_own_panes() {
    let fake = std::sync::Arc::new(FakeRunner::new(HashMap::from([(
        "display-message",
        Result {
            code: 0,
            out: "own\n".to_string(),
            err: String::new(),
        },
    )])));
    let client = create_client(fake.clone());
    let res = client.send_keys_own("%1", &["Enter"]);
    assert!(res.is_ok());
    assert!(
        fake.calls()
            .iter()
            .any(|c| c.contains(&"send-keys".to_string()))
    );
}

#[test]
fn test_capture_own_refuses_agent_panes() {
    let fake = std::sync::Arc::new(FakeRunner::new(HashMap::from([(
        "display-message",
        Result {
            code: 0,
            out: "agent\n".to_string(),
            err: String::new(),
        },
    )])));
    let client = create_client(fake);
    let res = client.capture_own("%1");
    assert_eq!(res, Err(ToSViolation::NotTandemOwned));
}

#[test]
fn test_capture_own_reads_own_panes() {
    let fake = std::sync::Arc::new(FakeRunner::new(HashMap::from([
        (
            "display-message",
            Result {
                code: 0,
                out: "own\n".to_string(),
                err: String::new(),
            },
        ),
        (
            "capture-pane",
            Result {
                code: 0,
                out: "hello".to_string(),
                err: String::new(),
            },
        ),
    ])));
    let client = create_client(fake);
    let res = client.capture_own("%2");
    assert_eq!(res.unwrap(), "hello");
}

#[test]
fn test_pane_id_must_look_like_a_pane_id() {
    let fake = std::sync::Arc::new(FakeRunner::new(HashMap::from([(
        "display-message",
        Result {
            code: 0,
            out: "own\n".to_string(),
            err: String::new(),
        },
    )])));
    let client = create_client(fake);
    let res = client.send_keys_own("%1; kill-server", &["Enter"]);
    assert_eq!(res, Err(ToSViolation::InvalidPaneId));
}

#[test]
fn test_each_workspace_gets_its_own_tmux_socket() {
    let temp = common::TempDir::new();
    let a = temp.path().join("a");
    let b = temp.path().join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();

    let sock_a = socket_for(&a);
    let sock_b = socket_for(&b);

    assert_ne!(sock_a, sock_b);
    assert_eq!(sock_a, socket_for(&a));
    assert_eq!(
        sock_a,
        socket_for(&temp.path().join("a").join("..").join("a"))
    );
    assert!(sock_a.starts_with("tandem-"));
    assert!(!sock_a.contains('/'));
}

#[test]
fn test_socket_for_parity_fixtures() {
    let fixture = load_fixture_value("tmux");
    let socket_cases = fixture["socket_for"].as_object().unwrap();

    for (path_str, expected_sock) in socket_cases {
        let actual_sock = socket_for(Path::new(path_str));
        assert_eq!(
            actual_sock,
            expected_sock.as_str().unwrap(),
            "socket_for({path_str})"
        );
    }
}
