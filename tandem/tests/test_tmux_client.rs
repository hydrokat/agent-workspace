mod common;

use common::parity::load_fixture_value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tandem::tmux::client::{
    PaneKind, Result, TmuxClient, TmuxError, TmuxRunner, ToSViolation, socket_for,
};

struct FakeRunner {
    calls: Mutex<Vec<Vec<String>>>,
    stdin_calls: Mutex<Vec<(Vec<String>, Option<String>)>>,
    responses: HashMap<String, Result>,
}

impl FakeRunner {
    fn new(responses: HashMap<&str, Result>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            stdin_calls: Mutex::new(Vec::new()),
            responses: responses
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }

    fn stdin_calls(&self) -> Vec<(Vec<String>, Option<String>)> {
        self.stdin_calls.lock().unwrap().clone()
    }
}

impl TmuxRunner for FakeRunner {
    fn run(&self, argv: &[String]) -> Result {
        self.calls.lock().unwrap().push(argv.to_vec());
        self.stdin_calls.lock().unwrap().push((argv.to_vec(), None));
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

    fn run_with_stdin(&self, argv: &[String], stdin: &str) -> Result {
        self.calls.lock().unwrap().push(argv.to_vec());
        self.stdin_calls
            .lock()
            .unwrap()
            .push((argv.to_vec(), Some(stdin.to_string())));
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

#[test]
fn test_run_batch_empty() {
    let fake = std::sync::Arc::new(FakeRunner::new(HashMap::new()));
    let client = create_client(fake.clone());
    let res = client.run_batch(&[]).unwrap();
    assert_eq!(res.code, 0);
    assert_eq!(fake.calls().len(), 0);
}

#[test]
fn test_run_batch_joins_with_semicolon() {
    let fake = std::sync::Arc::new(FakeRunner::new(HashMap::new()));
    let client = create_client(fake.clone());
    let cmd1 = vec![
        "set-option".to_string(),
        "-g".to_string(),
        "status".to_string(),
        "on".to_string(),
    ];
    let cmd2 = vec![
        "set-option".to_string(),
        "-g".to_string(),
        "mouse".to_string(),
        "on".to_string(),
    ];
    let res = client.run_batch(&[cmd1, cmd2]).unwrap();
    assert_eq!(res.code, 0);
    assert_eq!(fake.calls().len(), 1);
    let call = &fake.calls()[0];
    assert_eq!(
        call,
        &[
            "tmux",
            "-L",
            "tandem-test",
            "-f",
            "/c/tandem.conf",
            "set-option",
            "-g",
            "status",
            "on",
            ";",
            "set-option",
            "-g",
            "mouse",
            "on",
        ]
    );
}

#[test]
fn test_run_batch_enforces_allow_list() {
    let fake = std::sync::Arc::new(FakeRunner::new(HashMap::new()));
    let client = create_client(fake.clone());
    let safe = vec![
        "set-option".to_string(),
        "-g".to_string(),
        "status".to_string(),
        "on".to_string(),
    ];
    let unsafe_cmd = vec![
        "send-keys".to_string(),
        "-t".to_string(),
        "%1".to_string(),
        "rm -rf /".to_string(),
    ];
    let res = client.run_batch(&[safe, unsafe_cmd]);
    assert_eq!(
        res,
        Err(ToSViolation::SubcommandNotAllowed("send-keys".to_string()))
    );
    assert_eq!(fake.calls().len(), 0);
}

#[test]
fn test_pane_kind_parses_factory_worker_login_and_unknown() {
    assert_eq!(PaneKind::from_tag("own"), PaneKind::Own);
    assert_eq!(PaneKind::from_tag("agent"), PaneKind::Agent);
    assert_eq!(
        PaneKind::from_tag("factory-worker"),
        PaneKind::FactoryWorker
    );
    assert_eq!(PaneKind::from_tag("login"), PaneKind::Login);
    assert_eq!(PaneKind::from_tag(""), PaneKind::Other(String::new()));
    assert_eq!(
        PaneKind::from_tag("user"),
        PaneKind::Other("user".to_string())
    );
}

#[test]
fn test_send_keys_worker_allowed_for_matching_worker_and_run() {
    let fake = std::sync::Arc::new(FakeRunner::new(HashMap::from([(
        "display-message",
        Result {
            code: 0,
            out: "factory-worker\tr-abc\n".to_string(),
            err: String::new(),
        },
    )])));
    let client = create_client(fake.clone());
    let res = client.send_keys_worker("r-abc", "%11", &["Enter"]);
    assert!(res.is_ok(), "unexpected error: {res:?}");
    let calls = fake.calls();
    assert_eq!(calls.len(), 2);
    assert!(calls[1].ends_with(&[
        "send-keys".to_string(),
        "-t".to_string(),
        "%11".to_string(),
        "Enter".to_string()
    ]));
}

#[test]
fn test_worker_helpers_fail_closed_on_wrong_or_missing_run() {
    for output in [
        "factory-worker\tr-other\n",
        "factory-worker\t\n",
        "factory-worker\n",
    ] {
        let fake = std::sync::Arc::new(FakeRunner::new(HashMap::from([(
            "display-message",
            Result {
                code: 0,
                out: output.to_string(),
                err: String::new(),
            },
        )])));
        let client = create_client(fake.clone());

        let r1 = client.send_keys_worker("r-abc", "%11", &["Enter"]);
        assert!(
            matches!(r1, Err(TmuxError::ToS(ToSViolation::RunMismatch))),
            "send_keys_worker for {output:?}: {r1:?}"
        );
        let r2 = client.paste_worker("r-abc", "%11", "read prompt.md");
        assert!(
            matches!(r2, Err(TmuxError::ToS(ToSViolation::RunMismatch))),
            "paste_worker for {output:?}: {r2:?}"
        );
        let r3 = client.capture_worker("r-abc", "%11", 50);
        assert!(
            matches!(r3, Err(TmuxError::ToS(ToSViolation::RunMismatch))),
            "capture_worker for {output:?}: {r3:?}"
        );

        assert!(
            fake.calls()
                .iter()
                .all(|c| c.iter().any(|a| a == "display-message")),
            "must not invoke send-keys/load-buffer/paste-buffer/capture-pane on run mismatch"
        );
    }
}

#[test]
fn test_worker_helpers_refuse_login_own_agent_user_and_untagged_panes() {
    let cases = [
        ("login\t\n", ToSViolation::LoginPaneProtected),
        ("login\tr-abc\n", ToSViolation::LoginPaneProtected),
        ("own\t\n", ToSViolation::NotFactoryWorker),
        ("agent\t\n", ToSViolation::NotFactoryWorker),
        ("user\t\n", ToSViolation::NotFactoryWorker),
        ("\t\n", ToSViolation::NotFactoryWorker),
    ];

    for (output, expected) in cases {
        let fake = std::sync::Arc::new(FakeRunner::new(HashMap::from([(
            "display-message",
            Result {
                code: 0,
                out: output.to_string(),
                err: String::new(),
            },
        )])));
        let client = create_client(fake.clone());

        assert_eq!(
            client
                .send_keys_worker("r-abc", "%11", &["Enter"])
                .unwrap_err()
                .to_string(),
            TmuxError::ToS(expected.clone()).to_string()
        );
        assert_eq!(
            client
                .paste_worker("r-abc", "%11", "secret")
                .unwrap_err()
                .to_string(),
            TmuxError::ToS(expected.clone()).to_string()
        );
        assert_eq!(
            client
                .capture_worker("r-abc", "%11", 50)
                .unwrap_err()
                .to_string(),
            TmuxError::ToS(expected).to_string()
        );
        assert_eq!(fake.calls().len(), 3);
        assert!(
            fake.calls()
                .iter()
                .all(|c| c.iter().any(|a| a == "display-message"))
        );
    }
}

#[test]
fn test_own_helpers_refuse_login_and_factory_worker_panes() {
    for kind in ["login\n", "factory-worker\n"] {
        let fake = std::sync::Arc::new(FakeRunner::new(HashMap::from([(
            "display-message",
            Result {
                code: 0,
                out: kind.to_string(),
                err: String::new(),
            },
        )])));
        let client = create_client(fake.clone());
        assert_eq!(
            client.send_keys_own("%5", &["Enter"]),
            Err(ToSViolation::NotTandemOwned)
        );
        assert_eq!(client.capture_own("%5"), Err(ToSViolation::NotTandemOwned));
        assert!(
            fake.calls()
                .iter()
                .all(|c| c.iter().any(|a| a == "display-message"))
        );
    }
}

#[test]
fn test_paste_worker_uses_named_buffer_stdin_delete_and_enter() {
    let fake = std::sync::Arc::new(FakeRunner::new(HashMap::from([(
        "display-message",
        Result {
            code: 0,
            out: "factory-worker\tr-abc\n".to_string(),
            err: String::new(),
        },
    )])));
    let client = create_client(fake.clone());
    let prompt = "Read .agent-workspace/factory/runs/r-abc/jobs/j-1/prompt.md\nand follow it.";
    client.paste_worker("r-abc", "%11", prompt).unwrap();

    let calls = fake.stdin_calls();
    assert_eq!(calls.len(), 4);

    assert!(calls[0].0.iter().any(|a| a == "display-message"));
    assert_eq!(calls[0].1, None);

    let (load_argv, load_stdin) = &calls[1];
    let load_idx = load_argv.iter().position(|a| a == "load-buffer").unwrap();
    assert_eq!(load_argv[load_idx + 1], "-b");
    let buf_name = &load_argv[load_idx + 2];
    assert!(buf_name.starts_with("tandem-w-"));
    assert_eq!(load_argv[load_idx + 3], "-");
    assert_eq!(load_stdin.as_deref(), Some(prompt));

    let (paste_argv, paste_stdin) = &calls[2];
    let paste_idx = paste_argv.iter().position(|a| a == "paste-buffer").unwrap();
    assert_eq!(
        &paste_argv[paste_idx..],
        &[
            "paste-buffer".to_string(),
            "-d".to_string(),
            "-b".to_string(),
            buf_name.clone(),
            "-t".to_string(),
            "%11".to_string(),
        ]
    );
    assert_eq!(*paste_stdin, None);

    let (enter_argv, _) = &calls[3];
    assert!(enter_argv.ends_with(&[
        "send-keys".to_string(),
        "-t".to_string(),
        "%11".to_string(),
        "Enter".to_string(),
    ]));
}

#[test]
fn test_capture_worker_passes_lines_and_validates_pane_id_and_run_id() {
    let fake = std::sync::Arc::new(FakeRunner::new(HashMap::from([
        (
            "display-message",
            Result {
                code: 0,
                out: "factory-worker\tr-abc\n".to_string(),
                err: String::new(),
            },
        ),
        (
            "capture-pane",
            Result {
                code: 0,
                out: "worker output\n".to_string(),
                err: String::new(),
            },
        ),
    ])));
    let client = create_client(fake.clone());
    let out = client.capture_worker("r-abc", "%11", 80).unwrap();
    assert_eq!(out, "worker output\n");
    let cap_call = &fake.calls()[1];
    assert!(cap_call.ends_with(&[
        "capture-pane".to_string(),
        "-p".to_string(),
        "-S".to_string(),
        "-80".to_string(),
        "-t".to_string(),
        "%11".to_string(),
    ]));

    assert!(matches!(
        client.capture_worker("r-abc", "%11; bad", 10),
        Err(TmuxError::ToS(ToSViolation::InvalidPaneId))
    ));
    assert!(matches!(
        client.capture_worker("", "%11", 10),
        Err(TmuxError::ToS(ToSViolation::RunMismatch))
    ));
}
