use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use tandem::config::Config;
use tandem::factory::approval::{mark_ready, revoke_ready};
use tandem::factory::artifacts;
use tandem::factory::commands::run_with_caller;
use tandem::factory::events::{Event, read_all};
use tandem::factory::machine::{Command, EventData, TransitionError};
use tandem::factory::model::{Actor, ActorKind, IdeaState, Risk, RunState, StageState};
use tandem::factory::run::{
    CreateRunOptions, RunError, create_run, get_run, list_runs, withdraw_run,
};
use tandem::factory::store::FactoryStore;
use tandem::factory::workflow::DEFAULT_WORKFLOW_YAML;

fn temp_ws(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-run-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn enable_factory_mode(ws: &Path) {
    fs::create_dir_all(ws.join(".agent-workspace")).unwrap();
    fs::write(ws.join(".agent-workspace").join("factory-mode"), "1\n").unwrap();
}

fn user_actor() -> Actor {
    Actor {
        kind: ActorKind::User,
        id: "user".to_string(),
    }
}

fn orch_actor() -> Actor {
    Actor {
        kind: ActorKind::Orchestrator,
        id: "orch-1".to_string(),
    }
}

fn append_events_raw(path: &Path, events: &[Event]) {
    let mut f = OpenOptions::new().append(true).open(path).unwrap();
    for ev in events {
        writeln!(f, "{}", serde_json::to_string(ev).unwrap()).unwrap();
    }
}

fn setup_ready_idea(ws: &Path) -> (FactoryStore, String) {
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();
    let user = user_actor();
    let orch = orch_actor();

    let idea = store
        .create_idea("Add CSV export", "Export runs as CSV", "user", &user)
        .unwrap();
    store
        .execute(&idea.id, Command::StartDiscovery, &orch)
        .unwrap();

    let feas_src = ws.join("feas.md");
    fs::write(&feas_src, "# Feasibility\n\nFeasible.\n").unwrap();
    let feas_ref = artifacts::put(&store, &idea.id, "feasibility", &feas_src).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: feas_ref,
                verdict: None,
                risk: Some(Risk::Low),
                workflow: None,
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();

    store
        .execute(&idea.id, Command::SelectDirection, &orch)
        .unwrap();

    let plan_src = ws.join("plan.md");
    fs::write(
        &plan_src,
        "# Plan\n\n## Goal\nExport CSV.\n\n## Acceptance Criteria\n- Valid CSV\n\n## Files\n- src/csv.rs\n\n## Tests\n- cargo test\n",
    )
    .unwrap();
    let plan_ref = artifacts::put(&store, &idea.id, "plan", &plan_src).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".to_string(),
                artifact: plan_ref,
                verdict: None,
                risk: None,
                workflow: Some("default-delivery".to_string()),
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();

    let caller = tandem::factory::approval::Caller::cli(true, &|_| None, &|_| None);
    mark_ready(&store, &idea.id, &idea.id, "user", &caller).unwrap();
    (store, idea.id)
}

#[test]
fn create_run_rejects_non_ready_tampered_snapshot_and_invalid_workflow_without_writing() {
    let ws = temp_ws("failures");
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();
    let user = user_actor();

    // 1. Missing idea
    let err = create_run(
        &store,
        &ws,
        "i-missing1",
        CreateRunOptions::default(),
        &user,
    )
    .unwrap_err();
    assert!(matches!(err, RunError::NotFound(_)));

    // 2. Idea in Inbox (not ReadyForDevelopment)
    let inbox = store
        .create_idea("Draft", "Not ready", "user", &user)
        .unwrap();
    let before_events = fs::read(ws.join(".agent-workspace/factory/events.jsonl")).unwrap();
    let err = create_run(&store, &ws, &inbox.id, CreateRunOptions::default(), &user).unwrap_err();
    assert!(matches!(err, RunError::NotReady(_)));
    assert_eq!(
        fs::read(ws.join(".agent-workspace/factory/events.jsonl")).unwrap(),
        before_events
    );
    assert!(!ws.join(".agent-workspace/factory/runs").exists());

    // 3. Ready idea with tampered plan-snapshot.md
    let (store2, ready_id) = setup_ready_idea(&ws);
    let snap_path = ws
        .join(".agent-workspace/factory/ideas")
        .join(&ready_id)
        .join("artifacts/plan-snapshot.md");
    let original_snap = fs::read(&snap_path).unwrap();
    fs::write(&snap_path, b"tampered plan").unwrap();

    let events_before_tamper = fs::read(ws.join(".agent-workspace/factory/events.jsonl")).unwrap();
    let err = create_run(&store2, &ws, &ready_id, CreateRunOptions::default(), &user).unwrap_err();
    assert!(matches!(err, RunError::SnapshotInvalid(_)));
    assert_eq!(
        fs::read(ws.join(".agent-workspace/factory/events.jsonl")).unwrap(),
        events_before_tamper
    );
    assert_eq!(
        store2.get_idea(&ready_id).unwrap().unwrap().state,
        IdeaState::ReadyForDevelopment
    );

    // Restore snapshot, but break workspace workflow file
    fs::write(&snap_path, original_snap).unwrap();
    fs::create_dir_all(ws.join("workflows")).unwrap();
    fs::write(ws.join("workflows/default-delivery.yaml"), "name: broken\n").unwrap();

    let err = create_run(&store2, &ws, &ready_id, CreateRunOptions::default(), &user).unwrap_err();
    assert!(matches!(err, RunError::WorkflowInvalid(_)));
    assert_eq!(
        fs::read(ws.join(".agent-workspace/factory/events.jsonl")).unwrap(),
        events_before_tamper
    );
    assert_eq!(
        store2.get_idea(&ready_id).unwrap().unwrap().state,
        IdeaState::ReadyForDevelopment
    );
    assert!(!ws.join(".agent-workspace/factory/runs").exists());

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn create_run_copies_workflow_records_event_and_moves_idea_to_queued() {
    let ws = temp_ws("success-embedded");
    let (store, idea_id) = setup_ready_idea(&ws);
    let user = user_actor();

    let run = create_run(
        &store,
        &ws,
        &idea_id,
        CreateRunOptions {
            deploy_requested: true,
            ..Default::default()
        },
        &user,
    )
    .unwrap();

    assert_eq!(run.idea, idea_id);
    assert_eq!(run.workflow, "default-delivery");
    assert_eq!(run.workflow_source, "embedded");
    assert_eq!(run.state, RunState::Queued);
    assert!(run.deploy_requested);
    assert!(!run.plan_sha256.is_empty());
    assert_eq!(run.stages.len(), 7);
    assert!(run.stages.iter().all(|s| s.state == StageState::Pending));

    let copied_wf = fs::read_to_string(
        ws.join(".agent-workspace/factory/runs")
            .join(&run.id)
            .join("workflow.yaml"),
    )
    .unwrap();
    assert_eq!(copied_wf, DEFAULT_WORKFLOW_YAML);

    let loaded_run = get_run(&store, &run.id).unwrap().unwrap();
    assert_eq!(loaded_run, run);

    let idea = store.get_idea(&idea_id).unwrap().unwrap();
    assert_eq!(idea.state, IdeaState::Queued);
    assert_eq!(idea.runs, vec![run.id.clone()]);

    // Also test file source when workflows/default-delivery.yaml exists
    let ws2 = temp_ws("success-file");
    fs::create_dir_all(ws2.join("workflows")).unwrap();
    fs::write(
        ws2.join("workflows/default-delivery.yaml"),
        DEFAULT_WORKFLOW_YAML,
    )
    .unwrap();
    let (store2, idea2) = setup_ready_idea(&ws2);
    let run2 = create_run(&store2, &ws2, &idea2, CreateRunOptions::default(), &user).unwrap();
    assert_eq!(run2.workflow_source, "file");
    assert!(!run2.deploy_requested);

    let _ = fs::remove_dir_all(&ws);
    let _ = fs::remove_dir_all(&ws2);
}

#[test]
fn stage_events_fold_and_rebuild_reconstructs_run_json() {
    let ws = temp_ws("fold-rebuild");
    let (store, idea_id) = setup_ready_idea(&ws);
    let user = user_actor();
    let sys = Actor {
        kind: ActorKind::System,
        id: "engine".to_string(),
    };

    let run = create_run(&store, &ws, &idea_id, CreateRunOptions::default(), &user).unwrap();

    let events_path = ws.join(".agent-workspace/factory/events.jsonl");
    let extra = vec![
        Event::from_data(
            100.0,
            sys.clone(),
            idea_id.clone(),
            Some(run.id.clone()),
            &EventData::RunStarted {
                run: run.id.clone(),
            },
        )
        .unwrap(),
        Event::from_data(
            101.0,
            sys.clone(),
            idea_id.clone(),
            Some(run.id.clone()),
            &EventData::StageStarted {
                run: run.id.clone(),
                stage: "plan".to_string(),
                attempt: 1,
                grant: None,
            },
        )
        .unwrap(),
        Event::from_data(
            102.0,
            sys.clone(),
            idea_id.clone(),
            Some(run.id.clone()),
            &EventData::StageSucceeded {
                run: run.id.clone(),
                stage: "plan".to_string(),
            },
        )
        .unwrap(),
        Event::from_data(
            103.0,
            sys.clone(),
            idea_id.clone(),
            Some(run.id.clone()),
            &EventData::StageFailed {
                run: run.id.clone(),
                stage: "create-tests".to_string(),
                reason: "flaky".to_string(),
            },
        )
        .unwrap(),
        Event::from_data(
            104.0,
            sys.clone(),
            idea_id.clone(),
            Some(run.id.clone()),
            &EventData::StageSkipped {
                run: run.id.clone(),
                stage: "demo".to_string(),
                reason: "ui_changed is false".to_string(),
            },
        )
        .unwrap(),
        Event::from_data(
            105.0,
            sys,
            idea_id.clone(),
            Some(run.id.clone()),
            &EventData::StageBlocked {
                run: run.id.clone(),
                stage: "deploy".to_string(),
                reason: "human_approval required".to_string(),
            },
        )
        .unwrap(),
    ];
    append_events_raw(&events_path, &extra);

    // Remove run.json and rebuild from log
    let run_json = ws
        .join(".agent-workspace/factory/runs")
        .join(&run.id)
        .join("run.json");
    fs::remove_file(&run_json).unwrap();

    let report = store.rebuild().unwrap();
    assert_eq!(report.ideas, 1);
    assert!(run_json.exists());

    let rebuilt = get_run(&store, &run.id).unwrap().unwrap();
    assert_eq!(rebuilt.state, RunState::Running);
    let plan = rebuilt.stages.iter().find(|s| s.name == "plan").unwrap();
    assert_eq!(plan.state, StageState::Succeeded);
    assert_eq!(plan.attempts, 1);
    let ct = rebuilt
        .stages
        .iter()
        .find(|s| s.name == "create-tests")
        .unwrap();
    assert_eq!(ct.state, StageState::Failed);
    assert_eq!(ct.rationale.as_deref(), Some("flaky"));
    let demo = rebuilt.stages.iter().find(|s| s.name == "demo").unwrap();
    assert_eq!(demo.state, StageState::Skipped);
    let deploy = rebuilt.stages.iter().find(|s| s.name == "deploy").unwrap();
    assert_eq!(deploy.state, StageState::Blocked);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn withdraw_run_is_user_only_requires_no_started_stage_and_allows_revoke() {
    let ws = temp_ws("withdraw");
    let (store, idea_id) = setup_ready_idea(&ws);
    let user = user_actor();
    let orch = orch_actor();

    let run = create_run(&store, &ws, &idea_id, CreateRunOptions::default(), &user).unwrap();

    // Orchestrator cannot withdraw
    let err = withdraw_run(&store, &run.id, &orch).unwrap_err();
    assert!(matches!(
        err,
        RunError::Transition(TransitionError::ActorNotAllowed { .. })
    ));

    // User can withdraw before any stage starts
    let withdrawn = withdraw_run(&store, &run.id, &user).unwrap();
    assert_eq!(withdrawn.state, RunState::Withdrawn);

    let idea = store.get_idea(&idea_id).unwrap().unwrap();
    assert_eq!(idea.state, IdeaState::ReadyForDevelopment);

    // User can now revoke ready tag
    let caller = tandem::factory::approval::Caller::cli(true, &|_| None, &|_| None);
    let revoked = revoke_ready(&store, &idea_id, "user", &caller).unwrap();
    assert_eq!(revoked.state, IdeaState::Planning);

    // Cannot withdraw if a stage already started
    mark_ready(&store, &idea_id, &idea_id, "user", &caller).unwrap();
    let run2 = create_run(&store, &ws, &idea_id, CreateRunOptions::default(), &user).unwrap();
    let sys = Actor {
        kind: ActorKind::System,
        id: "engine".to_string(),
    };
    append_events_raw(
        &ws.join(".agent-workspace/factory/events.jsonl"),
        &[Event::from_data(
            200.0,
            sys,
            idea_id.clone(),
            Some(run2.id.clone()),
            &EventData::StageStarted {
                run: run2.id.clone(),
                stage: "plan".to_string(),
                attempt: 1,
                grant: None,
            },
        )
        .unwrap()],
    );
    let err = withdraw_run(&store, &run2.id, &user).unwrap_err();
    assert!(matches!(err, RunError::StageAlreadyStarted(_)));

    let _ = fs::remove_dir_all(&ws);
}

fn call_factory(
    args: &[&str],
    ws: &Path,
    cfg: &Config,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
) -> i32 {
    let mut full = vec!["tandem"];
    full.extend_from_slice(args);
    let cli = tandem::cli::parse_from(full).unwrap();
    let tandem::cli::Command::Factory(f_args) = cli.command.unwrap() else {
        panic!("expected factory command");
    };
    let caller = tandem::factory::approval::Caller::cli(true, &|_| None, &|_| None);
    run_with_caller(&f_args, ws, cfg, &caller, stdout, stderr)
}

#[test]
fn cli_run_create_show_list_and_withdraw_work_and_gate_factory_mode() {
    let ws = temp_ws("cli");
    let (store, idea_id) = setup_ready_idea(&ws);
    let cfg = Config {
        state_dir: ws.join("state"),
        ..Default::default()
    };

    // Without factory mode, create fails
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = call_factory(
        &["factory", "run", "create", &idea_id, "--json"],
        &ws,
        &cfg,
        &mut stdout,
        &mut stderr,
    );
    assert_ne!(code, 0);

    enable_factory_mode(&ws);
    let cfg = Config {
        factory: true,
        state_dir: ws.join("state"),
        ..Default::default()
    };

    // Create with --deploy --json
    stdout.clear();
    stderr.clear();
    let code = call_factory(
        &["factory", "run", "create", &idea_id, "--deploy", "--json"],
        &ws,
        &cfg,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));
    let created: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    let run_id = created["id"].as_str().unwrap().to_string();
    assert_eq!(created["deploy_requested"], true);

    // Show --json
    stdout.clear();
    let code = call_factory(
        &["factory", "run", "show", &run_id, "--json"],
        &ws,
        &cfg,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, 0);
    let shown: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(shown["id"], run_id);

    // List --idea --json
    stdout.clear();
    let code = call_factory(
        &["factory", "run", "list", "--idea", &idea_id, "--json"],
        &ws,
        &cfg,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, 0);
    let listed: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(list_runs(&store, Some(&idea_id)).unwrap().len(), 1);

    // Withdraw --confirm
    stdout.clear();
    let code = call_factory(
        &[
            "factory",
            "run",
            "withdraw",
            &run_id,
            "--confirm",
            &run_id,
            "--json",
        ],
        &ws,
        &cfg,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));
    let withdrawn: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(withdrawn["state"], "withdrawn");

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn p013_and_p014_logs_still_replay() {
    let ws = temp_ws("compat-replay");
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();

    let events_path = ws.join(".agent-workspace/factory/events.jsonl");
    let log = concat!(
        r#"{"v":1,"seq":1,"ts":1000.0,"actor":{"kind":"user","id":"u1"},"type":"idea.created","idea":"i-p013aaaa","data":{"id":"i-p013aaaa","title":"Legacy p-013","text":"Old log","owner":"u1"}}"#,
        "\n",
        r#"{"v":1,"seq":2,"ts":1001.0,"actor":{"kind":"orchestrator","id":"o1"},"type":"idea.discovery_started","idea":"i-p013aaaa","data":{}}"#,
        "\n",
        r#"{"v":1,"seq":3,"ts":1002.0,"actor":{"kind":"orchestrator","id":"o1"},"type":"idea.direction_selected","idea":"i-p013aaaa","data":{}}"#,
        "\n",
        r#"{"v":1,"seq":4,"ts":1003.0,"actor":{"kind":"user","id":"u1"},"type":"idea.created","idea":"i-p014bbbb","data":{"id":"i-p014bbbb","title":"Legacy p-014","text":"Snapshot log","owner":"u1"}}"#,
        "\n",
        r#"{"v":1,"seq":5,"ts":1004.0,"actor":{"kind":"orchestrator","id":"o1"},"type":"idea.discovery_started","idea":"i-p014bbbb","data":{}}"#,
        "\n",
        r#"{"v":1,"seq":6,"ts":1005.0,"actor":{"kind":"orchestrator","id":"o1"},"type":"idea.direction_selected","idea":"i-p014bbbb","data":{}}"#,
        "\n",
        r#"{"v":1,"seq":7,"ts":1006.0,"actor":{"kind":"user","id":"u1"},"type":"idea.plan_snapshot_frozen","idea":"i-p014bbbb","data":{"plan_snapshot":{"path":"ideas/i-p014bbbb/artifacts/plan-snapshot.md","version":1,"sha256":"abc123","created":1006.0},"approved_by":"u1","via":"cli"}}"#,
        "\n",
        r#"{"v":1,"seq":8,"ts":1007.0,"actor":{"kind":"user","id":"u1"},"type":"idea.ready","idea":"i-p014bbbb","data":{}}"#,
        "\n",
        r#"{"v":1,"seq":9,"ts":1008.0,"actor":{"kind":"system","id":"sys"},"type":"run.created","idea":"i-p014bbbb","run":"r-p014cccc","data":{"run":"r-p014cccc"}}"#,
        "\n",
    );
    fs::write(&events_path, log).unwrap();

    let parsed = read_all(&events_path).unwrap();
    assert_eq!(parsed.events.len(), 9);
    assert_eq!(parsed.skipped, 0);

    let report = store.rebuild().unwrap();
    assert_eq!(report.ideas, 2);
    assert_eq!(report.skipped, 0);

    let p13 = store.get_idea("i-p013aaaa").unwrap().unwrap();
    assert_eq!(p13.state, IdeaState::Planning);
    let p14 = store.get_idea("i-p014bbbb").unwrap().unwrap();
    assert_eq!(p14.state, IdeaState::Queued);
    assert_eq!(p14.runs, vec!["r-p014cccc".to_string()]);

    let _ = fs::remove_dir_all(&ws);
}
