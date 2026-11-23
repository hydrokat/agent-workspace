use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::sync::{Arc, Mutex};

use tandem::config::Config;
use tandem::factory::approval::{mark_ready_with_workflow, revoke_ready};
use tandem::factory::artifacts;
use tandem::factory::broker::job::{JobState, job_dir, read_job_state, set_job_state};
use tandem::factory::commands::{FactoryArgs, run_with_io};
use tandem::factory::events::read_all;
use tandem::factory::executor::{Executor, StageJob, StageOutcome};
use tandem::factory::login::LoginProbe;
use tandem::factory::machine::Command;
use tandem::factory::model::{Actor, ActorKind, IdeaState, RunState, StageState};
use tandem::factory::run::get_run;
use tandem::factory::runner::{
    RunnerError, cancel_idea, cancel_run, drive_run_step, pause_run_by_user,
    resume_paused_run_with_probe,
};
use tandem::factory::store::FactoryStore;

struct StaticLoginProbe(bool);
impl LoginProbe for StaticLoginProbe {
    fn which(&self, bin: &str) -> Option<PathBuf> {
        (self.0 && bin == "claude").then(|| PathBuf::from("/usr/local/bin/claude"))
    }
    fn version(&self, _cli: &str) -> String {
        "claude 2.0".to_string()
    }
    fn run(&self, _argv: &[&str]) -> (i32, String, String) {
        if self.0 {
            (
                0,
                r#"{"loggedIn":true,"authMethod":"apiKey"}"#.to_string(),
                String::new(),
            )
        } else {
            (1, String::new(), "not signed in".to_string())
        }
    }
}

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
    let mut settings = store.settings().unwrap();
    settings.approval_grace_secs = 0;
    tandem::factory::settings::save(&store, &settings).unwrap();
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
    let caller = tandem::factory::approval::Caller::cli(true, &no_env, &|_| None);
    let ready = mark_ready_with_workflow(
        &store, ws, &idea.id, &idea.id, "alice", None, false, &caller,
    )
    .unwrap();
    (store, idea.id, ready.run.unwrap().id)
}

struct HookExecutor<F: FnMut(&StageJob) -> StageOutcome> {
    calls: Arc<Mutex<Vec<(String, u32)>>>,
    hook: F,
}

impl<F: FnMut(&StageJob) -> StageOutcome> Executor for HookExecutor<F> {
    fn ready_clis(&self) -> Vec<tandem::factory::executor::ReadyCli> {
        vec![tandem::factory::executor::ReadyCli::new(
            "claude",
            Some(tandem::factory::broker::terms::AuthType::ApiKey),
        )]
    }
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
                let caller = tandem::factory::approval::Caller::cli(true, &no_env, &|_| None);
                pause_run_by_user(&store_clone, &job.run, false, &user_actor(), &caller).unwrap();
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
                let caller = tandem::factory::approval::Caller::cli(true, &no_env, &|_| None);
                let res = pause_run_by_user(&store_clone, &job.run, true, &user_actor(), &caller)
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
    resume_paused_run_with_probe(&store, &run_id, &user_actor(), &StaticLoginProbe(true)).unwrap();
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
fn resume_verifies_plan_hash_workflow_hash_and_cli_readiness() {
    let ws = temp_ws("resume-checks");
    let (store, idea_id, run_id) = setup_queued_run(&ws);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut exec = HookExecutor {
        calls: Arc::clone(&calls),
        hook: |_| StageOutcome::Succeeded,
    };

    // Run plan, then pause
    let caller = tandem::factory::approval::Caller::cli(true, &no_env, &|_| None);
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    pause_run_by_user(&store, &run_id, false, &user_actor(), &caller).unwrap();

    // 1. Tamper with plan snapshot -> resume refused, run stays Paused
    let snap_path = ws
        .join(".agent-workspace/factory/ideas")
        .join(&idea_id)
        .join("artifacts/plan-snapshot.md");
    let orig_snap = fs::read(&snap_path).unwrap();
    fs::write(&snap_path, b"tampered plan").unwrap();
    let err = resume_paused_run_with_probe(&store, &run_id, &user_actor(), &StaticLoginProbe(true))
        .unwrap_err();
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
    let err = resume_paused_run_with_probe(&store, &run_id, &user_actor(), &StaticLoginProbe(true))
        .unwrap_err();
    assert!(err.to_string().to_lowercase().contains("workflow"));
    assert_eq!(
        get_run(&store, &run_id).unwrap().unwrap().state,
        RunState::Paused
    );
    fs::write(&wf_path, &orig_wf).unwrap();

    // 3. CLI not signed in -> resume refused (CLI not ready)
    let err =
        resume_paused_run_with_probe(&store, &run_id, &user_actor(), &StaticLoginProbe(false))
            .unwrap_err();
    assert!(err.to_string().to_lowercase().contains("not ready"));
    assert_eq!(
        get_run(&store, &run_id).unwrap().unwrap().state,
        RunState::Paused
    );

    // 4. Signed in -> resume succeeds and continues from create-tests
    let resumed =
        resume_paused_run_with_probe(&store, &run_id, &user_actor(), &StaticLoginProbe(true))
            .unwrap();
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
    let caller = tandem::factory::approval::Caller::cli(true, &no_env, &|_| None);
    let err = cancel_run(&store, &run_id, "r-wrong", &user_actor(), &caller).unwrap_err();
    assert!(err.to_string().contains("--confirm"));

    // Agent caller must fail
    let agent_env = |k: &str| {
        if k == "TANDEM_ROLE" {
            Some("orchestrator".to_string())
        } else {
            None
        }
    };
    let agent_caller = tandem::factory::approval::Caller::cli(true, &agent_env, &|_| None);
    let err = cancel_run(&store, &run_id, &run_id, &user_actor(), &agent_caller).unwrap_err();
    assert!(matches!(err, RunnerError::Approval(_)));

    // Valid cancel succeeds, keeps worktree, returns idea to ReadyForDevelopment
    let res = cancel_run(&store, &run_id, &run_id, &user_actor(), &caller).unwrap();
    assert_eq!(res.run.state, RunState::Cancelled);
    assert_eq!(res.idea.state, IdeaState::ReadyForDevelopment);
    assert!(wt_path.exists(), "worktree must be kept on cancel");

    // Cancelled run must never start another stage
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    assert_eq!(calls.lock().unwrap().len(), 1);

    // Idea can create a new run or revoke ready tag
    let revoked = revoke_ready(&store, &idea_id, "alice", &caller).unwrap();
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
    let caller = tandem::factory::approval::Caller::cli(true, &no_env, &|_| None);
    let err = cancel_idea(&store, &idea_id, "i-wrong", &user_actor(), &caller).unwrap_err();
    assert!(err.to_string().contains("--confirm"));

    let res = cancel_idea(&store, &idea_id, &idea_id, &user_actor(), &caller).unwrap();
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
    let caller = tandem::factory::approval::Caller::cli(true, &no_env, &|_| None);
    cfg.factory = true;
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = tandem::factory::commands::run_with_caller(
        &parsed.args,
        &ws,
        &cfg,
        &caller,
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
    let code = tandem::factory::commands::run_with_caller_and_probe(
        &parsed.args,
        &ws,
        &cfg,
        &caller,
        &StaticLoginProbe(true),
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
    let code = tandem::factory::commands::run_with_caller(
        &parsed.args,
        &ws,
        &cfg,
        &caller,
        &mut out,
        &mut err,
    );
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    let stdout_str = String::from_utf8_lossy(&out);
    assert!(stdout_str.contains("Ready for development"));
    assert!(stdout_str.contains("run cleanup"));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_factory_run_log_tails_sanitizes_and_validates() {
    use clap::Parser;

    #[derive(Parser)]
    struct Wrapper {
        #[command(flatten)]
        args: FactoryArgs,
    }

    let ws = temp_ws("run-log");
    let (store, _idea_id, run_id) = setup_queued_run(&ws);
    let corr_id = format!("{run_id}.develop.1");
    let jdir = job_dir(&store, &run_id, "develop", 1);
    fs::create_dir_all(&jdir).unwrap();

    let mut payload = "A".repeat(300_000);
    payload.push_str("\n\x1b[31mRED\x1b[0m\x00\x07TAIL_END\n");
    fs::write(jdir.join("log.txt"), payload.as_bytes()).unwrap();

    let cfg = Config {
        factory: true,
        ..Default::default()
    };

    let parsed =
        Wrapper::try_parse_from(["factory", "run", "log", &run_id, &corr_id, "--tail", "64"])
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
    let text = String::from_utf8(out).unwrap();
    assert!(text.len() <= 64);
    assert!(text.contains("REDTAIL_END"));
    assert!(!text.contains('\x1b'));
    assert!(!text.contains('\x00'));

    let parsed_over = Wrapper::try_parse_from([
        "factory", "run", "log", &run_id, &corr_id, "--tail", "262145",
    ])
    .unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_io(
        &parsed_over.args,
        &ws,
        &cfg,
        &mut Cursor::new(b""),
        &mut out,
        &mut err,
    );
    assert_eq!(code, 1);

    let parsed_bad_job = Wrapper::try_parse_from([
        "factory",
        "run",
        "log",
        &run_id,
        "r-00000000.develop.1",
        "--tail",
        "100",
    ])
    .unwrap();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_io(
        &parsed_bad_job.args,
        &ws,
        &cfg,
        &mut Cursor::new(b""),
        &mut out,
        &mut err,
    );
    assert_eq!(code, 1);

    let _ = fs::remove_dir_all(&ws);
}

fn run_tandem_cli(
    ws: &Path,
    state_dir: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
) -> std::process::Output {
    tandem::web_manager::save_web_config(
        &tandem::web_manager::web_config_path(&state_dir.join("tandem")),
        &tandem::web_manager::WebConfig {
            auth_token: "test-token".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let stubs_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("stub-clis");
    let orig_path = std::env::var("PATH").unwrap_or_default();
    let child_path = format!("{}:{orig_path}", stubs_dir.display());
    let mut cmd = StdCommand::new(env!("CARGO_BIN_EXE_tandem"));
    cmd.args(args)
        .env("PATH", child_path)
        .env("TANDEM_WORKSPACE", ws)
        .env("XDG_STATE_HOME", state_dir)
        .env("TANDEM_AUTH_TOKEN", "test-token")
        .env("TANDEM_INTERNAL_SURFACE", "web")
        .env_remove("TANDEM_FACTORY")
        .env_remove("TANDEM_ROLE")
        .env_remove("TANDEM_SESSION_ID")
        .env_remove("TANDEM_FACTORY_IDEA");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.output().expect("run tandem binary")
}

#[test]
fn cli_binary_run_target_retry_pause_resume_cancel_and_cleanup() {
    let ws = temp_ws("cli-bin-run");
    let state_dir = ws.join(".state");
    fs::create_dir_all(&state_dir).unwrap();
    let (store, _idea_id, run_id) = setup_queued_run(&ws);
    let repo_a = ws.join("codebase-symlinks/repo-a");
    let repo_b = ws.join("codebase-symlinks/repo-b");
    fs::create_dir_all(&repo_a).unwrap();
    fs::create_dir_all(&repo_b).unwrap();
    init_git_repo(&repo_a);
    init_git_repo(&repo_b);

    let factory_env = &[("TANDEM_FACTORY", "1")];
    let agent_env = &[("TANDEM_FACTORY", "1"), ("TANDEM_ROLE", "orchestrator")];

    // Target: agent caller forbidden -> exit 3
    let target_agent = run_tandem_cli(
        &ws,
        &state_dir,
        &["factory", "run", "target", &run_id, "repo-a", "--json"],
        agent_env,
    );
    assert_eq!(target_agent.status.code(), Some(3));

    // Target: unknown repo -> exit 1
    let target_bad = run_tandem_cli(
        &ws,
        &state_dir,
        &["factory", "run", "target", &run_id, "nope", "--json"],
        factory_env,
    );
    assert_eq!(target_bad.status.code(), Some(1));

    // Target: valid -> exit 0
    let target_ok = run_tandem_cli(
        &ws,
        &state_dir,
        &["factory", "run", "target", &run_id, "repo-a", "--json"],
        factory_env,
    );
    assert_eq!(
        target_ok.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&target_ok.stderr)
    );
    let target_json: serde_json::Value = serde_json::from_slice(&target_ok.stdout).unwrap();
    assert_eq!(target_json["target"], "repo-a");

    // Drive one failing step so run pauses with retries exhausted on `plan`
    let mut exec = HookExecutor {
        calls: Arc::new(Mutex::new(Vec::new())),
        hook: |_| StageOutcome::Failed {
            reason: "no structured result".to_string(),
        },
    };
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    assert_eq!(
        get_run(&store, &run_id).unwrap().unwrap().state,
        RunState::Paused
    );

    // Retry: agent caller forbidden -> exit 3
    let retry_agent = run_tandem_cli(
        &ws,
        &state_dir,
        &[
            "factory",
            "run",
            "retry",
            &run_id,
            "plan",
            "--hypothesis",
            "try again",
            "--json",
        ],
        agent_env,
    );
    assert_eq!(retry_agent.status.code(), Some(3));

    // Retry: empty hypothesis -> exit 1
    let retry_empty = run_tandem_cli(
        &ws,
        &state_dir,
        &[
            "factory",
            "run",
            "retry",
            &run_id,
            "plan",
            "--hypothesis",
            "   ",
            "--json",
        ],
        factory_env,
    );
    assert_eq!(retry_empty.status.code(), Some(1));

    // Retry: valid -> exit 0
    let retry_ok = run_tandem_cli(
        &ws,
        &state_dir,
        &[
            "factory",
            "run",
            "retry",
            &run_id,
            "plan",
            "--hypothesis",
            "use smaller slice",
            "--json",
        ],
        factory_env,
    );
    assert_eq!(
        retry_ok.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&retry_ok.stderr)
    );
    let retry_json: serde_json::Value = serde_json::from_slice(&retry_ok.stdout).unwrap();
    assert_eq!(retry_json["state"], "running");

    // Pause (human-readable) -> Resume (human-readable)
    let pause_txt = run_tandem_cli(
        &ws,
        &state_dir,
        &["factory", "run", "pause", &run_id],
        factory_env,
    );
    assert_eq!(pause_txt.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&pause_txt.stdout).contains("paused"));

    let resume_txt = run_tandem_cli(
        &ws,
        &state_dir,
        &["factory", "run", "resume", &run_id],
        factory_env,
    );
    assert_eq!(resume_txt.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&resume_txt.stdout).contains("resumed"));

    // Cancel: wrong confirm -> exit 1, then valid --json -> exit 0
    let cancel_bad = run_tandem_cli(
        &ws,
        &state_dir,
        &["factory", "run", "cancel", &run_id, "--confirm", "wrong"],
        factory_env,
    );
    assert_eq!(cancel_bad.status.code(), Some(1));

    let cancel_ok = run_tandem_cli(
        &ws,
        &state_dir,
        &[
            "factory",
            "run",
            "cancel",
            &run_id,
            "--confirm",
            &run_id,
            "--json",
        ],
        factory_env,
    );
    assert_eq!(cancel_ok.status.code(), Some(0));

    // Cleanup: wrong confirm -> exit 1, then valid --json -> exit 0
    let cleanup_bad = run_tandem_cli(
        &ws,
        &state_dir,
        &[
            "factory",
            "run",
            "cleanup",
            &run_id,
            "--confirm",
            "wrong",
            "--json",
        ],
        factory_env,
    );
    assert_eq!(cleanup_bad.status.code(), Some(1));

    let cleanup_ok = run_tandem_cli(
        &ws,
        &state_dir,
        &[
            "factory",
            "run",
            "cleanup",
            &run_id,
            "--confirm",
            &run_id,
            "--json",
        ],
        factory_env,
    );
    assert_eq!(
        cleanup_ok.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&cleanup_ok.stderr)
    );
    let cleanup_json: serde_json::Value = serde_json::from_slice(&cleanup_ok.stdout).unwrap();
    assert_eq!(cleanup_json["cleaned"], run_id);

    let _ = fs::remove_dir_all(&ws);
}
