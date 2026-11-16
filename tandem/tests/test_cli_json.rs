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
