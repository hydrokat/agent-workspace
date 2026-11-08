use std::fs;
use std::path::PathBuf;
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
