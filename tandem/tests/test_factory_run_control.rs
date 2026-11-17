use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tandem::config::Config;
use tandem::factory::approval::{mark_ready_with_workflow, revoke_ready};
use tandem::factory::artifacts;
use tandem::factory::broker::gate::{
    AuthType, check, consent_provider, revoke_provider, today_iso_date,
};
use tandem::factory::broker::job::{JobState, job_dir, read_job_state, set_job_state};
use tandem::factory::broker::spawn::{ProviderCommand, WaitOutcome, spawn_provider};
use tandem::factory::commands::{FactoryArgs, run_with_io};
use tandem::factory::events::read_all;
use tandem::factory::executor::{Executor, StageJob, StageOutcome};
use tandem::factory::machine::Command;
use tandem::factory::model::{Actor, ActorKind, IdeaState, RunState, StageState};
use tandem::factory::run::get_run;
use tandem::factory::runner::{
    RunnerError, cancel_idea, cancel_run, drive_run_step, pause_run_by_user, resume_paused_run,
};
use tandem::factory::store::FactoryStore;

fn temp_ws(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-run-control-{}-{}-{}",
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

fn user_actor() -> Actor {
    Actor {
        kind: ActorKind::User,
        id: "alice".to_string(),
    }
}

fn no_env(_: &str) -> Option<String> {
    None
}

fn setup_queued_run(ws: &Path) -> (FactoryStore, String, String) {
    init_git_repo(ws);
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();
    let user = user_actor();
    let orch = Actor {
        kind: ActorKind::Orchestrator,
        id: "orch".to_string(),
    };
    let idea = store
        .create_idea(
            "Run control test",
            "Test pause resume cancel",
            "alice",
            &user,
        )
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
    consent_provider(
        &store,
        "claude",
        AuthType::ApiKey,
        "claude",
        "alice",
        &no_env,
    )
    .unwrap();
    let ready = mark_ready_with_workflow(
        &store, ws, &idea.id, &idea.id, "cli", "alice", None, false, &no_env,
    )
    .unwrap();
    (store, idea.id, ready.run.unwrap().id)
}

struct HookExecutor<F: FnMut(&StageJob) -> StageOutcome> {
    calls: Arc<Mutex<Vec<(String, u32)>>>,
    hook: F,
}

impl<F: FnMut(&StageJob) -> StageOutcome> Executor for HookExecutor<F> {
    fn run(&mut self, job: &StageJob) -> StageOutcome {
        self.calls
            .lock()
            .unwrap()
            .push((job.stage.clone(), job.attempt));
        (self.hook)(job)
    }
}

#[test]
fn pause_at_stage_boundary_lets_running_stage_finish_and_starts_no_new_stage() {
    let ws = temp_ws("pause-boundary");
    let (store, _idea_id, run_id) = setup_queued_run(&ws);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let store_clone = store.clone();
    let mut exec = HookExecutor {
        calls: Arc::clone(&calls),
        hook: move |job| {
            if job.stage == "create-tests" {
                pause_run_by_user(&store_clone, &job.run, false, &user_actor(), &no_env).unwrap();
            }
            StageOutcome::Succeeded
        },
    };

    // Stage 1: plan
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    // Stage 2: create-tests (pauses mid-stage without --now, then finishes Succeeded)
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();

    let run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(run.state, RunState::Paused);
    assert_eq!(
        run.stages
            .iter()
            .find(|s| s.name == "create-tests")
            .unwrap()
            .state,
        StageState::Succeeded
    );

    // Subsequent step must not start any new stage while paused
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    let recorded = calls.lock().unwrap().clone();
    assert_eq!(
        recorded,
        vec![("plan".to_string(), 1), ("create-tests".to_string(), 1)]
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn pause_now_stops_running_child_process_and_interrupted_attempt_does_not_consume_retry() {
    let ws = temp_ws("pause-now");
    let (store, _idea_id, run_id) = setup_queued_run(&ws);

    // Test RunningJob::wait_with_stop stops a long-running child process promptly
    let worktrees_dir = ws.join("worktrees");
    fs::create_dir_all(&worktrees_dir).unwrap();
    let wt = worktrees_dir.join("stop-test");
    fs::create_dir_all(&wt).unwrap();
    let log_path = ws.join("child.log");
    let pass = check("claude", &store.settings().unwrap(), &today_iso_date()).unwrap();
    let mut parent_env = std::collections::BTreeMap::new();
    parent_env.insert(
        "PATH".to_string(),
        std::env::var("PATH").unwrap_or_default(),
    );
    // Create a fake `claude` script in temp bin so spawn_provider resolves it
    let bin_dir = ws.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let fake_claude = bin_dir.join("claude");
    fs::write(&fake_claude, "#!/bin/sh\nexec sleep 30\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_claude, fs::Permissions::from_mode(0o755)).unwrap();
    }
    parent_env.insert(
        "PATH".to_string(),
        format!(
            "{}:{}",
            bin_dir.display(),
            std::env::var("PATH").unwrap_or_default()
        ),
    );

    let mut job = spawn_provider(
        pass,
        ProviderCommand {
            program: "claude".to_string(),
            args: vec![],
            workspace_root: ws.clone(),
            cwd: wt.clone(),
            log_path,
            stdin_prompt: None,
            factory_env: std::collections::BTreeMap::new(),
            parent_env,
        },
    )
    .unwrap();

    let started = Instant::now();
    let outcome = job
        .wait_with_stop(
            Duration::from_secs(30),
            &|| started.elapsed() >= Duration::from_millis(80),
            Duration::from_millis(150),
        )
        .unwrap();
    assert!(matches!(outcome, WaitOutcome::Stopped(_)));
    assert!(started.elapsed() < Duration::from_secs(5));

    // Now test runner pause --now + resume + retry budget preservation
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut outcomes: HashMap<(String, u32), StageOutcome> = HashMap::new();
    let store_clone = store.clone();
    let mut exec = HookExecutor {
        calls: Arc::clone(&calls),
        hook: move |job| {
            if job.stage == "create-tests" && job.attempt == 1 {
                let j_dir = job_dir(&store_clone, &job.run, &job.stage, job.attempt);
                fs::create_dir_all(&j_dir).unwrap();
                set_job_state(&j_dir, JobState::Running).unwrap();
                let res = pause_run_by_user(&store_clone, &job.run, true, &user_actor(), &no_env)
                    .unwrap();
                assert_eq!(res.stopped_jobs.len(), 1);
                assert_eq!(read_job_state(&j_dir).unwrap(), JobState::Interrupted);
                return StageOutcome::Interrupted;
            }
            StageOutcome::Succeeded
        },
    };

    // Plan succeeds
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    // create-tests attempt 1 is interrupted by pause --now
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();

    let paused_run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(paused_run.state, RunState::Paused);
    let ct_stage = paused_run
        .stages
        .iter()
        .find(|s| s.name == "create-tests")
        .unwrap();
    assert_eq!(ct_stage.state, StageState::Pending);
    assert_eq!(ct_stage.interrupted_attempts, 1);

    // Resume and verify create-tests runs attempt 2, fails with hypothesis, and STILL has retry budget
    // (attempt 3 is allowed because attempt 1 was interrupted and did not consume a retry)
    outcomes.insert(
        ("create-tests".to_string(), 2),
        StageOutcome::Failed {
            reason: "hypothesis: fix test fixture".to_string(),
        },
    );
    let mut exec_retry = HookExecutor {
        calls: Arc::clone(&calls),
        hook: move |job| {
            outcomes
                .get(&(job.stage.clone(), job.attempt))
                .cloned()
                .unwrap_or(StageOutcome::Succeeded)
        },
    };
    resume_paused_run(&store, &run_id, &user_actor()).unwrap();
    // Attempt 2 fails with hypothesis -> stays Running (not paused for exhausted retries)
    drive_run_step(&store, &ws, &run_id, &mut exec_retry).unwrap();
    let mid_run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(mid_run.state, RunState::Running);
    // Attempt 3 succeeds
    drive_run_step(&store, &ws, &run_id, &mut exec_retry).unwrap();

    let recorded = calls.lock().unwrap().clone();
    assert_eq!(
        recorded,
        vec![
            ("plan".to_string(), 1),
            ("create-tests".to_string(), 1),
            ("create-tests".to_string(), 2),
            ("create-tests".to_string(), 3),
        ]
    );

    let after_resume = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(
        after_resume
            .stages
            .iter()
            .find(|s| s.name == "create-tests")
            .unwrap()
            .state,
        StageState::Succeeded
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn resume_verifies_plan_hash_workflow_hash_and_provider_gate() {
    let ws = temp_ws("resume-checks");
    let (store, idea_id, run_id) = setup_queued_run(&ws);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut exec = HookExecutor {
        calls: Arc::clone(&calls),
        hook: |_| StageOutcome::Succeeded,
    };

    // Run plan, then pause
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    pause_run_by_user(&store, &run_id, false, &user_actor(), &no_env).unwrap();

    // 1. Tamper with plan snapshot -> resume refused, run stays Paused
    let snap_path = ws
        .join(".agent-workspace/factory/ideas")
        .join(&idea_id)
        .join("artifacts/plan-snapshot.md");
    let orig_snap = fs::read(&snap_path).unwrap();
    fs::write(&snap_path, b"tampered plan").unwrap();
    let err = resume_paused_run(&store, &run_id, &user_actor()).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("plan snapshot"));
    assert_eq!(
        get_run(&store, &run_id).unwrap().unwrap().state,
        RunState::Paused
    );
    fs::write(&snap_path, &orig_snap).unwrap();

    // 2. Tamper with run's workflow.yaml copy -> resume refused, run stays Paused
    let wf_path = ws
        .join(".agent-workspace/factory/runs")
        .join(&run_id)
        .join("workflow.yaml");
    let orig_wf = fs::read(&wf_path).unwrap();
    fs::write(&wf_path, b"tampered: true\n").unwrap();
    let err = resume_paused_run(&store, &run_id, &user_actor()).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("workflow"));
    assert_eq!(
        get_run(&store, &run_id).unwrap().unwrap().state,
        RunState::Paused
    );
    fs::write(&wf_path, &orig_wf).unwrap();

    // 3. Revoke provider consent -> resume refused with gate closed message
    revoke_provider(&store, "claude", "alice", &no_env).unwrap();
    let err = resume_paused_run(&store, &run_id, &user_actor()).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("gate"));
    assert_eq!(
        get_run(&store, &run_id).unwrap().unwrap().state,
        RunState::Paused
    );

    // 4. Re-consent -> resume succeeds and continues from create-tests
    consent_provider(
        &store,
        "claude",
        AuthType::ApiKey,
        "claude",
        "alice",
        &no_env,
    )
    .unwrap();
    let resumed = resume_paused_run(&store, &run_id, &user_actor()).unwrap();
    assert_eq!(resumed.state, RunState::Running);

    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    let recorded = calls.lock().unwrap().clone();
    assert_eq!(
        recorded,
        vec![("plan".to_string(), 1), ("create-tests".to_string(), 1)]
    );

    let log = read_all(&ws.join(".agent-workspace/factory/events.jsonl")).unwrap();
    assert!(log.events.iter().any(|e| e.r#type == "run.paused"));
    assert!(log.events.iter().any(|e| e.r#type == "run.resumed"));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn cancel_run_stops_jobs_returns_idea_to_ready_keeps_worktree_and_allows_revoke_or_new_run() {
    let ws = temp_ws("cancel-run");
    let (store, idea_id, run_id) = setup_queued_run(&ws);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut exec = HookExecutor {
        calls: Arc::clone(&calls),
        hook: |_| StageOutcome::Succeeded,
    };

    // Drive plan so worktree is created and idea is InDevelopment
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    let run_before = get_run(&store, &run_id).unwrap().unwrap();
    let wt_path = PathBuf::from(run_before.worktree_path.as_ref().unwrap());
    assert!(wt_path.exists());

    // Confirmation mismatch must fail
    let err = cancel_run(&store, &run_id, "r-wrong", &user_actor(), &no_env).unwrap_err();
    assert!(err.to_string().contains("--confirm"));

    // Agent caller must fail
    let agent_env = |k: &str| {
        if k == "TANDEM_ROLE" {
            Some("orchestrator".to_string())
        } else {
            None
        }
    };
    let err = cancel_run(&store, &run_id, &run_id, &user_actor(), &agent_env).unwrap_err();
    assert!(matches!(err, RunnerError::Approval(_)));

    // Valid cancel succeeds, keeps worktree, returns idea to ReadyForDevelopment
    let res = cancel_run(&store, &run_id, &run_id, &user_actor(), &no_env).unwrap();
    assert_eq!(res.run.state, RunState::Cancelled);
    assert_eq!(res.idea.state, IdeaState::ReadyForDevelopment);
    assert!(wt_path.exists(), "worktree must be kept on cancel");

    // Cancelled run must never start another stage
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    assert_eq!(calls.lock().unwrap().len(), 1);

    // Idea can create a new run or revoke ready tag
    let revoked = revoke_ready(&store, &idea_id, "alice", &no_env).unwrap();
    assert_eq!(revoked.state, IdeaState::Planning);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn idea_cancel_cancels_idea_and_active_run_and_keeps_worktree() {
    let ws = temp_ws("cancel-idea");
    let (store, idea_id, run_id) = setup_queued_run(&ws);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut exec = HookExecutor {
        calls: Arc::clone(&calls),
        hook: |_| StageOutcome::Succeeded,
    };

    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    let run_before = get_run(&store, &run_id).unwrap().unwrap();
    let wt_path = PathBuf::from(run_before.worktree_path.as_ref().unwrap());
    assert!(wt_path.exists());

    // Mismatched confirm fails
    let err = cancel_idea(&store, &idea_id, "i-wrong", &user_actor(), &no_env).unwrap_err();
    assert!(err.to_string().contains("--confirm"));

    let res = cancel_idea(&store, &idea_id, &idea_id, &user_actor(), &no_env).unwrap();
    assert_eq!(res.idea.state, IdeaState::Cancelled);
    assert_eq!(
        get_run(&store, &run_id).unwrap().unwrap().state,
        RunState::Cancelled
    );
    assert!(wt_path.exists());

    let log = read_all(&ws.join(".agent-workspace/factory/events.jsonl")).unwrap();
    assert!(log.events.iter().any(|e| e.r#type == "run.cancelled"));
    assert!(log.events.iter().any(|e| e.r#type == "idea.cancelled"));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn cli_run_control_commands_work_and_enforce_factory_mode() {
    use clap::Parser;

    #[derive(Parser)]
    struct Wrapper {
        #[command(flatten)]
        args: FactoryArgs,
    }

    let ws = temp_ws("cli");
    let (store, _idea_id, run_id) = setup_queued_run(&ws);
    let mut exec = HookExecutor {
        calls: Arc::new(Mutex::new(Vec::new())),
        hook: |_| StageOutcome::Succeeded,
    };
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    let mut cfg = Config {
        state_dir: ws.join(".state"),
        ..Default::default()
    };

    // Without factory enabled -> exit 2
    let parsed = Wrapper::try_parse_from(["factory", "run", "pause", &run_id]).unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_io(
        &parsed.args,
        &ws,
        &cfg,
        &mut Cursor::new(b""),
        &mut out,
        &mut err,
    );
    assert_eq!(code, 2);

    // Enable factory mode
    cfg.factory = true;
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_io(
        &parsed.args,
        &ws,
        &cfg,
        &mut Cursor::new(b""),
        &mut out,
        &mut err,
    );
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    assert_eq!(
        get_run(&store, &run_id).unwrap().unwrap().state,
        RunState::Paused
    );

    // Resume via CLI
    let parsed = Wrapper::try_parse_from(["factory", "run", "resume", &run_id]).unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_io(
        &parsed.args,
        &ws,
        &cfg,
        &mut Cursor::new(b""),
        &mut out,
        &mut err,
    );
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    assert_eq!(
        get_run(&store, &run_id).unwrap().unwrap().state,
        RunState::Running
    );

    // Cancel via CLI
    let parsed =
        Wrapper::try_parse_from(["factory", "run", "cancel", &run_id, "--confirm", &run_id])
            .unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_io(
        &parsed.args,
        &ws,
        &cfg,
        &mut Cursor::new(b""),
        &mut out,
        &mut err,
    );
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    let stdout_str = String::from_utf8_lossy(&out);
    assert!(stdout_str.contains("Ready for development"));
    assert!(stdout_str.contains("run cleanup"));

    let _ = fs::remove_dir_all(&ws);
}
