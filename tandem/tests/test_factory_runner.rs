use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use tandem::factory::approval::mark_ready_with_workflow;
use tandem::factory::artifacts;
use tandem::factory::broker::gate::{AuthType, consent_provider};
use tandem::factory::events::read_all;
use tandem::factory::executor::{Executor, StageJob, StageOutcome};
use tandem::factory::machine::Command;
use tandem::factory::model::{Actor, ActorKind, IdeaState, RunState, StageState};
use tandem::factory::run::get_run;
use tandem::factory::runner::{
    RunnerError, acquire_runner_lock, drive_run_step, resume_paused_run, retry_paused_stage,
};
use tandem::factory::store::FactoryStore;

fn temp_ws(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-runner-{}-{}-{}",
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

fn init_git_repo(dir: &Path) {
    assert!(
        StdCommand::new("git")
            .args(["init", "-b", "trunk"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args(["config", "user.email", "t@example.com"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    fs::write(dir.join("README.md"), "# Test\n").unwrap();
    assert!(
        StdCommand::new("git")
            .args(["add", "README.md"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
}

fn setup_queued_run(ws: &Path) -> (FactoryStore, String, String) {
    init_git_repo(ws);
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();
    let user = Actor {
        kind: ActorKind::User,
        id: "alice".into(),
    };
    let orch = Actor {
        kind: ActorKind::Orchestrator,
        id: "orch".into(),
    };
    let idea = store
        .create_idea("Runner test", "Execute stages", "alice", &user)
        .unwrap();
    store
        .execute(&idea.id, Command::StartDiscovery, &orch)
        .unwrap();
    let feas = ws.join("f.md");
    fs::write(&feas, "# Feasible\n").unwrap();
    let feas_ref = artifacts::put(&store, &idea.id, "feasibility", &feas).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".into(),
                artifact: feas_ref,
                verdict: None,
                risk: None,
                workflow: None,
                missing_sections: vec![],
            },
            &orch,
        )
        .unwrap();
    store
        .execute(&idea.id, Command::SelectDirection, &orch)
        .unwrap();
    let plan = ws.join("p.md");
    fs::write(
        &plan,
        "# Plan\n\n## Goal\nRun\n\n## Acceptance Criteria\n- Works\n\n## Files\n- a\n\n## Tests\n- test\n",
    )
    .unwrap();
    let plan_ref = artifacts::put(&store, &idea.id, "plan", &plan).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".into(),
                artifact: plan_ref,
                verdict: None,
                risk: None,
                workflow: Some("default-delivery".into()),
                missing_sections: vec![],
            },
            &orch,
        )
        .unwrap();
    let ready = mark_ready_with_workflow(
        &store,
        ws,
        &idea.id,
        &idea.id,
        "cli",
        "alice",
        None,
        false,
        &|_| None,
    )
    .unwrap();
    (store, idea.id, ready.run.unwrap().id)
}

struct ScriptedExecutor {
    outcomes: HashMap<(String, u32), StageOutcome>,
    calls: Vec<(String, u32)>,
}

impl Executor for ScriptedExecutor {
    fn run(&mut self, job: &StageJob) -> StageOutcome {
        self.calls.push((job.stage.clone(), job.attempt));
        self.outcomes
            .get(&(job.stage.clone(), job.attempt))
            .cloned()
            .unwrap_or(StageOutcome::Succeeded)
    }
}

#[test]
fn runner_single_instance_lock_prevents_second_runner() {
    let ws = temp_ws("lock");
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();

    let _guard = acquire_runner_lock(&store).unwrap();
    let err = acquire_runner_lock(&store).unwrap_err();
    assert!(matches!(err, RunnerError::AlreadyRunning));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn runner_pauses_when_gate_closed_then_advances_retries_and_resumes_from_log() {
    let ws = temp_ws("loop");
    let (store, idea_id, run_id) = setup_queued_run(&ws);
    let mut exec = ScriptedExecutor {
        outcomes: HashMap::new(),
        calls: vec![],
    };

    // 1. Gate is closed -> run pauses with reason & resolution command
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    let run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(run.state, RunState::Paused);
    assert!(run.pause_reason.as_deref().unwrap().contains("gate"));
    assert!(exec.calls.is_empty());

    // 2. Open claude gate and resume/drive -> plan succeeds, idea moves to InDevelopment
    consent_provider(
        &store,
        "claude",
        AuthType::ApiKey,
        "claude",
        "alice",
        &|_| None,
    )
    .unwrap();
    let user = Actor {
        kind: ActorKind::User,
        id: "alice".into(),
    };
    resume_paused_run(&store, &run_id, &user).unwrap();
    exec.outcomes.insert(
        ("create-tests".into(), 1),
        StageOutcome::Failed {
            reason: "hypothesis: add missing fixture".into(),
        },
    );
    // Drive plan
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    assert_eq!(
        store.get_idea(&idea_id).unwrap().unwrap().state,
        IdeaState::InDevelopment
    );
    let run_after_plan = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(
        run_after_plan
            .stages
            .iter()
            .find(|s| s.name == "plan")
            .unwrap()
            .state,
        StageState::Succeeded
    );

    // 3. Drive create-tests attempt 1 (fails with new hypothesis), then attempt 2 (succeeds + model upgraded)
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    let log = read_all(&ws.join(".agent-workspace/factory/events.jsonl")).unwrap();
    assert!(
        log.events
            .iter()
            .any(|e| e.r#type == "stage.model_upgraded")
    );

    // 4. Restart recovery: plan was only called once
    assert_eq!(exec.calls.iter().filter(|(s, _)| s == "plan").count(), 1);

    // 5. Duplicate hypothesis exhausts retries and pauses; user retry grants another attempt
    exec.outcomes.insert(
        ("develop".into(), 1),
        StageOutcome::Failed {
            reason: "no structured result".into(),
        },
    );
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    let paused = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(paused.state, RunState::Paused);

    let user = Actor {
        kind: ActorKind::User,
        id: "alice".into(),
    };
    retry_paused_stage(
        &store,
        &run_id,
        "develop",
        "try smaller slice",
        &user,
        &|_| None,
    )
    .unwrap();
    let resumed = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(resumed.state, RunState::Running);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn retry_paused_stage_enforces_caller_gates_hashes_and_persists_hypothesis() {
    let ws = temp_ws("retry-sec");
    let (store, idea_id, run_id) = setup_queued_run(&ws);
    let mut exec = ScriptedExecutor {
        outcomes: HashMap::new(),
        calls: vec![],
    };

    // Gate closed -> run pauses
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    let user = Actor {
        kind: ActorKind::User,
        id: "alice".into(),
    };

    // Agent caller rejected via env and via actor kind
    let err_env = retry_paused_stage(&store, &run_id, "plan", "new hypothesis", &user, &|k| {
        if k == "TANDEM_ROLE" {
            Some("orchestrator".to_string())
        } else {
            None
        }
    })
    .unwrap_err();
    assert_eq!(err_env.code(), "agent_caller_forbidden");

    let orch = Actor {
        kind: ActorKind::Orchestrator,
        id: "orch".into(),
    };
    let err_actor = retry_paused_stage(&store, &run_id, "plan", "new hypothesis", &orch, &|_| None)
        .unwrap_err();
    assert_eq!(err_actor.code(), "agent_caller_forbidden");

    // Closed gate blocks retry
    let err_gate = retry_paused_stage(&store, &run_id, "plan", "new hypothesis", &user, &|_| None)
        .unwrap_err();
    assert_eq!(err_gate.code(), "gate_not_met");

    // Open gate, then tamper with plan snapshot -> hash mismatch blocks retry
    consent_provider(
        &store,
        "claude",
        AuthType::ApiKey,
        "claude",
        "alice",
        &|_| None,
    )
    .unwrap();
    let snap_path = store
        .root
        .join("ideas")
        .join(&idea_id)
        .join("artifacts")
        .join("plan-snapshot.md");
    let orig_snap = fs::read_to_string(&snap_path).unwrap();
    fs::write(&snap_path, "# Tampered\n").unwrap();
    let err_hash = retry_paused_stage(&store, &run_id, "plan", "new hypothesis", &user, &|_| None)
        .unwrap_err();
    assert_eq!(err_hash.code(), "gate_not_met");
    fs::write(&snap_path, orig_snap).unwrap();

    // Valid retry succeeds and persists hypothesis on stage + event
    let updated = retry_paused_stage(
        &store,
        &run_id,
        "plan",
        "  check edge case  ",
        &user,
        &|_| None,
    )
    .unwrap();
    assert_eq!(updated.state, RunState::Running);
    let stage = updated.stages.iter().find(|s| s.name == "plan").unwrap();
    assert_eq!(stage.retry_hypothesis.as_deref(), Some("check edge case"));
    let log = read_all(&ws.join(".agent-workspace/factory/events.jsonl")).unwrap();
    assert!(
        log.events
            .iter()
            .any(|e| e.r#type == "stage.retry_requested")
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn broker_executor_handles_closed_gate_and_idempotent_result_and_ui_slices() {
    let ws = temp_ws("broker-exec");
    let (store, _idea_id, run_id) = setup_queued_run(&ws);
    tandem::factory::workspace::prepare_worktree(&store, &ws, &run_id).unwrap();

    // Closed gate -> StageOutcome::Blocked
    let mut exec = tandem::factory::executor::BrokerExecutor {
        workspace: ws.clone(),
    };
    let outcome = exec.run(&StageJob {
        run: run_id.clone(),
        stage: "plan".to_string(),
        attempt: 1,
    });
    assert!(matches!(outcome, StageOutcome::Blocked { .. }));

    // Open gate and pre-populate valid job result + plan artifacts -> idempotent path verifies contract
    consent_provider(
        &store,
        "claude",
        AuthType::ApiKey,
        "claude",
        "alice",
        &|_| None,
    )
    .unwrap();
    let art_dir = store.root.join("runs").join(&run_id).join("artifacts");
    fs::create_dir_all(art_dir.join("specs/p-001-demo")).unwrap();
    fs::write(art_dir.join("specs/p-001-demo/impl.md"), "# Impl\n").unwrap();
    fs::write(
        art_dir.join("slices.json"),
        r#"{
  "phase": "p-001-demo",
  "slices": [
    {
      "id": "t-001",
      "title": "UI slice",
      "type": "New Feature",
      "paths": ["README.md"],
      "dependencies": [],
      "acceptance_criteria": ["Works"],
      "ui": true
    }
  ]
}"#,
    )
    .unwrap();
    assert!(tandem::factory::runner::has_ui_slices(&store, &run_id));

    let j_dir = tandem::factory::broker::job::job_dir(&store, &run_id, "plan", 1);
    fs::create_dir_all(&j_dir).unwrap();
    fs::write(
        j_dir.join("result.json"),
        r#"{
  "status": "succeeded",
  "summary": "Planned",
  "files_changed": [],
  "artifacts": ["specs/p-001-demo/impl.md", "slices.json"]
}"#,
    )
    .unwrap();

    let ok_outcome = exec.run(&StageJob {
        run: run_id.clone(),
        stage: "plan".to_string(),
        attempt: 1,
    });
    assert_eq!(ok_outcome, StageOutcome::Succeeded);

    let _ = fs::remove_dir_all(&ws);
}
