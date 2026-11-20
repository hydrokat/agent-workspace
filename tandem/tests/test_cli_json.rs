use std::fs;
use std::path::PathBuf;
use tandem::cli::parse_from;
use tandem::commands::{answer_query_inner, collect_snapshot_json, list_ips_payload};
use tandem::config::Config;
use tandem::coordination::store::CoordStore;

fn temp_workspace(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tandem-test-cli-json-{name}-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(path.join(".agent-workspace/coordination/queries")).unwrap();
    fs::create_dir_all(path.join("specs")).unwrap();
    path
}

#[test]
fn test_collect_snapshot_json() {
    let ws = temp_workspace("snapshot");
    let config = Config::default();

    let payload = collect_snapshot_json(&ws, &config).expect("snapshot collection must succeed");
    assert!(payload.workspace_id.starts_with("tandem-"));
    assert_eq!(
        payload.workspace_path,
        ws.canonicalize().unwrap().to_string_lossy().to_string()
    );
    assert!(payload.snapshot.coord_exists);

    let json_str = serde_json::to_string_pretty(&payload).unwrap();
    assert!(json_str.contains("workspace_id"));
    assert!(json_str.contains("workspace_path"));
    assert!(json_str.contains("snapshot"));

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn test_answer_query_valid() {
    let ws = temp_workspace("answer-valid");
    let config = Config::default();
    let store = CoordStore::new(ws.join(".agent-workspace/coordination"));

    let q_content = r#"---
id: "q-abc12345"
status: "open"
task: "p-001 t-001"
question: "Choose an option?"
options: ["Option A", "Option B"]
created: 1700000000.0
---
"#;
    fs::write(
        ws.join(".agent-workspace/coordination/queries/q-abc12345.md"),
        q_content,
    )
    .unwrap();

    let result = answer_query_inner(&ws, &config, "q-abc12345", "Option A");
    assert!(result.is_ok(), "answer must succeed: {:?}", result.err());

    let updated = store.queries();
    let q = updated
        .iter()
        .find(|x| x.id == "q-abc12345")
        .expect("query must exist");
    assert_eq!(q.status, "answered");
    assert_eq!(q.answer, "Option A");
    assert_eq!(q.answered_via, "cli");

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn test_answer_query_malicious_path_traversal() {
    let ws = temp_workspace("answer-traversal");
    let config = Config::default();

    let malicious_ids = [
        "../q-traversal",
        "q-../../etc/passwd",
        "q-invalid/slash",
        "q-INVALID_CAPS",
        "invalid-prefix",
        "",
    ];

    for bad_id in malicious_ids {
        let result = answer_query_inner(&ws, &config, bad_id, "test");
        assert!(
            result.is_err(),
            "malicious or invalid id '{bad_id}' must be rejected"
        );
    }

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn test_answer_query_empty_answer() {
    let ws = temp_workspace("answer-empty");
    let config = Config::default();

    let result = answer_query_inner(&ws, &config, "q-abc12345", "   ");
    assert!(result.is_err(), "empty answer must be rejected");

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn test_snapshot_json_compact_by_default() {
    let ws = temp_workspace("compact");
    let config = Config::default();

    let payload = collect_snapshot_json(&ws, &config).expect("snapshot collection must succeed");

    let compact = serde_json::to_string(&payload).unwrap();
    let pretty = serde_json::to_string_pretty(&payload).unwrap();

    assert!(
        !compact.contains("  \""),
        "Compact output must not have leading two-space indentation"
    );
    assert!(
        pretty.contains("  \""),
        "Pretty output must have leading two-space indentation"
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_snapshot_json_pretty_flag() {
    let ws = temp_workspace("pretty-flag");
    let config = Config::default();

    let captured = {
        let payload = collect_snapshot_json(&ws, &config).expect("must succeed");
        serde_json::to_string_pretty(&payload).unwrap()
    };

    assert!(
        captured.contains("  \""),
        "Pretty-serialised output should have two-space indent"
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_snapshot_watch_interval_ms_too_low_rejected() {
    let result = parse_from(["tandem", "snapshot", "--watch", "--interval-ms", "100"]);
    assert!(
        result.is_err(),
        "--interval-ms 100 is below the minimum of 250 and must be rejected"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.exit_code(),
        2,
        "clap must exit with code 2 for invalid arg range"
    );
}

#[test]
fn test_list_ips_payload() {
    let ws = temp_workspace("list-ips");
    let payload = list_ips_payload(&ws);

    assert!(!payload.ips.is_empty(), "must discover at least localhost");
    assert!(
        payload.ips.iter().any(|c| c.ip == "127.0.0.1"),
        "must include 127.0.0.1"
    );

    let json_str = serde_json::to_string(&payload).unwrap();
    assert!(json_str.contains("\"ips\":"));
    assert!(json_str.contains("\"workspaces\":"));

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn test_snapshot_json_normal_mode_has_no_factory_key() {
    let ws = temp_workspace("no-factory");
    let fstore = tandem::factory::store::FactoryStore::new(&ws);
    let actor = tandem::factory::model::Actor {
        kind: tandem::factory::model::ActorKind::User,
        id: "you".to_string(),
    };
    fstore
        .create_idea("Should not be read", "body", "you", &actor)
        .unwrap();

    let config = Config::default();
    let payload = collect_snapshot_json(&ws, &config).expect("snapshot collection must succeed");
    assert!(payload.snapshot.factory.is_none());

    let val = serde_json::to_value(&payload).unwrap();
    assert!(
        val.get("snapshot").and_then(|s| s.get("factory")).is_none(),
        "normal-mode snapshot JSON must omit the factory key"
    );

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn test_snapshot_json_factory_mode_includes_factory_block() {
    let ws = temp_workspace("with-factory");
    let fstore = tandem::factory::store::FactoryStore::new(&ws);
    let actor = tandem::factory::model::Actor {
        kind: tandem::factory::model::ActorKind::User,
        id: "you".to_string(),
    };
    let created = fstore
        .create_idea("Export team activity as CSV", "details", "you", &actor)
        .unwrap();

    let config = Config {
        factory: true,
        ..Config::default()
    };
    let payload = collect_snapshot_json(&ws, &config).expect("snapshot collection must succeed");
    let factory = payload
        .snapshot
        .factory
        .as_ref()
        .expect("factory-mode snapshot must include factory block");
    assert!(factory.enabled);
    assert_eq!(factory.needs_you, 0);
    assert_eq!(factory.ideas.len(), 1);
    assert_eq!(factory.ideas[0].id, created.id);
    assert_eq!(factory.ideas[0].title, "Export team activity as CSV");
    assert_eq!(factory.ideas[0].owner, "you");
    assert_eq!(factory.ideas[0].blockers, 0);
    assert_eq!(factory.by_state.get("inbox").copied(), Some(1));
    assert!(factory.error.is_none());

    let val = serde_json::to_value(&payload).unwrap();
    assert!(
        val.get("snapshot").and_then(|s| s.get("factory")).is_some(),
        "factory-mode snapshot JSON must contain the factory key"
    );

    let _ = fs::remove_dir_all(ws);
}

fn collect_values_at_path<'a>(
    node: &'a serde_json::Value,
    segments: &[&str],
) -> Vec<&'a serde_json::Value> {
    if segments.is_empty() {
        return vec![node];
    }
    let head = segments[0];
    let rest = &segments[1..];
    if let Some(key) = head.strip_suffix("[]") {
        let Some(arr) = node.get(key).and_then(|v| v.as_array()) else {
            return vec![];
        };
        if arr.is_empty() {
            return vec![];
        }
        return arr
            .iter()
            .flat_map(|item| collect_values_at_path(item, rest))
            .collect();
    }
    let Some(next) = node.get(head) else {
        return vec![];
    };
    collect_values_at_path(next, rest)
}

#[test]
fn test_snapshot_payload_matches_contract_keys() {
    use tandem::commands::SnapshotPayload;
    use tandem::coordination::schema::{Claim, Session};
    use tandem::snapshot::{SprintPhaseView, SprintView, StatusSnapshot};
    use tandem::sources::ongoing::{OngoingSpec, OngoingTask};
    use tandem::sources::specs::{Phase, Task};
    use tandem::tmux::tabs::PaneInfo;

    let phase = Phase {
        name: "p-001-sample".to_string(),
        title: "Sample".to_string(),
        branch: "feat/sample".to_string(),
        target: "trunk".to_string(),
        tasks: vec![Task {
            id: "t-001".to_string(),
            title: "Task 1".to_string(),
            status: "In Progress".to_string(),
            objectives_done: 1,
            objectives_total: 2,
        }],
        warning: String::new(),
        archived: false,
    };

    let claim = Claim {
        phase: "p-001-sample".to_string(),
        task: "t-001".to_string(),
        session: "implementer-1".to_string(),
        branch: "feat/sample".to_string(),
        worktree: String::new(),
        paths: vec![],
        heartbeat: 1700000090.0,
    };

    let payload = SnapshotPayload {
        workspace_id: "tandem-sample01".to_string(),
        workspace_path: "/workspace/sample".to_string(),
        quota_line: "Claude 5h: 12%".to_string(),
        selected: vec!["%1".to_string()],
        workspace_name: Some("Sample".to_string()),
        window: None,
        panes: vec![PaneInfo {
            id: "%1".to_string(),
            window: "@1".to_string(),
            kind: "agent".to_string(),
            role: "implementer".to_string(),
            index: 1,
            label: "implementer-1".to_string(),
            name: "API worker".to_string(),
            dead: false,
            dead_status: None,
            session: "implementer-1".to_string(),
            spec: "p-001-sample".to_string(),
            cli_version: "1.0.0".to_string(),
            idea: String::new(),
            ..PaneInfo::default()
        }],
        snapshot: StatusSnapshot {
            coord_exists: true,
            now: 1700000100.0,
            phases: vec![phase.clone()],
            sessions: vec![Session {
                id: "implementer-1".to_string(),
                cli: "claude".to_string(),
                role: "implementer".to_string(),
                model: "sonnet-4".to_string(),
                pane: "%1".to_string(),
                cwd: "/workspace/sample".to_string(),
                branch: "feat/sample".to_string(),
                worktree: String::new(),
                task: "p-001-sample t-001".to_string(),
                heartbeat: 1700000090.0,
                name: "Session One".to_string(),
            }],
            claims: vec![claim.clone()],
            ongoing_specs: vec![OngoingSpec {
                phase,
                next_up: false,
            }],
            ongoing_tasks: vec![OngoingTask {
                phase: "p-001-sample".to_string(),
                task_id: "t-001".to_string(),
                title: "Task 1".to_string(),
                status: "In Progress".to_string(),
                objectives_done: 1,
                objectives_total: 2,
                claim: Some(claim),
                session_pane: Some("%1".to_string()),
                stale: false,
                status_mismatch: false,
                next_step: None,
            }],
            sprint: Some(SprintView {
                id: "s-001".to_string(),
                title: "Sample Sprint".to_string(),
                status: "locked".to_string(),
                start_date: "2026-11-01".to_string(),
                end_date: "2026-11-08".to_string(),
                capacity_days: 5.0,
                committed_days: 2.5,
                overloaded: false,
                done: 0,
                total: 1,
                must_remaining: 1,
                phases: vec![SprintPhaseView {
                    name: "p-001-sample".to_string(),
                    moscow: "Must Have".to_string(),
                    effort: 2.5,
                    done: 0,
                    total: 1,
                    archived: false,
                }],
            }),
            ..StatusSnapshot::default()
        },
    };

    let val = serde_json::to_value(&payload).unwrap();
    let keys_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../test/fixtures/snapshot-keys.txt");
    let contents = fs::read_to_string(&keys_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", keys_path.display()));

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key_path, expected_type) = line.split_once(':').expect("line must be path:type");
        let segments: Vec<&str> = key_path.split('.').collect();
        let values = collect_values_at_path(&val, &segments);
        assert!(
            !values.is_empty(),
            "Missing key path in Rust SnapshotPayload: {key_path}"
        );
        for v in values {
            let actual = match v {
                serde_json::Value::String(_) => "string",
                serde_json::Value::Number(_) => "number",
                serde_json::Value::Bool(_) => "boolean",
                serde_json::Value::Array(_) => "array",
                serde_json::Value::Object(_) => "object",
                serde_json::Value::Null => "null",
            };
            assert_eq!(actual, expected_type, "Unexpected JSON type for {key_path}");
        }
    }
}
