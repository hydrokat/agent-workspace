mod common;

use common::TempDir;
use std::fs;
use std::path::Path;
use std::process::Command;
use tandem::coordination::{CoordError, CoordStore, decode_query_fields, encode_query_fields};

#[test]
fn test_unknown_json_keys_survive_answer_rewrite() {
    let dir = TempDir::new();
    let store_root = dir.path().join("coordination");
    fs::create_dir_all(store_root.join("queries")).unwrap();
    let store = CoordStore::new(store_root.clone());

    let mut fields = serde_json::Map::new();
    fields.insert(
        "id".to_string(),
        serde_json::Value::String("q-1234".to_string()),
    );
    fields.insert(
        "status".to_string(),
        serde_json::Value::String("open".to_string()),
    );
    fields.insert(
        "custom_future_key".to_string(),
        serde_json::Value::String("custom_value".to_string()),
    );
    fields.insert(
        "custom_nested_object".to_string(),
        serde_json::json!({"level": 42}),
    );

    let query_file = store_root.join("queries").join("q-1234.md");
    fs::write(&query_file, encode_query_fields(&fields)).unwrap();

    store
        .answer("q-1234", "Approved answer", "tandem")
        .expect("answer succeeds");

    let updated_text = fs::read_to_string(&query_file).unwrap();
    let updated_fields = decode_query_fields(&updated_text).unwrap();

    assert_eq!(
        updated_fields["status"],
        serde_json::Value::String("answered".to_string())
    );
    assert_eq!(
        updated_fields["answer"],
        serde_json::Value::String("Approved answer".to_string())
    );
    assert_eq!(
        updated_fields["custom_future_key"],
        serde_json::Value::String("custom_value".to_string()),
        "Unknown future keys must survive answer rewrite"
    );
    assert_eq!(
        updated_fields["custom_nested_object"],
        serde_json::json!({"level": 42}),
        "Unknown nested future objects must survive answer rewrite"
    );
}

#[test]
fn test_invalid_query_ids_and_path_traversal_rejected() {
    let dir = TempDir::new();
    let store = CoordStore::new(dir.path().to_path_buf());

    let bad_ids = [
        "../q-1234",
        "q-12/34",
        "../../etc/passwd",
        "q-1",
        "not-a-q-id",
        "",
    ];

    for bad in bad_ids {
        assert!(
            matches!(
                store.answer(bad, "ans", "tandem"),
                Err(CoordError::InvalidInput(_))
            ),
            "Expected InvalidInput for bad query id: {}",
            bad
        );
        assert!(
            matches!(store.withdraw(bad), Err(CoordError::InvalidInput(_))),
            "Expected InvalidInput for bad query id on withdraw: {}",
            bad
        );
    }
}

#[test]
fn test_answer_conflict_error_when_already_answered() {
    let dir = TempDir::new();
    let store_root = dir.path().join("coordination");
    fs::create_dir_all(store_root.join("queries")).unwrap();
    let store = CoordStore::new(store_root.clone());

    let mut fields = serde_json::Map::new();
    fields.insert(
        "id".to_string(),
        serde_json::Value::String("q-1234".to_string()),
    );
    fields.insert(
        "status".to_string(),
        serde_json::Value::String("open".to_string()),
    );
    let query_file = store_root.join("queries").join("q-1234.md");
    fs::write(&query_file, encode_query_fields(&fields)).unwrap();

    store.answer("q-1234", "First answer", "tandem").unwrap();

    let second = store.answer("q-1234", "Second answer", "tandem");
    match second {
        Err(CoordError::AnswerConflict(status)) => assert_eq!(status, "answered"),
        other => panic!("Expected AnswerConflict, got {:?}", other),
    }
}

#[test]
fn test_coord_py_interop_round_trip() {
    let python = match Command::new("python3").arg("--version").output() {
        Ok(out) if out.status.success() => "python3",
        _ => return,
    };

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let coord_py = manifest_dir
        .parent()
        .unwrap()
        .join("templates/skills/agent-coordination/scripts/coord.py");
    if !coord_py.exists() {
        return;
    }

    let dir = TempDir::new();
    let coord_dir = dir.path().join("coordination");
    fs::create_dir_all(&coord_dir).unwrap();

    let run_coord = |args: &[&str]| -> (i32, String, String) {
        let out = Command::new(python)
            .arg(&coord_py)
            .args(args)
            .env("TANDEM_COORD_DIR", &coord_dir)
            .output()
            .expect("run coord.py");
        (
            out.status.code().unwrap_or(1),
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    };

    let (reg_code, _, reg_err) =
        run_coord(&["--session", "s-interop", "register", "--model", "sonnet"]);
    assert_eq!(reg_code, 0, "coord.py register failed: {}", reg_err);

    let (claim_code, claim_out, claim_err) = run_coord(&[
        "--session",
        "s-interop",
        "claim",
        "p-001-tandem",
        "t-001",
        "--branch",
        "feat/x",
        "--worktree",
        "/w",
        "--paths",
        "src/main.rs",
    ]);
    assert_eq!(
        claim_code, 0,
        "coord.py claim failed: stdout={}, stderr={}",
        claim_out, claim_err
    );

    let (ask_code, ask_out, ask_err) = run_coord(&[
        "--session",
        "s-interop",
        "ask",
        "Should we use Rust?",
        "--blocking",
        "src/main.rs",
        "--task",
        "p-001-tandem t-001",
    ]);
    assert_eq!(
        ask_code, 0,
        "coord.py ask failed: stdout={}, stderr={}",
        ask_out, ask_err
    );
    let ask_json: serde_json::Value = serde_json::from_str(&ask_out).expect("parse ask json");
    let qid = ask_json["id"].as_str().expect("query id");

    let store = CoordStore::new(coord_dir.clone());
    let queries = store.queries();
    assert_eq!(queries.len(), 1);
    assert_eq!(queries[0].id, qid);
    assert_eq!(queries[0].status, "open");
    assert_eq!(queries[0].question, "Should we use Rust?");

    store
        .answer(qid, "Yes, absolutely", "tandem")
        .expect("Rust store answer");

    let (ans_code, ans_out, _) = run_coord(&["--session", "s-interop", "answers", "--mine"]);
    assert_eq!(ans_code, 0, "coord.py answers failed");
    let ans_json: serde_json::Value = serde_json::from_str(&ans_out).expect("parse answers json");
    let answers = ans_json["answers"].as_array().expect("answers array");
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0]["id"], qid);
    assert_eq!(answers[0]["answer"], "Yes, absolutely");
    assert_eq!(answers[0]["status"], "answered");
}
