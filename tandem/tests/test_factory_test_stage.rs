mod common;

use std::collections::{BTreeMap, HashMap};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use common::factory_fixture::{StubEnvLoginProbe, StubWorkerTmuxRunner};
use tandem::factory::approval::{Caller, mark_ready_with_workflow};
use tandem::factory::artifacts;
use tandem::factory::broker::checks::{CheckOutcome, MAX_CHECK_LOG_BYTES, run_checks};
use tandem::factory::broker::job::jobs_dir;
use tandem::factory::checks::{CheckKind, ChecksConfig, ConfirmedChecks, set_checks};
use tandem::factory::events::read_all;
use tandem::factory::executor::{BrokerExecutor, Executor, StageJob, StageOutcome};
use tandem::factory::machine::Command;
use tandem::factory::model::{Actor, ActorKind, RunState, StageState};
use tandem::factory::run::get_run;
use tandem::factory::runner::drive_run_step;
use tandem::factory::stages::test::run_test_stage_checks;
use tandem::factory::store::FactoryStore;
use tandem::tmux::SESSION;
use tandem::tmux::client::TmuxClient;

fn temp_ws(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-stage-{label}-{}-{nanos}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn init_git_repo(dir: &Path) {
    let run = |args: &[&str]| {
        let out = StdCommand::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run(&["init", "-b", "trunk"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    fs::write(dir.join("README.md"), "# Test\n").unwrap();
    run(&["add", "README.md"]);
    run(&["commit", "-m", "init"]);
}

fn confirmed_from(
    target: &str,
    test: Vec<Vec<String>>,
    lint: Vec<Vec<String>>,
    timeout_secs: u64,
) -> ConfirmedChecks {
    let cfg = ChecksConfig {
        test,
        lint,
        timeout_secs,
        ui_paths: vec![],
        confirmed_by: Some("alice".to_string()),
        confirmed_at: Some(1000.0),
    };
    ConfirmedChecks::from_config(target, &cfg).unwrap()
}

fn setup_queued_run(ws: &Path) -> (FactoryStore, String, String) {
    init_git_repo(ws);
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();
    let mut settings = store.settings().unwrap();
    settings.approval_grace_secs = 0;
    tandem::factory::settings::save(&store, &settings).unwrap();
    let user = Actor {
        kind: ActorKind::User,
        id: "alice".into(),
    };
    let orch = Actor {
        kind: ActorKind::Orchestrator,
        id: "orch".into(),
    };
    let idea = store
        .create_idea("Test stage", "Verify test stage", "alice", &user)
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
    let caller = Caller::cli(true, &|_| None, &|_| None);
    let ready = mark_ready_with_workflow(
        &store, ws, &idea.id, &idea.id, "alice", None, false, &caller,
    )
    .unwrap();
    (store, idea.id, ready.run.unwrap().id)
}

struct ScriptedExecutor {
    outcomes: HashMap<(String, u32), StageOutcome>,
    calls: Vec<(String, u32)>,
}

impl Executor for ScriptedExecutor {
    fn ready_clis(&self) -> Vec<tandem::factory::executor::ReadyCli> {
        vec![tandem::factory::executor::ReadyCli::new(
            "claude",
            Some(tandem::factory::broker::terms::AuthType::ApiKey),
        )]
    }

    fn run(&mut self, job: &StageJob) -> StageOutcome {
        self.calls.push((job.stage.clone(), job.attempt));
        self.outcomes
            .get(&(job.stage.clone(), job.attempt))
            .cloned()
            .unwrap_or(StageOutcome::Succeeded)
    }
}

#[test]
fn run_checks_scrubs_env_and_writes_attempt_log() {
    let ws = temp_ws("scrub-env");
    let worktree = ws.join("wt");
    let artifact_dir = ws.join("artifacts");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&artifact_dir).unwrap();

    let confirmed = confirmed_from(
        "workspace",
        vec![vec![
            "python3".into(),
            "-c".into(),
            "import os, json; print(json.dumps(dict(os.environ)))".into(),
        ]],
        vec![],
        30,
    );
    // Note: ConfirmedChecks::from_config rejects `python3 -c`? Wait - `python3` is not in SHELL_BINARIES,
    // so `python3 -c` is allowed!
    let outcomes: Vec<CheckOutcome> =
        run_checks(&confirmed, CheckKind::Test, &worktree, &artifact_dir, 1).unwrap();
    assert_eq!(outcomes.len(), 1);
    let outcome = &outcomes[0];
    assert!(outcome.passed(), "expected pass, got {outcome:?}");
    assert_eq!(outcome.exit_code, Some(0));
    assert!(!outcome.timed_out);
    assert!(!outcome.log_cap_exceeded);
    assert!(
        outcome.log_path.ends_with("checks/test-1-attempt1.log"),
        "unexpected log path: {}",
        outcome.log_path.display()
    );

    let raw = fs::read_to_string(&outcome.log_path).unwrap();
    let env_map: BTreeMap<String, String> = serde_json::from_str(raw.trim()).unwrap();
    assert_eq!(env_map.get("CI").map(String::as_str), Some("1"));
    for key in env_map.keys() {
        assert!(
            matches!(
                key.as_str(),
                "PATH" | "HOME" | "LANG" | "LC_ALL" | "CI" | "__CF_USER_TEXT_ENCODING"
            ),
            "unexpected env var leaked into check: {key}"
        );
        assert!(!key.starts_with("TANDEM_"), "tandem env leaked: {key}");
    }

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn run_checks_enforces_timeout() {
    let ws = temp_ws("timeout");
    let worktree = ws.join("wt");
    let artifact_dir = ws.join("artifacts");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&artifact_dir).unwrap();

    let confirmed = confirmed_from(
        "workspace",
        vec![vec!["sleep".into(), "5".into()]],
        vec![],
        1,
    );
    let outcomes = run_checks(&confirmed, CheckKind::Test, &worktree, &artifact_dir, 1).unwrap();
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].timed_out);
    assert!(!outcomes[0].passed());

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn run_checks_caps_log_at_16_mib() {
    let ws = temp_ws("log-cap");
    let worktree = ws.join("wt");
    let artifact_dir = ws.join("artifacts");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&artifact_dir).unwrap();

    let confirmed = confirmed_from(
        "workspace",
        vec![vec![
            "python3".into(),
            "-c".into(),
            "import sys; chunk = b'A' * (1024 * 1024); [sys.stdout.buffer.write(chunk) for _ in range(20)]; sys.stdout.buffer.flush()".into(),
        ]],
        vec![],
        30,
    );
    let outcomes = run_checks(&confirmed, CheckKind::Test, &worktree, &artifact_dir, 1).unwrap();
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].log_cap_exceeded);
    assert!(!outcomes[0].passed());
    let log_len = fs::metadata(&outcomes[0].log_path).unwrap().len() as usize;
    assert!(log_len <= MAX_CHECK_LOG_BYTES);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn run_checks_uses_run_tests_py_for_failures_only_summary_when_present() {
    let ws = temp_ws("run-tests-py");
    let worktree = ws.join("wt");
    let artifact_dir = ws.join("artifacts");
    let wrapper_dir = worktree.join(".agents/skills/test-failures/scripts");
    fs::create_dir_all(&wrapper_dir).unwrap();
    fs::create_dir_all(&artifact_dir).unwrap();

    fs::write(
        wrapper_dir.join("run_tests.py"),
        r#"#!/usr/bin/env python3
import subprocess, sys
idx = sys.argv.index("--")
cmd = sys.argv[idx + 1:]
proc = subprocess.run(cmd, capture_output=True, text=True)
if proc.returncode != 0:
    print("FAILURES_ONLY_SUMMARY: test_alpha failed")
sys.exit(proc.returncode)
"#,
    )
    .unwrap();

    let confirmed = confirmed_from(
        "workspace",
        vec![vec![
            "python3".into(),
            "-c".into(),
            "import sys; print('NOISY PASSING LINE'); sys.exit(2)".into(),
        ]],
        vec![],
        30,
    );
    let outcomes = run_checks(&confirmed, CheckKind::Test, &worktree, &artifact_dir, 1).unwrap();
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[0].exit_code, Some(2));
    assert!(!outcomes[0].passed());
    let summary = outcomes[0].summary.as_deref().unwrap_or("");
    assert!(
        summary.contains("FAILURES_ONLY_SUMMARY: test_alpha failed"),
        "got summary: {summary}"
    );
    assert!(!summary.contains("NOISY PASSING LINE"));

    let _ = fs::remove_dir_all(&ws);
}

use tandem::factory::login::LoginProbe;

struct ReadyClaudeProbe;
impl LoginProbe for ReadyClaudeProbe {
    fn which(&self, bin: &str) -> Option<PathBuf> {
        (bin == "claude").then(|| PathBuf::from("/usr/local/bin/claude"))
    }
    fn version(&self, _cli: &str) -> String {
        "claude 2.0".to_string()
    }
    fn run(&self, _argv: &[&str]) -> (i32, String, String) {
        (
            0,
            r#"{"loggedIn":true,"authMethod":"apiKey"}"#.to_string(),
            String::new(),
        )
    }
}

#[test]
fn test_stage_passing_checks_succeeds_without_starting_agent_job() {
    let ws = temp_ws("pass-no-agent");
    let (store, _idea_id, run_id) = setup_queued_run(&ws);
    let caller = Caller::cli(true, &|_| None, &|_| None);
    set_checks(
        &store,
        "workspace",
        vec![vec!["true".into()]],
        vec![vec!["true".into()]],
        Some(30),
        vec![],
        "workspace",
        "alice",
        &caller,
    )
    .unwrap();

    // Advance plan, create-tests, develop with scripted executor, then run test stage with BrokerExecutor.
    let mut scripted = ScriptedExecutor {
        outcomes: HashMap::new(),
        calls: vec![],
    };
    drive_run_step(&store, &ws, &run_id, &mut scripted).unwrap(); // plan
    drive_run_step(&store, &ws, &run_id, &mut scripted).unwrap(); // create-tests
    drive_run_step(&store, &ws, &run_id, &mut scripted).unwrap(); // develop

    let mut broker = BrokerExecutor::new(&ws).with_login_probe(Arc::new(ReadyClaudeProbe));
    drive_run_step(&store, &ws, &run_id, &mut broker).unwrap(); // test

    let run = get_run(&store, &run_id).unwrap().unwrap();
    let test_stage = run.stages.iter().find(|s| s.name == "test").unwrap();
    assert_eq!(test_stage.state, StageState::Succeeded);
    assert!(
        !jobs_dir(&store, &run_id).join("test-1").exists(),
        "no agent job should be created when all checks pass"
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_stage_failing_checks_invokes_economy_verifier_and_records_diagnosis() {
    let ws = temp_ws("fail-verifier");
    let (store, _idea_id, run_id) = setup_queued_run(&ws);
    let caller = Caller::cli(true, &|_| None, &|_| None);
    set_checks(
        &store,
        "workspace",
        vec![vec!["false".into()]],
        vec![],
        Some(30),
        vec![],
        "workspace",
        "alice",
        &caller,
    )
    .unwrap();

    let mut scripted = ScriptedExecutor {
        outcomes: HashMap::new(),
        calls: vec![],
    };
    drive_run_step(&store, &ws, &run_id, &mut scripted).unwrap(); // plan
    drive_run_step(&store, &ws, &run_id, &mut scripted).unwrap(); // create-tests
    drive_run_step(&store, &ws, &run_id, &mut scripted).unwrap(); // develop

    // Create a fake `claude` binary that writes result.json even if it claims succeeded;
    // tandem must still judge the test stage as failed.
    let bin_dir = ws.join("fake-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let claude_bin = bin_dir.join("claude");
    let capture_args = ws.join("claude-args.txt");
    fs::write(
        &claude_bin,
        format!(
            r#"#!/bin/sh
if [ "${{1:-}}" = "auth" ] && [ "${{2:-}}" = "status" ]; then
  echo '{{"loggedIn":true,"authMethod":"apiKey"}}'
  exit 0
fi
echo "$@" > "{}"
cat > "$TANDEM_JOB_DIR/result.json" <<'EOF'
{{"status":"succeeded","summary":"checked failures","rationale":"diagnosis: false exited 1; hypothesis: fix assertion","files_changed":[],"artifacts":[],"tests_run":[]}}
EOF
"#,
            capture_args.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&claude_bin).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&claude_bin, perms).unwrap();
    }

    let mut env_map = BTreeMap::new();
    let old_path = std::env::var("PATH").unwrap_or_default();
    env_map.insert(
        "PATH".to_string(),
        format!("{}:{old_path}", bin_dir.display()),
    );
    if let Ok(home) = std::env::var("HOME") {
        env_map.insert("HOME".to_string(), home);
    }

    let broker = BrokerExecutor::new(&ws)
        .with_tmux(TmuxClient::with_runner(
            SESSION,
            None,
            Arc::new(StubWorkerTmuxRunner::new()),
        ))
        .with_login_probe(Arc::new(StubEnvLoginProbe {
            env: env_map.clone(),
            signed_out: false,
            stub_only_dir: Some(bin_dir.clone()),
        }));
    let outcome = broker.run_with_env(
        &StageJob {
            run: run_id.clone(),
            stage: "test".to_string(),
            attempt: 1,
        },
        env_map,
    );

    match outcome {
        StageOutcome::Failed { reason } => {
            assert!(
                reason.contains("diagnosis: false exited 1"),
                "expected verifier diagnosis, got: {reason}"
            );
        }
        other => panic!("expected Failed outcome, got {other:?}"),
    }

    let invoked_args = fs::read_to_string(&capture_args).unwrap();
    assert!(
        invoked_args.contains("claude-haiku-4-5"),
        "verifier must run on economy tier, got args: {invoked_args}"
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn runner_reopens_develop_on_test_failure_and_pauses_when_develop_retries_exhausted() {
    let ws = temp_ws("reopen-develop");
    let (store, _idea_id, run_id) = setup_queued_run(&ws);
    let caller = Caller::cli(true, &|_| None, &|_| None);
    set_checks(
        &store,
        "workspace",
        vec![vec!["true".into()]],
        vec![],
        Some(30),
        vec![],
        "workspace",
        "alice",
        &caller,
    )
    .unwrap();

    let mut exec = ScriptedExecutor {
        outcomes: HashMap::from([
            (
                ("test".into(), 1),
                StageOutcome::Failed {
                    reason: "diagnosis 1: missing edge case".into(),
                },
            ),
            (
                ("test".into(), 2),
                StageOutcome::Failed {
                    reason: "diagnosis 2: still failing".into(),
                },
            ),
            (
                ("test".into(), 3),
                StageOutcome::Failed {
                    reason: "diagnosis 3: retries exhausted".into(),
                },
            ),
        ]),
        calls: vec![],
    };

    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap(); // plan
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap(); // create-tests
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap(); // develop (1)
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap(); // test (1) fails -> reopens develop

    let run1 = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(run1.state, RunState::Running);
    let dev1 = run1.stages.iter().find(|s| s.name == "develop").unwrap();
    assert_eq!(dev1.state, StageState::Pending);
    assert_eq!(
        dev1.retry_hypothesis.as_deref(),
        Some("diagnosis 1: missing edge case")
    );

    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap(); // develop (2)
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap(); // test (2) fails -> reopens develop (retry 2 of 2)
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap(); // develop (3)
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap(); // test (3) fails -> develop retries exhausted -> pauses

    let run_final = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(run_final.state, RunState::Paused);
    assert!(!run_final.action_commands.is_empty());

    let dev_calls = exec.calls.iter().filter(|(s, _)| s == "develop").count();
    assert_eq!(dev_calls, 3, "develop should run 1 initial + 2 retries");

    let log = read_all(&ws.join(".agent-workspace/factory/events.jsonl")).unwrap();
    assert!(log.events.iter().any(|e| e.r#type == "stage.reopened"));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn baseline_failure_pauses_run_before_any_work() {
    let ws = temp_ws("baseline-fail");
    let (store, _idea_id, run_id) = setup_queued_run(&ws);
    let caller = Caller::cli(true, &|_| None, &|_| None);
    set_checks(
        &store,
        "workspace",
        vec![vec!["false".into()]],
        vec![],
        Some(30),
        vec![],
        "workspace",
        "alice",
        &caller,
    )
    .unwrap();

    let mut exec = ScriptedExecutor {
        outcomes: HashMap::new(),
        calls: vec![],
    };
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();

    let run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(run.state, RunState::Paused);
    assert_eq!(
        run.pause_reason.as_deref(),
        Some("checks fail on the base branch")
    );
    assert_eq!(run.baseline_passed, Some(false));
    assert!(
        exec.calls.is_empty(),
        "no stage should run when baseline fails: {:?}",
        exec.calls
    );

    let log = read_all(&ws.join(".agent-workspace/factory/events.jsonl")).unwrap();
    assert!(log.events.iter().any(|e| e.r#type == "run.baseline"));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_stage_report_helper_summarizes_failures() {
    let ws = temp_ws("stage-report");
    let worktree = ws.join("wt");
    let artifact_dir = ws.join("artifacts");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&artifact_dir).unwrap();

    let confirmed = confirmed_from(
        "workspace",
        vec![vec!["true".into()]],
        vec![vec!["false".into()]],
        30,
    );
    let report = run_test_stage_checks(&confirmed, &worktree, &artifact_dir, 1).unwrap();
    assert!(!report.all_passed);
    assert!(report.failures_summary.contains("lint"));
    assert!(report.failures_summary.contains("false"));

    let _ = fs::remove_dir_all(&ws);
}
