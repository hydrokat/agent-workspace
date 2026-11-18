mod common;

use common::TempDir;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;
use std::sync::{Arc, Barrier};
use std::thread;
use tandem::factory::events::{Event, append, read_all, read_since};
use tandem::factory::machine::Command;
use tandem::factory::model::{Actor, ActorKind, FactorySettings, IdeaState};
use tandem::factory::store::{FactoryError, FactoryStore};

fn user_actor() -> Actor {
    Actor {
        kind: ActorKind::User,
        id: "alice".to_string(),
    }
}

#[test]
fn test_create_list_get_permissions_and_gitignore() {
    let dir = TempDir::new();
    let store = FactoryStore::new(dir.path());
    assert!(!store.exists());

    store.ensure().expect("ensure succeeds");
    assert!(store.exists());

    let root = dir.path().join(".agent-workspace").join("factory");
    let ideas_dir = root.join("ideas");
    let gitignore = root.join(".gitignore");

    assert_eq!(
        fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&ideas_dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&gitignore).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(fs::read_to_string(&gitignore).unwrap().trim(), "*");

    let actor = user_actor();
    let first = store
        .create_idea("First idea", "Initial body", "alice", &actor)
        .expect("create first");
    std::thread::sleep(std::time::Duration::from_millis(5));
    let second = store
        .create_idea("Second idea", "Second body", "alice", &actor)
        .expect("create second");

    let first_file = ideas_dir.join(format!("{}.json", first.id));
    let events_file = root.join("events.jsonl");
    assert_eq!(
        fs::metadata(&first_file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&events_file).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let fetched = store
        .get_idea(&first.id)
        .expect("get_idea")
        .expect("idea exists");
    assert_eq!(fetched.id, first.id);
    assert_eq!(fetched.title, "First idea");
    assert_eq!(fetched.state, IdeaState::Inbox);

    let listed = store.list_ideas().expect("list_ideas");
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id, second.id);
    assert_eq!(listed[1].id, first.id);

    assert_eq!(
        store.settings().expect("settings"),
        FactorySettings::default()
    );
}

#[test]
fn test_concurrent_appends_produce_gap_free_seq() {
    let dir = TempDir::new();
    let store = FactoryStore::new(dir.path());
    store.ensure().expect("ensure");

    let root = dir.path().join(".agent-workspace").join("factory");
    let log_path = Arc::new(root.join("events.jsonl"));
    let lock_path = Arc::new(root.join(".lock"));

    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();

    for t in 0..2 {
        let log_path = Arc::clone(&log_path);
        let lock_path = Arc::clone(&lock_path);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            for i in 0..25 {
                let ev = Event {
                    v: 1,
                    seq: 0,
                    ts: 1000.0 + i as f64,
                    actor: user_actor(),
                    r#type: "idea.discovery_started".to_string(),
                    idea: format!("i-thread{t:02}{i:02}"),
                    run: None,
                    data: serde_json::json!({}),
                };
                let redacted = tandem::factory::events::redact_event(
                    ev,
                    &tandem::factory::redact::RedactContext::new(),
                );
                append(&log_path, &lock_path, &[redacted]).expect("append");
            }
        }));
    }

    for h in handles {
        h.join().expect("join");
    }

    let read = read_all(&log_path).expect("read_all");
    assert!(!read.truncated);
    assert_eq!(read.skipped, 0);
    assert_eq!(read.events.len(), 50);
    for (idx, ev) in read.events.iter().enumerate() {
        assert_eq!(ev.seq, (idx + 1) as u64);
        assert_eq!(ev.v, 1);
    }

    let since = read_since(&log_path, 40).expect("read_since");
    assert_eq!(since.events.len(), 10);
    assert_eq!(since.events[0].seq, 41);
}

#[test]
fn test_half_written_last_line_recovers_and_continues_seq() {
    let dir = TempDir::new();
    let store = FactoryStore::new(dir.path());
    store.ensure().expect("ensure");

    let root = dir.path().join(".agent-workspace").join("factory");
    let log_path = root.join("events.jsonl");
    let lock_path = root.join(".lock");

    let idea = store
        .create_idea("Recoverable", "Body", "alice", &user_actor())
        .expect("create");

    let mut file = OpenOptions::new().append(true).open(&log_path).unwrap();
    file.write_all(br#"{"v":1,"seq":2,"ts":1234.0,"actor":{"kind":"user""#)
        .unwrap();
    file.flush().unwrap();

    let read = read_all(&log_path).expect("read_all");
    assert!(read.truncated);
    assert_eq!(read.events.len(), 1);
    assert_eq!(read.events[0].seq, 1);

    let updated = store
        .execute(&idea.id, Command::StartDiscovery, &user_actor())
        .expect("execute after truncated tail");
    assert_eq!(updated.state, IdeaState::Ideation);

    let after = read_all(&log_path).expect("read_all after append");
    assert!(!after.truncated);
    assert_eq!(after.events.len(), 2);
    assert_eq!(after.events[0].seq, 1);
    assert_eq!(after.events[1].seq, 2);

    let ev = Event {
        v: 1,
        seq: 0,
        ts: 2000.0,
        actor: user_actor(),
        r#type: "idea.ready".to_string(),
        idea: idea.id.clone(),
        run: None,
        data: serde_json::json!({}),
    };
    let redacted =
        tandem::factory::events::redact_event(ev, &tandem::factory::redact::RedactContext::new());
    let appended = append(&log_path, &lock_path, &[redacted]).expect("append");
    assert_eq!(appended[0].seq, 3);
}

#[test]
fn test_rebuild_restores_deleted_materialized_files() {
    let dir = TempDir::new();
    let store = FactoryStore::new(dir.path());
    let actor = user_actor();

    let idea_a = store
        .create_idea("Idea A", "Body A", "alice", &actor)
        .expect("create A");
    let idea_b = store
        .create_idea("Idea B", "Body B", "alice", &actor)
        .expect("create B");
    store
        .execute(&idea_a.id, Command::StartDiscovery, &actor)
        .expect("start discovery A");

    let ideas_dir = dir
        .path()
        .join(".agent-workspace")
        .join("factory")
        .join("ideas");
    let path_a = ideas_dir.join(format!("{}.json", idea_a.id));
    let path_b = ideas_dir.join(format!("{}.json", idea_b.id));

    let original_a = fs::read_to_string(&path_a).unwrap();
    let original_b = fs::read_to_string(&path_b).unwrap();

    fs::remove_file(&path_a).unwrap();
    fs::remove_file(&path_b).unwrap();

    let report = store.rebuild().expect("rebuild");
    assert_eq!(report.ideas, 2);
    assert!(!report.truncated);

    assert_eq!(fs::read_to_string(&path_a).unwrap(), original_a);
    assert_eq!(fs::read_to_string(&path_b).unwrap(), original_b);
}

#[test]
fn test_corrupt_materialized_file_is_rebuilt_on_list() {
    let dir = TempDir::new();
    let store = FactoryStore::new(dir.path());
    let actor = user_actor();

    let idea = store
        .create_idea("Corruptible", "Body", "alice", &actor)
        .expect("create");
    let idea_path = dir
        .path()
        .join(".agent-workspace")
        .join("factory")
        .join("ideas")
        .join(format!("{}.json", idea.id));
    fs::write(&idea_path, "{corrupt json").unwrap();

    let listed = store.list_ideas().expect("list_ideas");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, idea.id);
    assert_eq!(listed[0].title, "Corruptible");
}

#[test]
fn test_symlinked_root_and_log_are_refused() {
    let dir = TempDir::new();
    let target = dir.path().join("outside");
    fs::create_dir_all(&target).unwrap();

    let aw = dir.path().join(".agent-workspace");
    fs::create_dir_all(&aw).unwrap();
    symlink(&target, aw.join("factory")).unwrap();

    let store = FactoryStore::new(dir.path());
    assert!(matches!(store.ensure(), Err(FactoryError::Symlink(_))));
    assert!(!store.exists());

    let dir2 = TempDir::new();
    let store2 = FactoryStore::new(dir2.path());
    store2.ensure().unwrap();
    let log_target = dir2.path().join("outside.jsonl");
    fs::write(&log_target, "").unwrap();
    symlink(
        &log_target,
        dir2.path()
            .join(".agent-workspace")
            .join("factory")
            .join("events.jsonl"),
    )
    .unwrap();

    assert!(matches!(store2.ensure(), Err(FactoryError::Symlink(_))));
    assert!(matches!(
        store2.create_idea("Title", "Text", "alice", &user_actor()),
        Err(FactoryError::Symlink(_))
    ));
}

#[test]
fn test_oversized_title_and_event_rejected_without_writing() {
    let dir = TempDir::new();
    let store = FactoryStore::new(dir.path());
    store.ensure().expect("ensure");

    let oversized_title = "x".repeat(121);
    let err = store
        .create_idea(&oversized_title, "Body", "alice", &user_actor())
        .unwrap_err();
    assert!(matches!(err, FactoryError::Transition(_)));

    let root = dir.path().join(".agent-workspace").join("factory");
    let log_path = root.join("events.jsonl");
    let lock_path = root.join(".lock");
    assert!(!log_path.exists());
    assert!(store.list_ideas().unwrap().is_empty());

    let huge_payload = "a".repeat(65 * 1024);
    let oversized_event = Event {
        v: 1,
        seq: 0,
        ts: 1000.0,
        actor: user_actor(),
        r#type: "idea.created".to_string(),
        idea: "i-12345678".to_string(),
        run: None,
        data: serde_json::json!({ "blob": huge_payload }),
    };

    let redacted = tandem::factory::events::redact_event(
        oversized_event,
        &tandem::factory::redact::RedactContext::new(),
    );
    let err = append(&log_path, &lock_path, &[redacted]).unwrap_err();
    assert!(matches!(err, FactoryError::EventTooLarge { .. }));
    assert!(!log_path.exists());
}
