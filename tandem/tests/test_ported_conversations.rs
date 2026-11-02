mod common;

use common::TempDir;
use tandem::conversations::{
    agy_log_path, can_resume, claude_transcript_exists, current, forget, from_agy_log_with_tail,
    new_id, record, recorded, valid_id,
};

const ID: &str = "46f2fc12-7f1f-4ca2-b714-06cbbfd669e9";
const OTHER: &str = "0b9a3c1e-1111-4222-8333-444455556666";

#[test]
fn test_valid_id_accepts_uuids_only() {
    assert_eq!(valid_id(&ID.to_uppercase()), Some(ID.to_string()));
    for bad in ["", "abc", &format!("{ID}x"), &format!("../{ID}")] {
        assert!(valid_id(bad).is_none());
    }
    assert!(valid_id(&new_id()).is_some());
}

#[test]
fn test_record_round_trip_is_owner_only() {
    let tmp = TempDir::new();
    record(tmp.path(), "planner-1", ID).unwrap();

    let path = tmp.path().join("conversations").join("planner-1.json");
    assert_eq!(recorded(tmp.path(), "planner-1"), Some(ID.to_string()));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(&path).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }

    let parsed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let map = parsed.as_object().unwrap();
    assert!(map.contains_key("id"));
    assert!(map.contains_key("updated"));
}

#[test]
fn test_session_ids_are_checked() {
    let tmp = TempDir::new();
    for session in ["../x", "planner", "Planner-1", "planner-100"] {
        assert!(record(tmp.path(), session, ID).is_err());
        assert!(current(tmp.path(), session, "claude").is_none());
    }
}

#[test]
fn test_invalid_conversation_is_never_recorded() {
    let tmp = TempDir::new();
    assert!(record(tmp.path(), "planner-1", "not-a-uuid").is_err());
}

#[test]
fn test_agy_log_gives_the_latest_conversation() {
    let tmp = TempDir::new();
    let log = agy_log_path(tmp.path(), "implementer-2").unwrap();
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(
        &log,
        format!(
            "I1017 server.go:1199] Created conversation {ID} (backend=X)\n\
             I1017 conversation_manager.go:973] Streaming conversation {OTHER}\n\
             I1017 something else\n"
        ),
    )
    .unwrap();

    assert_eq!(
        current(tmp.path(), "implementer-2", "agy"),
        Some(OTHER.to_string())
    );
}

#[test]
fn test_agy_log_only_reads_the_tail() {
    let tmp = TempDir::new();
    let log = agy_log_path(tmp.path(), "planner-1").unwrap();
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    let payload = format!("Created conversation {ID}\n{}\n", "x".repeat(400));
    std::fs::write(&log, payload).unwrap();

    assert!(from_agy_log_with_tail(&log, 200).is_none());
}

#[test]
fn test_missing_sources_mean_unknown() {
    let tmp = TempDir::new();
    assert!(current(tmp.path(), "planner-1", "claude").is_none());
    assert!(current(tmp.path(), "planner-1", "agy").is_none());
    assert!(current(tmp.path(), "planner-1", "other").is_none());
}

#[test]
fn test_forget_removes_record_and_log() {
    let tmp = TempDir::new();
    record(tmp.path(), "planner-1", ID).unwrap();
    let log = agy_log_path(tmp.path(), "planner-1").unwrap();
    std::fs::write(&log, format!("Created conversation {ID}\n")).unwrap();

    forget(tmp.path(), "planner-1");
    let conv_dir = tmp.path().join("conversations");
    let entries: Vec<_> = std::fs::read_dir(conv_dir)
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert!(entries.is_empty());

    let missing = tmp.path().join("missing");
    forget(&missing, "planner-1");
}

#[test]
fn test_claude_resume_needs_a_saved_transcript() {
    let tmp = TempDir::new();
    assert!(!can_resume("claude", Some(ID), tmp.path()));

    let proj = tmp.path().join("projects").join("-Users-me-ws");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(proj.join(format!("{ID}.jsonl")), "{}\n").unwrap();

    assert!(can_resume("claude", Some(ID), tmp.path()));
    assert!(!can_resume("claude", Some(OTHER), tmp.path()));
    assert!(!can_resume("claude", None, tmp.path()));
}

#[test]
fn test_claude_transcript_check_only_stats_paths_never_opens() {
    let tmp = TempDir::new();
    let proj = tmp.path().join("projects").join("-Users-me-ws");
    std::fs::create_dir_all(&proj).unwrap();
    let file_path = proj.join(format!("{ID}.jsonl"));
    std::fs::write(&file_path, "sample transcript\n").unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file_path, std::fs::Permissions::from_mode(0o000)).unwrap();
    }

    assert!(
        claude_transcript_exists(tmp.path(), ID),
        "transcript exists check should succeed by stat alone without opening"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

#[test]
fn test_agy_resumes_any_known_conversation() {
    let tmp = TempDir::new();
    assert!(can_resume("agy", Some(ID), tmp.path()));
    assert!(!can_resume("agy", None, tmp.path()));
    assert!(!can_resume("agy", Some("bad"), tmp.path()));
}
