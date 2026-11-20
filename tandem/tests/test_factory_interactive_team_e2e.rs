mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use common::factory_fixture::{
    FIXED_TODAY, FactoryFixture, FixtureExecutor, StubEnvLoginProbe, TEST_TERMS, orch_actor,
    sys_actor, user_actor, user_caller,
};
use tandem::factory::approval::mark_ready;
use tandem::factory::artifacts;
use tandem::factory::broker::context::ContextManifest;
use tandem::factory::broker::job::{JobInput, JobState, job_dir, set_job_state};
use tandem::factory::events::read_all;
use tandem::factory::executor::BrokerExecutor;
use tandem::factory::login::open_login_pane;
use tandem::factory::machine::Command;
use tandem::factory::model::{FeasibilityVerdict, Risk, RunState};
use tandem::factory::recovery::diagnose_with_tmux;
use tandem::factory::run::{CreateRunOptions, create_run, get_run};
use tandem::factory::runner::drive_run_step_with_gh;
use tandem::factory::team::{
    AssignStageInput, AssignmentSource, TeamError, assign_stage, capture_agent_output, list_agents,
    load_assignments, relaunch_agent, send_agent_message, terminate_agent,
};
use tandem::factory::telemetry::{self, CostBasis};
use tandem::factory::worker::{WorkerAssignment, WorkerLaunchSpec, launch};
use tandem::tmux::SESSION;
use tandem::tmux::client::{TmuxClient, TmuxError};

struct TmuxSessionGuard {
    socket: String,
}

impl TmuxSessionGuard {
    fn start(socket: String) -> Self {
        let status = StdCommand::new("tmux")
            .args([
                "-L",
                &socket,
                "new-session",
                "-d",
                "-s",
                SESSION,
                "-n",
                "status",
                "sleep",
                "120",
            ])
            .status()
            .expect("spawn real tmux session");
        assert!(status.success(), "tmux new-session failed: {status}");
        Self { socket }
    }
}

impl Drop for TmuxSessionGuard {
    fn drop(&mut self) {
        let _ = StdCommand::new("tmux")
            .args(["-L", &self.socket, "kill-server"])
            .status();
    }
}

fn stub_clis_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("stub-clis")
        .canonicalize()
        .expect("stub-clis fixture directory")
}

fn setup_env(fixture: &FactoryFixture) -> BTreeMap<String, String> {
    let mut env = fixture.env_map();
    let stubs = stub_clis_dir();
    let old_path = env.get("PATH").cloned().unwrap_or_default();
    env.insert(
        "PATH".to_string(),
        format!("{}:{old_path}", stubs.display()),
    );
    env.insert(
        "TANDEM_TEST_SECRET".to_string(),
        "top-secret-should-not-leak".to_string(),
    );
    env.insert(
        "ANTHROPIC_API_KEY".to_string(),
        "sk-ant-top-secret-should-not-leak".to_string(),
    );
    env.insert(
        "GEMINI_API_KEY".to_string(),
        "AIza-top-secret-should-not-leak".to_string(),
    );
    env
}

fn prepare_ready_idea(fixture: &FactoryFixture, title: &str) -> String {
    let user = user_actor();
    let orch = orch_actor();

    let idea = fixture
        .store
        .create_idea(title, "Export team activity as CSV", "user", &user)
        .unwrap();

    fixture
        .store
        .execute(
            &idea.id,
            Command::BindOrchestrator {
                cli: "claude".to_string(),
                model: "opus".to_string(),
                conversation: Some("conv-iteam-1".to_string()),
            },
            &user,
        )
        .unwrap();

    fixture
        .store
        .execute(
            &idea.id,
            Command::Describe {
                title: title.to_string(),
                text: "Export team activity CSV via interactive CLI team.".to_string(),
            },
            &orch,
        )
        .unwrap();

    let feas_src = fixture.workspace.join(".agent-workspace/feas.md");
    fs::write(&feas_src, "# Feasibility\n\nFeasible.\n").unwrap();
    let feas_ref = artifacts::put(&fixture.store, &idea.id, "feasibility", &feas_src).unwrap();
    fixture
        .store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: feas_ref,
                verdict: Some(FeasibilityVerdict::Feasible),
                risk: Some(Risk::Low),
                workflow: None,
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();

    fixture
        .store
        .execute(&idea.id, Command::SelectDirection, &orch)
        .unwrap();

    let plan_src = fixture.workspace.join(".agent-workspace/plan.md");
    let plan_body = "## Scope\nExport CSV.\n\n## Acceptance criteria\n- Export team activity to CSV\n\n## Risks\nLow.\n\n## Estimates\n1 day.\n\n## Workflow recommendation\ndefault-delivery\n";
    fs::write(&plan_src, plan_body).unwrap();
    let plan_ref = artifacts::put(&fixture.store, &idea.id, "plan", &plan_src).unwrap();
    let missing = artifacts::missing_plan_sections(plan_body);
    fixture
        .store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".to_string(),
                artifact: plan_ref,
                verdict: None,
                risk: None,
                workflow: Some("default-delivery".to_string()),
                missing_sections: missing,
            },
            &orch,
        )
        .unwrap();

    mark_ready(&fixture.store, &idea.id, &idea.id, "user", &user_caller()).unwrap();
    idea.id
}

#[test]
fn real_tmux_interactive_team_end_to_end() {
    if !common::require_tool("tmux") {
        return;
    }

    let fixture = FactoryFixture::new("happy");
    let env = setup_env(&fixture);
    let stubs = stub_clis_dir();
    let probe = StubEnvLoginProbe {
        env: env.clone(),
        signed_out: false,
        stub_only_dir: Some(stubs.clone()),
    };

    let socket = format!(
        "tandem-iteam-{}-{}",
        std::process::id(),
        &uuid::Uuid::new_v4().to_string()[..8]
    );
    let _tmux_guard = TmuxSessionGuard::start(socket.clone());
    let tmux = TmuxClient::new(&socket, None);

    // 1. CLI-native login pane stays isolated from guarded worker ops.
    let login_pane = open_login_pane(&tmux, &fixture.workspace, "claude", &probe).unwrap();
    assert!(login_pane.pane_id.starts_with('%'));
    let capture_err = tmux
        .capture_worker("r-none", &login_pane.pane_id, 5)
        .unwrap_err();
    assert!(matches!(capture_err, TmuxError::ToS(_)));
    let paste_err = tmux
        .paste_worker("r-none", &login_pane.pane_id, "secret")
        .unwrap_err();
    assert!(matches!(paste_err, TmuxError::ToS(_)));
    tmux.kill_pane(&login_pane.pane_id).unwrap();

    // 2. Idea -> Gate 2 -> Run (FactoryFixture::new("happy") already confirmed workspace checks).
    let repo_dir = fixture.workspace.clone();
    let idea_id = prepare_ready_idea(&fixture, "Interactive team CSV export");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();

    // 3. Stage assignments precedence: Orchestrator assigns plan, user overrides develop.
    assign_stage(
        &fixture.store,
        AssignStageInput {
            run_id: &run.id,
            stage: "plan",
            cli: "agy",
            model: "gemini-2.5-pro",
            role: Some("planner"),
            by: AssignmentSource::Orchestrator,
            actor_id: "orch-1",
        },
    )
    .unwrap();
    assign_stage(
        &fixture.store,
        AssignStageInput {
            run_id: &run.id,
            stage: "create-tests",
            cli: "opencode",
            model: "anthropic/claude-sonnet-4-6",
            role: Some("qa-engineer"),
            by: AssignmentSource::Orchestrator,
            actor_id: "orch-1",
        },
    )
    .unwrap();
    assign_stage(
        &fixture.store,
        AssignStageInput {
            run_id: &run.id,
            stage: "review",
            cli: "claude",
            model: "sonnet",
            role: Some("code-reviewer"),
            by: AssignmentSource::Orchestrator,
            actor_id: "orch-1",
        },
    )
    .unwrap();
    assign_stage(
        &fixture.store,
        AssignStageInput {
            run_id: &run.id,
            stage: "develop",
            cli: "claude",
            model: "sonnet",
            role: Some("senior-backend-dev"),
            by: AssignmentSource::User,
            actor_id: "alice",
        },
    )
    .unwrap();
    let conflict = assign_stage(
        &fixture.store,
        AssignStageInput {
            run_id: &run.id,
            stage: "develop",
            cli: "agy",
            model: "gemini-2.5-pro",
            role: None,
            by: AssignmentSource::Orchestrator,
            actor_id: "orch-1",
        },
    )
    .unwrap_err();
    assert!(matches!(conflict, TeamError::Forbidden(_)));

    let saved_assignments = load_assignments(&fixture.store, &run.id).unwrap();
    assert_eq!(
        saved_assignments.stages["develop"].source,
        AssignmentSource::User
    );
    assert_eq!(saved_assignments.stages["plan"].cli, "agy");

    // 4. Drive workflow end to end using real tmux worker panes and interactive stubs.
    let mut exec = FixtureExecutor {
        broker: BrokerExecutor::new(&fixture.workspace)
            .with_tmux(TmuxClient::new(&socket, None))
            .with_login_probe(Arc::new(probe)),
        parent_env: env.clone(),
        terms: TEST_TERMS,
        today: FIXED_TODAY.to_string(),
    };

    for _ in 0..16 {
        let cur = get_run(&fixture.store, &run.id).unwrap().unwrap();
        if matches!(
            cur.state,
            RunState::Delivered | RunState::Paused | RunState::Cancelled | RunState::Failed
        ) {
            break;
        }
        drive_run_step_with_gh(
            &fixture.store,
            &fixture.workspace,
            &run.id,
            &mut exec,
            &fixture.gh_bin,
        )
        .unwrap();
    }

    let settled = get_run(&fixture.store, &run.id).unwrap().unwrap();
    let plan_log = fs::read_to_string(job_dir(&fixture.store, &run.id, "plan", 1).join("log.txt"))
        .unwrap_or_default();
    let plan_res =
        fs::read_to_string(job_dir(&fixture.store, &run.id, "plan", 1).join("result.json"))
            .unwrap_or_default();
    let failed_events: Vec<String> = read_all(&fixture.store.root.join("events.jsonl"))
        .map(|l| {
            l.events
                .into_iter()
                .filter(|e| e.r#type == "stage.failed")
                .map(|e| format!("{} {:?}", e.r#type, e.data))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        settled.state,
        RunState::Delivered,
        "run did not deliver cleanly: pause_reason={:?}, failed_events={failed_events:?}, stages={:?}, plan_log={plan_log:?}, plan_res={plan_res:?}",
        settled.pause_reason,
        settled.stages
    );
    assert!(!settled.prs.is_empty(), "expected PR created by stub gh");

    let run_dir = fixture.store.run_dir(&run.id);
    assert!(run_dir.join("handoff.md").is_file());
    assert!(repo_dir.exists());

    let ev_log = read_all(&fixture.store.root.join("events.jsonl")).unwrap();
    let cost_report = telemetry::run_usage_from_events(&settled, &ev_log.events);
    assert_eq!(cost_report.total_cost.basis, CostBasis::Unknown);
    assert!(ev_log.events.iter().any(|e| e.r#type == "job.started"));

    let plan_input: JobInput = serde_json::from_str(
        &fs::read_to_string(job_dir(&fixture.store, &run.id, "plan", 1).join("input.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(plan_input.provider, "agy");
    let dev_input: JobInput = serde_json::from_str(
        &fs::read_to_string(job_dir(&fixture.store, &run.id, "develop", 1).join("input.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(dev_input.provider, "claude");

    // 5. Verify live guarded tmux send/capture/relaunch/terminate and clean doctor diagnosis.
    let live_attempt = 99u32;
    let live_jdir = job_dir(&fixture.store, &run.id, "develop", live_attempt);
    fs::create_dir_all(&live_jdir).unwrap();
    let prompt_path = live_jdir.join("prompt.md");
    fs::write(&prompt_path, "Interactive live pane check STUB_STAY_ALIVE").unwrap();
    fs::write(
        live_jdir.join("input.json"),
        serde_json::to_string(&JobInput {
            correlation_id: format!("{}.develop.{live_attempt}", run.id),
            run: run.id.clone(),
            stage: "develop".to_string(),
            attempt: live_attempt,
            agent: "implementer".to_string(),
            provider: "claude".to_string(),
            model: "sonnet".to_string(),
            tool_grants: vec![],
            cwd: settled
                .worktree_path
                .clone()
                .unwrap_or_else(|| repo_dir.display().to_string()),
            time_limit_secs: 60,
            context_manifest: ContextManifest {
                readable_scope: Some("worktree".to_string()),
                ..ContextManifest::default()
            },
        })
        .unwrap(),
    )
    .unwrap();
    set_job_state(&live_jdir, JobState::Running).unwrap();

    let mut live_env = env.clone();
    live_env.insert("STUB_STAY_ALIVE".to_string(), "1".to_string());
    let launched = launch(
        &tmux,
        &WorkerLaunchSpec {
            run_id: run.id.clone(),
            stage: "develop".to_string(),
            attempt: live_attempt,
            job_dir: live_jdir.clone(),
            prompt_file: prompt_path,
            result_path: live_jdir.join("result.json"),
            worktree_path: Path::new(
                settled
                    .worktree_path
                    .as_deref()
                    .unwrap_or(&repo_dir.display().to_string()),
            )
            .to_path_buf(),
            assignment: WorkerAssignment {
                cli: "claude".to_string(),
                model: "sonnet".to_string(),
                read_only: false,
                tools: vec![],
                agent: "implementer".to_string(),
                role: "senior-backend-dev".to_string(),
            },
            env: live_env,
            time_limit_secs: 60,
        },
    )
    .unwrap();
    fs::write(live_jdir.join("pane_id"), format!("{}\n", launched.pane_id)).unwrap();

    let agents = list_agents(&fixture.store, &tmux, &run.id).unwrap();
    let live_cid = format!("{}.develop.{live_attempt}", run.id);
    assert!(
        agents.iter().any(|a| a.job_id == live_cid && a.pane_alive),
        "expected alive pane for {live_cid}, got: {agents:?}"
    );

    let msg_file = live_jdir.join("nudge.txt");
    fs::write(&msg_file, "PING_FROM_ORCHESTRATOR").unwrap();
    send_agent_message(&fixture.store, &tmux, &run.id, &live_cid, &msg_file).unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_ping = false;
    while Instant::now() < deadline {
        let cap = capture_agent_output(&fixture.store, &tmux, &run.id, &live_cid, 30).unwrap();
        if cap.contains("PING_FROM_ORCHESTRATOR") {
            saw_ping = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(saw_ping, "expected pasted message inside live worker pane");

    let relaunched = relaunch_agent(&fixture.store, &tmux, &run.id, &live_cid).unwrap();
    assert_ne!(relaunched.pane_id, launched.pane_id);
    terminate_agent(&fixture.store, &tmux, &run.id, &live_cid, &live_cid).unwrap();

    let diag = diagnose_with_tmux(&fixture.store, &fixture.workspace, &tmux).unwrap();
    assert!(
        diag.orphan_worker_panes.is_empty(),
        "expected no orphan worker panes after delivery and terminate: {:?}",
        diag.orphan_worker_panes
    );
}
