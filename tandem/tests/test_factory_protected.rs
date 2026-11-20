use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use std::sync::Arc;
use tandem::factory::approval::{Caller, MarkReadyOptions, mark_ready_with_options};
use tandem::factory::artifacts;
use tandem::factory::broker::gate::{AuthType, consent_provider};
use tandem::factory::broker::result::{JobResult, JobResultStatus};
use tandem::factory::checks::{set_checks, set_deploy_command};
use tandem::factory::events::read_all;
use tandem::factory::executor::BrokerExecutor;
use tandem::factory::login::LoginProbe;
use tandem::factory::machine::{Command as MachineCommand, EventData};
use tandem::factory::model::{Actor, ActorKind, FactorySettings, PrLink, RunState, StageState};
use tandem::factory::protected::{
    APPROVAL_TTL_SECS, ProtectedAction, ProtectedApproval, approve, load_run_approvals,
};
use tandem::factory::run::get_run;
use tandem::factory::runner::{append_run_events, drive_run_step, resume_paused_run_with_probe};

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
use tandem::factory::stages::deploy::{
    DeployBlockReason, DeployStageContract, check_deploy_preconditions, run_deploy_stage,
};
use tandem::factory::stages::{StageContext, StageContract};
use tandem::factory::store::FactoryStore;
use tandem::factory::workflow::DEFAULT_WORKFLOW_YAML;
use tandem::factory::workflow::parse::parse_workflow_str;
use tandem::factory::workflow::scheduler::{RunFacts, SchedulerLimits, TriState, next};
use tandem::factory::workflow::validate::validate;

struct TestWorkspace {
    root: PathBuf,
    state_dir: PathBuf,
}

impl TestWorkspace {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "tandem-factory-protected-{name}-{}-{nanos}",
            std::process::id()
        ));
        let root = base.join("ws");
        let state_dir = base.join("state");
        fs::create_dir_all(&root).unwrap();
        tandem::web_manager::save_web_config(
            &tandem::web_manager::web_config_path(&state_dir.join("tandem")),
            &tandem::web_manager::WebConfig {
                auth_token: "test-token".into(),
                ..Default::default()
            },
        )
        .unwrap();
        Self { root, state_dir }
    }

    fn run_cli(&self, args: &[&str], extra_env: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tandem"));
        cmd.args(args)
            .env("TANDEM_WORKSPACE", &self.root)
            .env("XDG_STATE_HOME", &self.state_dir)
            .env("TANDEM_AUTH_TOKEN", "test-token")
            .env("TANDEM_INTERNAL_SURFACE", "web")
            .env("TANDEM_FACTORY", "1")
            .env_remove("TANDEM_ROLE")
            .env_remove("TANDEM_SESSION_ID")
            .env_remove("TANDEM_FACTORY_IDEA")
            .env_remove("CLAUDECODE")
            .env_remove("CODEX_THREAD_ID")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        cmd.output().expect("run tandem")
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        if let Some(parent) = self.root.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

fn no_env(_: &str) -> Option<String> {
    None
}

fn agent_env(key: &str) -> Option<String> {
    if key == "TANDEM_ROLE" {
        Some("orchestrator".to_string())
    } else {
        None
    }
}

fn user_actor() -> Actor {
    Actor {
        kind: ActorKind::User,
        id: "alice".to_string(),
    }
}

fn orch_actor() -> Actor {
    Actor {
        kind: ActorKind::Orchestrator,
        id: "orch".to_string(),
    }
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn git_init_repo(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    for args in [
        vec!["init", "-b", "trunk"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test"],
    ] {
        let out = Command::new("git")
            .args(&args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {:?} failed", args);
    }
    fs::write(dir.join("README.md"), "# test\n").unwrap();
    let out = Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success());
}

fn setup_ready_idea_and_run(
    ws: &TestWorkspace,
    deploy_requested: bool,
) -> (FactoryStore, String, String) {
    git_init_repo(&ws.root);
    let store = FactoryStore::new(&ws.root);
    store.ensure().unwrap();
    let mut settings = store.settings().unwrap();
    settings.approval_grace_secs = 0;
    tandem::factory::settings::save(&store, &settings).unwrap();

    let caller = Caller::cli(true, &no_env, &|_| None);
    consent_provider(
        &store,
        "claude",
        AuthType::ApiKey,
        "claude",
        "alice",
        &caller,
    )
    .unwrap();

    let actor = user_actor();
    let orch = orch_actor();
    let idea = store
        .create_idea(
            "Protected deploy test",
            "Verify Gate 3 and deploy",
            "alice",
            &actor,
        )
        .unwrap();
    let idea_id = idea.id;
    store
        .execute(&idea_id, MachineCommand::StartDiscovery, &orch)
        .unwrap();

    let feas_file = ws.root.join("feas.md");
    fs::write(&feas_file, "# Feasibility\nFeasible.\n").unwrap();
    let feas_ref = artifacts::put(&store, &idea_id, "feasibility", &feas_file).unwrap();
    store
        .execute(
            &idea_id,
            MachineCommand::StoreArtifact {
                kind: "feasibility".to_string(),
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
        .execute(&idea_id, MachineCommand::SelectDirection, &orch)
        .unwrap();

    let plan_file = ws.root.join("plan.md");
    fs::write(
        &plan_file,
        "# Plan\n\n## Goal\nok\n\n## Acceptance Criteria\n- works\n\n## Files\n- README.md\n\n## Tests\n- cargo test\n",
    )
    .unwrap();
    let plan_ref = artifacts::put(&store, &idea_id, "plan", &plan_file).unwrap();
    store
        .execute(
            &idea_id,
            MachineCommand::StoreArtifact {
                kind: "plan".to_string(),
                artifact: plan_ref,
                verdict: None,
                risk: None,
                workflow: Some("default-delivery".to_string()),
                missing_sections: vec![],
            },
            &orch,
        )
        .unwrap();

    let ready = mark_ready_with_options(
        &store,
        &ws.root,
        &idea_id,
        &idea_id,
        "alice",
        MarkReadyOptions {
            deploy_requested,
            accept_estimate: true,
            ..Default::default()
        },
        &caller,
    )
    .unwrap();
    let run_id = ready.run.expect("run created").id;
    (store, idea_id, run_id)
}

#[test]
fn protected_action_parses_all_three_variants() {
    assert_eq!(
        ProtectedAction::parse("production_deploy"),
        Some(ProtectedAction::ProductionDeploy)
    );
    assert_eq!(
        ProtectedAction::parse("secrets_access"),
        Some(ProtectedAction::SecretsAccess)
    );
    assert_eq!(
        ProtectedAction::parse("production_access"),
        Some(ProtectedAction::ProductionAccess)
    );
    assert_eq!(ProtectedAction::parse("unknown"), None);

    assert_eq!(
        ProtectedAction::ProductionDeploy.as_str(),
        "production_deploy"
    );
    assert_eq!(ProtectedAction::SecretsAccess.as_str(), "secrets_access");
    assert_eq!(
        ProtectedAction::ProductionAccess.as_str(),
        "production_access"
    );
}

#[test]
fn approval_matches_checks_run_stage_hash_expiry_and_single_use() {
    let approval = ProtectedApproval {
        seq: 1,
        gate: 3,
        action: ProtectedAction::ProductionDeploy,
        run: "r-11111111".to_string(),
        stage: "deploy".to_string(),
        workflow_sha256: "abc123".to_string(),
        actor: user_actor(),
        surface: "cli".to_string(),
        granted_at: 1000.0,
        expires_at: 1000.0 + APPROVAL_TTL_SECS,
        used: false,
    };

    assert!(approval.matches("r-11111111", "deploy", "abc123", 1000.0));
    assert!(approval.matches("r-11111111", "deploy", "abc123", 4599.0));
    assert!(!approval.matches("r-22222222", "deploy", "abc123", 1500.0));
    assert!(!approval.matches("r-11111111", "review", "abc123", 1500.0));
    assert!(!approval.matches("r-11111111", "deploy", "wrong_hash", 1500.0));
    assert!(!approval.matches("r-11111111", "deploy", "abc123", 4600.0));

    let mut used = approval.clone();
    used.used = true;
    assert!(!used.matches("r-11111111", "deploy", "abc123", 1500.0));
}

#[test]
fn run_approve_enforces_user_only_confirm_and_confirmed_deploy_command() {
    let ws = TestWorkspace::new("approve-checks");
    let (store, _idea_id, run_id) = setup_ready_idea_and_run(&ws, true);

    let agent_caller = Caller::cli(true, &agent_env, &|_| None);
    let user_caller = Caller::cli(true, &no_env, &|_| None);

    let agent_err = approve(
        &store,
        &run_id,
        ProtectedAction::ProductionDeploy,
        &run_id,
        None,
        "alice",
        &agent_caller,
    )
    .unwrap_err();
    assert_eq!(agent_err.exit_code(), 3);

    let confirm_err = approve(
        &store,
        &run_id,
        ProtectedAction::ProductionDeploy,
        "wrong-id",
        None,
        "alice",
        &user_caller,
    )
    .unwrap_err();
    assert!(confirm_err.to_string().contains("--confirm"));

    let no_cmd_err = approve(
        &store,
        &run_id,
        ProtectedAction::ProductionDeploy,
        &run_id,
        None,
        "alice",
        &user_caller,
    )
    .unwrap_err();
    assert!(
        no_cmd_err
            .to_string()
            .contains("no deploy command is confirmed"),
        "unexpected error: {no_cmd_err}"
    );

    let sec_approval = approve(
        &store,
        &run_id,
        ProtectedAction::SecretsAccess,
        &run_id,
        Some("develop"),
        "alice",
        &user_caller,
    )
    .unwrap();
    assert_eq!(sec_approval.gate, 3);
    assert_eq!(sec_approval.action, ProtectedAction::SecretsAccess);
    assert_eq!(sec_approval.stage, "develop");

    set_deploy_command(
        &store,
        "workspace",
        vec!["echo".to_string(), "deployed".to_string()],
        "workspace",
        "alice",
        &user_caller,
    )
    .unwrap();

    let granted = approve(
        &store,
        &run_id,
        ProtectedAction::ProductionDeploy,
        &run_id,
        None,
        "alice",
        &user_caller,
    )
    .unwrap();
    assert_eq!(granted.gate, 3);
    assert_eq!(granted.action, ProtectedAction::ProductionDeploy);
    assert_eq!(granted.run, run_id);
    assert_eq!(granted.stage, "deploy");
    assert_eq!(granted.surface, "cli");
    assert!((granted.expires_at - granted.granted_at - 3600.0).abs() < 1.0);

    let log = read_all(&store.root.join("events.jsonl")).unwrap();
    let ev = log
        .events
        .iter()
        .rev()
        .find(|e| e.r#type == "approval.granted")
        .expect("approval.granted event recorded");
    assert_eq!(ev.data["gate"], 3);
    assert_eq!(ev.data["action"], "production_deploy");
    assert_eq!(ev.data["run"], run_id);
    assert_eq!(ev.data["stage"], "deploy");
    assert_eq!(ev.data["surface"], "cli");
    assert!(
        ev.data["workflow_sha256"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
    assert!(ev.data["expires_at"].as_f64().is_some());
}

#[test]
fn cli_run_approve_and_checks_set_deploy() {
    let ws = TestWorkspace::new("cli-approve");
    let (_store, _idea_id, run_id) = setup_ready_idea_and_run(&ws, true);

    let out = ws.run_cli(
        &[
            "factory",
            "run",
            "approve",
            &run_id,
            "--action",
            "production_deploy",
            "--confirm",
            &run_id,
        ],
        &[("TANDEM_ROLE", "implementer")],
    );
    assert_eq!(out.status.code(), Some(3));

    let out = ws.run_cli(
        &[
            "factory",
            "run",
            "approve",
            &run_id,
            "--action",
            "production_deploy",
            "--confirm",
            &run_id,
        ],
        &[],
    );
    assert_ne!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no deploy command is confirmed"),
        "stderr: {stderr}"
    );

    let bad_shell = ws.run_cli(
        &[
            "factory",
            "checks",
            "set",
            "--target",
            "workspace",
            "--deploy",
            r#"["sh","-c","echo hi"]"#,
            "--confirm",
            "workspace",
        ],
        &[],
    );
    assert_ne!(bad_shell.status.code(), Some(0));

    let set_out = ws.run_cli(
        &[
            "factory",
            "checks",
            "set",
            "--target",
            "workspace",
            "--deploy",
            r#"["echo","deploy-ok"]"#,
            "--confirm",
            "workspace",
        ],
        &[],
    );
    assert!(
        set_out.status.success(),
        "checks set --deploy failed: {}",
        String::from_utf8_lossy(&set_out.stderr)
    );

    let ok_out = ws.run_cli(
        &[
            "factory",
            "run",
            "approve",
            &run_id,
            "--action",
            "production_deploy",
            "--confirm",
            &run_id,
        ],
        &[],
    );
    assert!(
        ok_out.status.success(),
        "run approve failed: {}",
        String::from_utf8_lossy(&ok_out.stderr)
    );
    let stdout = String::from_utf8_lossy(&ok_out.stdout);
    assert!(stdout.contains("echo deploy-ok"), "stdout: {stdout}");
    assert!(stdout.contains("60 minutes"), "stdout: {stdout}");
}

#[test]
fn scheduler_blocks_deploy_with_command_until_approved_and_skips_by_default() {
    let raw = parse_workflow_str(DEFAULT_WORKFLOW_YAML).unwrap();
    let wf = validate(&raw, &FactorySettings::default()).unwrap();

    let stages: Vec<tandem::factory::model::Stage> = wf
        .stages
        .iter()
        .map(|s| {
            let state = match s.name.as_str() {
                "plan" | "create-tests" | "develop" | "test" | "review" => StageState::Succeeded,
                "demo" => StageState::Skipped,
                _ => StageState::Pending,
            };
            tandem::factory::model::Stage {
                name: s.name.clone(),
                agent: s.agent.as_str().to_string(),
                state,
                attempts: 0,
                interrupted_attempts: 0,
                started: None,
                finished: None,
                artifacts: vec![],
                rationale: None,
                retry_hypothesis: None,
            }
        })
        .collect();

    let default_decision = next(
        &wf,
        &stages,
        &RunFacts {
            ui_changed: TriState::No,
            deploy_requested: false,
            approvals: vec![],
        },
        &SchedulerLimits::from_workflow_and_settings(&wf, &FactorySettings::default(), &stages),
    );
    assert!(
        default_decision.skip.iter().any(|s| s.name == "deploy"),
        "deploy must be skipped when deploy_requested is false"
    );

    let blocked_decision = next(
        &wf,
        &stages,
        &RunFacts {
            ui_changed: TriState::No,
            deploy_requested: true,
            approvals: vec![],
        },
        &SchedulerLimits::from_workflow_and_settings(&wf, &FactorySettings::default(), &stages),
    );
    let blocked = blocked_decision
        .blocked
        .iter()
        .find(|b| b.name == "deploy")
        .expect("deploy must be blocked");
    assert!(
        blocked.reason.contains("tandem factory run approve"),
        "blocked reason should show command: {}",
        blocked.reason
    );
}

#[test]
fn deploy_stage_requires_pr_confirmed_command_and_single_use_approval_and_records_redacted_evidence()
 {
    let ws = TestWorkspace::new("deploy-e2e");
    let (store, idea_id, run_id) = setup_ready_idea_and_run(&ws, true);

    let caller = Caller::cli(true, &no_env, &|_| None);
    set_checks(
        &store,
        "workspace",
        vec![vec!["true".to_string()]],
        vec![],
        None,
        vec![],
        "workspace",
        "alice",
        &caller,
    )
    .unwrap();

    let sys = Actor {
        kind: ActorKind::System,
        id: "test".to_string(),
    };
    let mut run = get_run(&store, &run_id).unwrap().unwrap();
    let wt = ws.root.join("wt");
    fs::create_dir_all(&wt).unwrap();
    run.worktree_path = Some(wt.to_string_lossy().to_string());
    run.target = Some("workspace".to_string());
    for st in &mut run.stages {
        if matches!(
            st.name.as_str(),
            "plan" | "create-tests" | "develop" | "test" | "review"
        ) {
            st.state = StageState::Succeeded;
        } else if st.name == "demo" {
            st.state = StageState::Skipped;
        }
    }
    fs::write(
        store.run_dir(&run_id).join("run.json"),
        serde_json::to_vec_pretty(&run).unwrap(),
    )
    .unwrap();

    let mut exec = BrokerExecutor::new(&ws.root).with_login_probe(Arc::new(ReadyClaudeProbe));
    drive_run_step(&store, &ws.root, &run_id, &mut exec).unwrap();
    let paused = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(paused.state, RunState::Paused);
    assert!(
        paused
            .action_commands
            .iter()
            .any(|c| c.contains("tandem factory run approve") && c.contains("production_deploy")),
        "expected approve command in {:?}",
        paused.action_commands
    );

    set_deploy_command(
        &store,
        "workspace",
        vec![
            "python3".to_string(),
            "-c".to_string(),
            "print('deploy ok token=sk-ant-api03-1234567890abcdef1234567890abcdef')".to_string(),
        ],
        "workspace",
        "alice",
        &caller,
    )
    .unwrap();

    approve(
        &store,
        &run_id,
        ProtectedAction::ProductionDeploy,
        &run_id,
        None,
        "alice",
        &caller,
    )
    .unwrap();

    let settings = store.settings().unwrap();
    let approvals = load_run_approvals(&store, &run_id).unwrap();
    let pre_err =
        check_deploy_preconditions(&paused, &settings, &approvals, now_secs()).unwrap_err();
    assert_eq!(pre_err, DeployBlockReason::MissingPullRequest);

    resume_paused_run_with_probe(&store, &run_id, &user_actor(), &ReadyClaudeProbe).unwrap();
    drive_run_step(&store, &ws.root, &run_id, &mut exec).unwrap();
    let paused_no_pr = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(paused_no_pr.state, RunState::Paused);
    assert!(
        paused_no_pr
            .pause_reason
            .as_deref()
            .unwrap_or("")
            .contains("pull request"),
        "expected missing PR pause reason, got {:?}",
        paused_no_pr.pause_reason
    );

    append_run_events(
        &store,
        &idea_id,
        &run_id,
        None,
        vec![EventData::PrCreated {
            run: run_id.clone(),
            pr: PrLink {
                repo: "org/repo".to_string(),
                number: 42,
                url: "https://github.com/org/repo/pull/42".to_string(),
                state: Some("open".to_string()),
                checks_summary: Some("passed".to_string()),
            },
        }],
        &sys,
    )
    .unwrap();

    resume_paused_run_with_probe(&store, &run_id, &user_actor(), &ReadyClaudeProbe).unwrap();
    drive_run_step(&store, &ws.root, &run_id, &mut exec).unwrap();

    let after_deploy = get_run(&store, &run_id).unwrap().unwrap();
    let deploy_stage = after_deploy
        .stages
        .iter()
        .find(|s| s.name == "deploy")
        .unwrap();
    assert_eq!(deploy_stage.state, StageState::Succeeded);

    let deploy_ev = after_deploy
        .evidence
        .iter()
        .find(|e| e.stage == "deploy")
        .expect("deploy evidence recorded");
    let ev_content = fs::read_to_string(store.root.join(&deploy_ev.path)).unwrap();
    assert!(
        !ev_content.contains("sk-ant-api03-1234567890abcdef"),
        "secret must be redacted from deploy evidence: {ev_content}"
    );
    assert!(ev_content.contains("exit_code"));
    assert!(ev_content.contains("python3"));

    let approvals_after = load_run_approvals(&store, &run_id).unwrap();
    assert_eq!(approvals_after.len(), 1);
    assert!(
        approvals_after[0].used,
        "approval must be consumed after stage executes"
    );

    append_run_events(
        &store,
        &idea_id,
        &run_id,
        None,
        vec![EventData::StageReopened {
            run: run_id.clone(),
            stage: "deploy".to_string(),
            reason: "re-deploy".to_string(),
        }],
        &sys,
    )
    .unwrap();
    drive_run_step(&store, &ws.root, &run_id, &mut exec).unwrap();
    let paused_retry = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(paused_retry.state, RunState::Paused);
    assert!(
        paused_retry
            .action_commands
            .iter()
            .any(|c| c.contains("tandem factory run approve")),
        "retried deploy must require a new approval"
    );
}

#[test]
fn deploy_contract_preconditions_block_reasons_and_failure_exit_code() {
    let ws = TestWorkspace::new("deploy-edges");
    let wt = ws.root.join("wt");
    let art = ws.root.join("art");
    fs::create_dir_all(&wt).unwrap();
    fs::create_dir_all(&art).unwrap();

    let empty_result = JobResult {
        status: JobResultStatus::Succeeded,
        summary: String::new(),
        files_changed: vec![],
        artifacts: vec![],
        rationale: None,
        hypothesis: None,
        usage: None,
        no_behaviour_change: false,
    };
    let contract = DeployStageContract;
    assert_eq!(contract.name(), "deploy");

    let mut ctx = StageContext {
        run_id: "r-1",
        idea_id: "i-1",
        stage: "deploy",
        attempt: 1,
        has_repo_write: true,
        artifact_dir: &art,
        target_repo: &ws.root,
        worktree_path: &wt,
        plan_snapshot_path: &ws.root,
        confirmed_checks: None,
        result: &empty_result,
    };
    let err = contract.verify(&ctx).unwrap_err();
    assert_eq!(err.check, "deploy_read_only");
    assert!(err.detail.contains("repo_write"));

    ctx.has_repo_write = false;
    assert!(contract.verify(&ctx).is_ok());

    let reasons = [
        (
            DeployBlockReason::DeployNotRequested,
            "not requested",
            false,
        ),
        (DeployBlockReason::MissingPullRequest, "pull request", true),
        (
            DeployBlockReason::NoDeployCommandConfirmed("app".to_string()),
            "app",
            true,
        ),
        (
            DeployBlockReason::MissingApproval,
            "production_deploy",
            true,
        ),
    ];
    for (reason, expected_msg, has_cmds) in reasons {
        assert!(reason.message().contains(expected_msg));
        let cmds = reason.action_commands("r-1", "app");
        assert_eq!(!cmds.is_empty(), has_cmds);
    }

    let (store, _idea_id, run_id) = setup_ready_idea_and_run(&ws, false);
    let mut run = get_run(&store, &run_id).unwrap().unwrap();
    let settings = FactorySettings::default();
    assert_eq!(
        check_deploy_preconditions(&run, &settings, &[], now_secs()).unwrap_err(),
        DeployBlockReason::DeployNotRequested
    );

    run.deploy_requested = true;
    run.target = Some("workspace".to_string());
    assert_eq!(
        check_deploy_preconditions(&run, &settings, &[], now_secs()).unwrap_err(),
        DeployBlockReason::MissingPullRequest
    );

    run.prs.push(PrLink {
        repo: "org/repo".to_string(),
        number: 7,
        url: "https://github.com/org/repo/pull/7".to_string(),
        state: Some("open".to_string()),
        checks_summary: None,
    });
    assert_eq!(
        check_deploy_preconditions(&run, &settings, &[], now_secs()).unwrap_err(),
        DeployBlockReason::NoDeployCommandConfirmed("workspace".to_string())
    );

    let caller = Caller::cli(true, &no_env, &|_| None);
    set_deploy_command(
        &store,
        "workspace",
        vec![
            "python3".to_string(),
            "-c".to_string(),
            "import sys; sys.exit(2)".to_string(),
        ],
        "workspace",
        "alice",
        &caller,
    )
    .unwrap();
    let settings_with_deploy = store.settings().unwrap();
    assert_eq!(
        check_deploy_preconditions(&run, &settings_with_deploy, &[], now_secs()).unwrap_err(),
        DeployBlockReason::MissingApproval
    );

    approve(
        &store,
        &run_id,
        ProtectedAction::ProductionDeploy,
        &run_id,
        None,
        "alice",
        &caller,
    )
    .unwrap();
    let approvals = load_run_approvals(&store, &run_id).unwrap();
    let confirmed =
        check_deploy_preconditions(&run, &settings_with_deploy, &approvals, now_secs()).unwrap();

    let report = run_deploy_stage(&confirmed, &wt, &art, 1).unwrap();
    assert!(!report.passed);
    assert_eq!(report.exit_code, Some(2));
    assert!(report.summary.contains("exited with"));
}
