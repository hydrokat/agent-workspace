mod common;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use std::sync::Arc;
use tandem::factory::approval::mark_ready_with_workflow;
use tandem::factory::artifacts;
use tandem::factory::events::read_all;
use tandem::factory::executor::{Executor, StageJob, StageOutcome};
use tandem::factory::login::LoginProbe;
use tandem::factory::machine::Command;
use tandem::factory::model::{Actor, ActorKind, IdeaState, RunState, StageState};
use tandem::factory::run::get_run;
use tandem::factory::runner::{
    RunnerError, acquire_runner_lock, drive_run_step, resume_paused_run_with_probe,
    retry_paused_stage, retry_paused_stage_with_probe,
};

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
    let no_env = |_: &str| None;
    let no_pane = |_: &str| None;
    let caller = tandem::factory::approval::Caller::cli(true, &no_env, &no_pane);
    let zero_grace = tandem::factory::model::FactorySettings {
        approval_grace_secs: 0,
        ..Default::default()
    };
    tandem::factory::settings::save(&store, &zero_grace).unwrap();
    let ready = mark_ready_with_workflow(
        &store, ws, &idea.id, &idea.id, "alice", None, false, &caller,
    )
    .unwrap();
    (store, idea.id, ready.run.unwrap().id)
}

struct ScriptedExecutor {
    outcomes: HashMap<(String, u32), StageOutcome>,
    calls: Vec<(String, u32)>,
    ready: bool,
}

impl Executor for ScriptedExecutor {
    fn ready_clis(&self) -> Vec<tandem::factory::executor::ReadyCli> {
        if !self.ready {
            return Vec::new();
        }
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
        ready: false,
    };

    // 1. No CLI is signed in -> run pauses with reason & resolution command
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    let run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(run.state, RunState::Paused);
    assert!(
        run.pause_reason
            .as_deref()
            .unwrap()
            .contains("no CLI is installed and signed in")
    );
    assert!(exec.calls.is_empty());

    // 2. Sign in and resume/drive -> plan succeeds, idea moves to InDevelopment
    exec.ready = true;
    let user = Actor {
        kind: ActorKind::User,
        id: "alice".into(),
    };
    resume_paused_run_with_probe(&store, &run_id, &user, &StaticLoginProbe(true)).unwrap();
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
    let no_env = |_: &str| None;
    let no_pane = |_: &str| None;
    let caller = tandem::factory::approval::Caller::cli(true, &no_env, &no_pane);
    retry_paused_stage_with_probe(
        &store,
        &run_id,
        "develop",
        "try smaller slice",
        &user,
        &caller,
        &StaticLoginProbe(true),
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
        ready: false,
    };

    // No CLI signed in -> run pauses
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    let user = Actor {
        kind: ActorKind::User,
        id: "alice".into(),
    };

    // Agent caller rejected via env and via actor kind
    let agent_env = |k: &str| {
        if k == "TANDEM_ROLE" {
            Some("orchestrator".to_string())
        } else {
            None
        }
    };
    let no_env = |_: &str| None;
    let no_pane = |_: &str| None;
    let agent_caller = tandem::factory::approval::Caller::cli(true, &agent_env, &no_pane);
    let user_caller = tandem::factory::approval::Caller::cli(true, &no_env, &no_pane);
    let err_env = retry_paused_stage(
        &store,
        &run_id,
        "plan",
        "new hypothesis",
        &user,
        &agent_caller,
    )
    .unwrap_err();
    assert_eq!(err_env.code(), "agent_caller_forbidden");

    let orch = Actor {
        kind: ActorKind::Orchestrator,
        id: "orch".into(),
    };
    let err_actor = retry_paused_stage(
        &store,
        &run_id,
        "plan",
        "new hypothesis",
        &orch,
        &user_caller,
    )
    .unwrap_err();
    assert_eq!(err_actor.code(), "agent_caller_forbidden");

    // Closed gate blocks retry
    let err_gate = retry_paused_stage_with_probe(
        &store,
        &run_id,
        "plan",
        "new hypothesis",
        &user,
        &user_caller,
        &StaticLoginProbe(false),
    )
    .unwrap_err();
    assert_eq!(err_gate.code(), "gate_not_met");

    // Signed in, then tamper with plan snapshot -> hash mismatch blocks retry
    let snap_path = store
        .root
        .join("ideas")
        .join(&idea_id)
        .join("artifacts")
        .join("plan-snapshot.md");
    let orig_snap = fs::read_to_string(&snap_path).unwrap();
    fs::write(&snap_path, "# Tampered\n").unwrap();
    let err_hash = retry_paused_stage_with_probe(
        &store,
        &run_id,
        "plan",
        "new hypothesis",
        &user,
        &user_caller,
        &StaticLoginProbe(true),
    )
    .unwrap_err();
    assert_eq!(err_hash.code(), "gate_not_met");
    fs::write(&snap_path, orig_snap).unwrap();

    // Valid retry succeeds and persists hypothesis on stage + event
    let updated = retry_paused_stage_with_probe(
        &store,
        &run_id,
        "plan",
        "  check edge case  ",
        &user,
        &user_caller,
        &StaticLoginProbe(true),
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
    let mut exec = tandem::factory::executor::BrokerExecutor::new(&ws)
        .with_login_probe(Arc::new(StaticLoginProbe(false)));
    let outcome = exec.run(&StageJob {
        run: run_id.clone(),
        stage: "plan".to_string(),
        attempt: 1,
    });
    assert!(matches!(outcome, StageOutcome::Blocked { .. }));

    // Open gate and pre-populate valid job result + plan artifacts -> idempotent path verifies contract
    let mut exec = tandem::factory::executor::BrokerExecutor::new(&ws)
        .with_login_probe(Arc::new(StaticLoginProbe(true)));
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

#[cfg(unix)]
#[test]
fn broker_executor_runs_end_to_end_with_fake_provider_script() {
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    if !common::require_tool("tmux") {
        return;
    }

    struct TmuxServerGuard(String);
    impl Drop for TmuxServerGuard {
        fn drop(&mut self) {
            let _ = StdCommand::new("tmux")
                .args(["-L", &self.0, "kill-server"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    let ws = temp_ws("broker-e2e");
    let state_dir = ws.join(".state");
    fs::create_dir_all(&state_dir).unwrap();
    let tmux_socket = format!("tandem-e2e-{}", std::process::id());
    let _tmux_guard = TmuxServerGuard(tmux_socket.clone());
    assert!(
        StdCommand::new("tmux")
            .args(["-L", &tmux_socket, "new-session", "-d", "-s", "tandem"])
            .status()
            .unwrap()
            .success(),
        "failed to start isolated tmux server"
    );
    assert!(
        StdCommand::new("tmux")
            .args([
                "-L",
                &tmux_socket,
                "set-window-option",
                "-g",
                "remain-on-exit",
                "on"
            ])
            .status()
            .unwrap()
            .success(),
        "failed to set remain-on-exit on isolated tmux server"
    );

    let (store, _idea_id, run_id) = setup_queued_run(&ws);

    let fake_bin = ws.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let script_path = fake_bin.join("claude");
    let script = r#"#!/bin/sh
if [ "${1:-}" = "auth" ] && [ "${2:-}" = "status" ]; then
  echo '{"loggedIn":true,"authMethod":"apiKey"}'
  exit 0
fi
if [ ! -t 0 ]; then
  cat >/dev/null
fi
RUN_DIR="$(dirname "$(dirname "$TANDEM_JOB_DIR")")"
printf 'stage=%s attempt=%s\n' "$TANDEM_STAGE" "$TANDEM_ATTEMPT" > "$TANDEM_JOB_DIR/worker-ran.txt"
if [ "$TANDEM_STAGE" = "plan" ] && [ "$TANDEM_ATTEMPT" = "1" ]; then
  cat > "$TANDEM_JOB_DIR/result.json" <<'EOF'
{"status":"failed","summary":"attempt 1 failed","files_changed":[],"artifacts":[],"rationale":"synthetic failure","hypothesis":"create missing artifacts"}
EOF
  exit 1
fi
if [ "$TANDEM_STAGE" = "plan" ]; then
  mkdir -p "$RUN_DIR/artifacts/specs/p-001-demo"
  printf '# Impl\n' > "$RUN_DIR/artifacts/specs/p-001-demo/impl.md"
  cat > "$RUN_DIR/artifacts/slices.json" <<'EOF'
{"phase":"p-001-demo","slices":[{"id":"t-001","title":"Slice","type":"New Feature","paths":["README.md"],"dependencies":[],"acceptance_criteria":["Works"],"ui":false}]}
EOF
  cat > "$TANDEM_JOB_DIR/result.json" <<'EOF'
{"status":"succeeded","summary":"plan complete","files_changed":[],"artifacts":["specs/p-001-demo/impl.md","slices.json"],"hypothesis":"create impl and slices"}
EOF
  exit 0
fi
cat > "$TANDEM_JOB_DIR/result.json" <<'EOF'
{"status":"failed","summary":"stop here","files_changed":[],"artifacts":[],"rationale":"no structured result"}
EOF
exit 1
"#;
    fs::write(&script_path, script).unwrap();
    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

    for helper_cli in ["agy", "opencode"] {
        let p = fake_bin.join(helper_cli);
        fs::write(&p, "#!/bin/sh\nexit 1\n").unwrap();
        let mut hperms = fs::metadata(&p).unwrap().permissions();
        hperms.set_mode(0o755);
        fs::set_permissions(&p, hperms).unwrap();
    }

    let mut new_path = OsString::from(fake_bin.as_os_str());
    if let Some(orig) = std::env::var_os("PATH") {
        new_path.push(":");
        new_path.push(orig);
    }

    let mut child = StdCommand::new(env!("CARGO_BIN_EXE_tandem"))
        .args(["factory", "runner"])
        .env("PATH", &new_path)
        .env("TANDEM_WORKSPACE", &ws)
        .env("XDG_STATE_HOME", &state_dir)
        .env("TANDEM_FACTORY", "1")
        .env("TANDEM_TMUX_SOCKET", &tmux_socket)
        .env_remove("TANDEM_ROLE")
        .env_remove("TANDEM_SESSION_ID")
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn tandem factory runner");

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut final_run = None;
    while Instant::now() < deadline {
        if let Ok(Some(run)) = get_run(&store, &run_id)
            && run.state == RunState::Paused
        {
            final_run = Some(run);
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let _ = child.kill();
    let out = child.wait_with_output().unwrap();
    let cur_run = get_run(&store, &run_id).unwrap();

    let run = final_run.unwrap_or_else(|| {
        panic!(
            "runner should pause after create-tests fails; cur_run={cur_run:?} stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    let plan_stage = run.stages.iter().find(|s| s.name == "plan").unwrap();
    assert_eq!(
        plan_stage.state,
        StageState::Succeeded,
        "plan_stage={plan_stage:?} pause_reason={:?}",
        run.pause_reason
    );
    assert_eq!(plan_stage.attempts, 2);

    let j1 = tandem::factory::broker::job::job_dir(&store, &run_id, "plan", 1);
    assert!(j1.join("pane_id").is_file(), "expected pane_id in plan.1");
    let ran1 = fs::read_to_string(j1.join("worker-ran.txt")).unwrap();
    assert!(ran1.contains("stage=plan attempt=1"));

    let ct_stage = run
        .stages
        .iter()
        .find(|s| s.name == "create-tests")
        .unwrap();
    assert_eq!(ct_stage.attempts, 1);

    let _ = fs::remove_dir_all(&ws);
}
