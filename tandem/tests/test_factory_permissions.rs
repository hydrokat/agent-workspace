use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tandem::factory::approval::mark_ready_with_workflow;
use tandem::factory::artifacts;
use tandem::factory::broker::context::{ContextInputs, assemble_context};
use tandem::factory::broker::gate::{AuthType, consent_provider};
use tandem::factory::broker::job::{JobSpec, prepare_job};
use tandem::factory::events::read_all;
use tandem::factory::executor::{Executor, StageJob, StageOutcome};
use tandem::factory::machine::{Command as MachineCommand, EventData};
use tandem::factory::model::{Actor, ActorKind, FactorySettings, RunState};
use tandem::factory::permissions::{DeniedReason, ToolGrant, grant};
use tandem::factory::routing::{RoutingContext, route};
use tandem::factory::run::get_run;
use tandem::factory::runner::{drive_run_step, resume_paused_run};
use tandem::factory::settings::save as save_settings;
use tandem::factory::stages::scope::verify_scope_with_grant;
use tandem::factory::store::FactoryStore;
use tandem::factory::workflow::DEFAULT_WORKFLOW_YAML;
use tandem::factory::workflow::parse::parse_workflow_str;
use tandem::factory::workflow::schema::{Workflow, WorkflowAgent, WorkflowTool};
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
            "tandem-factory-perms-{name}-{}-{nanos}",
            std::process::id()
        ));
        let root = base.join("ws");
        let state_dir = base.join("state");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&state_dir).unwrap();
        Self { root, state_dir }
    }

    fn run_cli(&self, args: &[&str], extra_env: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tandem"));
        cmd.args(args)
            .env("TANDEM_WORKSPACE", &self.root)
            .env("XDG_STATE_HOME", &self.state_dir)
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

fn default_wf() -> Workflow {
    let raw = parse_workflow_str(DEFAULT_WORKFLOW_YAML).unwrap();
    validate(&raw, &FactorySettings::default()).unwrap()
}

fn git_init_repo(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    for args in [
        vec!["init"],
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

struct NoopExecutor;

impl Executor for NoopExecutor {
    fn run(&mut self, _job: &StageJob) -> StageOutcome {
        StageOutcome::Succeeded
    }
}

fn setup_queued_run(ws: &Path) -> (FactoryStore, String, String) {
    git_init_repo(ws);
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();
    let user = Actor {
        kind: ActorKind::User,
        id: "alice".to_string(),
    };
    let orch = Actor {
        kind: ActorKind::Orchestrator,
        id: "orch".to_string(),
    };
    consent_provider(
        &store,
        "claude",
        AuthType::ApiKey,
        "claude",
        "alice",
        &|_| None,
    )
    .unwrap();
    let idea = store
        .create_idea("Perm test", "Testing permissions", "alice", &user)
        .unwrap();
    store
        .execute(&idea.id, MachineCommand::StartDiscovery, &orch)
        .unwrap();
    let feas = ws.join("f.md");
    fs::write(&feas, "# Feasible\n").unwrap();
    let feas_ref = artifacts::put(&store, &idea.id, "feasibility", &feas).unwrap();
    store
        .execute(
            &idea.id,
            MachineCommand::StoreArtifact {
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
        .execute(&idea.id, MachineCommand::SelectDirection, &orch)
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
            MachineCommand::StoreArtifact {
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

#[test]
fn default_settings_grant_exact_same_tools_as_default_delivery() {
    let wf = default_wf();
    let settings = FactorySettings::default();
    let ctx = RoutingContext {
        preferred_provider: Some("claude".into()),
        ..Default::default()
    };

    for stage in &wf.stages {
        let has_gate3 = stage.name == "deploy";
        let granted = grant(stage.agent, stage, "workspace", &wf, &settings, has_gate3)
            .unwrap_or_else(|e| panic!("stage {} should be granted: {e}", stage.name));

        assert_eq!(granted.tools, stage.tools, "stage {}", stage.name);
        let expect_write = stage.tools.contains(&WorkflowTool::RepoWrite);
        assert_eq!(granted.has_repo_write, expect_write);
        assert_eq!(granted.read_only, !expect_write);

        if stage.name != "demo" {
            let routed = route(stage, &granted, &wf, &settings, &["claude"], 1, &ctx).unwrap();
            assert!(!routed.tool_flags.is_empty());
        }
    }
}

#[test]
fn stages_without_write_class_tool_get_read_only_grant() {
    let wf = default_wf();
    let settings = FactorySettings::default();

    for name in ["plan", "test", "review", "demo"] {
        let stage = wf.stages.iter().find(|s| s.name == name).unwrap();
        let granted = grant(stage.agent, stage, "workspace", &wf, &settings, false).unwrap();
        assert!(granted.read_only, "{name} must be read_only");
        assert!(!granted.has_repo_write, "{name} must not have repo_write");
    }

    let develop = wf.stages.iter().find(|s| s.name == "develop").unwrap();
    let granted = grant(develop.agent, develop, "workspace", &wf, &settings, false).unwrap();
    assert!(!granted.read_only);
    assert!(granted.has_repo_write);
}

#[test]
fn ci_and_deployment_require_gate3_approval() {
    let wf = default_wf();
    let settings = FactorySettings::default();
    let deploy = wf.stages.iter().find(|s| s.name == "deploy").unwrap();

    let err = grant(deploy.agent, deploy, "workspace", &wf, &settings, false).unwrap_err();
    assert!(matches!(err.reason, DeniedReason::Gate3Required));
    assert!(
        err.to_string()
            .starts_with("policy denied ci for release-engineer in deploy")
            || err
                .to_string()
                .starts_with("policy denied deployment for release-engineer in deploy")
    );

    let ok = grant(deploy.agent, deploy, "workspace", &wf, &settings, true).unwrap();
    assert_eq!(ok.tools, vec![WorkflowTool::Ci, WorkflowTool::Deployment]);
    assert!(ok.read_only);
}

#[test]
fn intersection_denies_and_never_silently_drops_tools() {
    let wf = default_wf();
    let develop = wf.stages.iter().find(|s| s.name == "develop").unwrap();

    // Narrow allowed_tools for implementer -> removes shell
    let mut settings = FactorySettings::default();
    settings.allowed_tools.insert(
        "implementer".to_string(),
        vec![WorkflowTool::RepoRead, WorkflowTool::RepoWrite],
    );
    let err = grant(develop.agent, develop, "workspace", &wf, &settings, false).unwrap_err();
    assert_eq!(err.tool, WorkflowTool::Shell);
    assert_eq!(
        err.to_string(),
        "policy denied shell for implementer in develop"
    );

    // Deny repo_write for target "restricted-repo"
    let mut settings2 = FactorySettings::default();
    settings2
        .denied_tools
        .insert("restricted-repo".to_string(), vec![WorkflowTool::RepoWrite]);
    let err2 = grant(
        develop.agent,
        develop,
        "restricted-repo",
        &wf,
        &settings2,
        false,
    )
    .unwrap_err();
    assert_eq!(err2.tool, WorkflowTool::RepoWrite);
    assert_eq!(
        err2.to_string(),
        "policy denied repo_write for implementer in develop"
    );

    // Custom workflow asks for repo_write on reviewer, whose default allowed_tools lacks it
    let mut custom_wf = wf.clone();
    let mut review_stage = wf
        .stages
        .iter()
        .find(|s| s.name == "review")
        .unwrap()
        .clone();
    review_stage.tools.push(WorkflowTool::RepoWrite);
    custom_wf.stages = vec![review_stage.clone()];
    let err3 = grant(
        review_stage.agent,
        &review_stage,
        "workspace",
        &custom_wf,
        &FactorySettings::default(),
        false,
    )
    .unwrap_err();
    assert_eq!(err3.tool, WorkflowTool::RepoWrite);
    assert_eq!(
        err3.to_string(),
        "policy denied repo_write for reviewer in review"
    );
}

#[test]
fn routing_uses_tool_grant_not_raw_workflow_tools() {
    let wf = default_wf();
    let settings = FactorySettings::default();
    let develop = wf.stages.iter().find(|s| s.name == "develop").unwrap();

    let read_only_grant = ToolGrant {
        agent: WorkflowAgent::Implementer,
        stage: "develop".to_string(),
        target: "workspace".to_string(),
        tools: vec![WorkflowTool::RepoRead],
        read_only: true,
        has_repo_write: false,
    };
    let routed = route(
        develop,
        &read_only_grant,
        &wf,
        &settings,
        &["claude"],
        1,
        &RoutingContext::default(),
    )
    .unwrap();
    assert!(!routed.tool_flags.iter().any(|f| {
        matches!(f, tandem::adapters::base::ToolFlag::Allow(name) if name == "Edit" || name == "Write" || name == "Bash")
    }));
}

#[test]
fn context_records_grant_in_input_json_and_scope_enforces_read_only() {
    let ws = TestWorkspace::new("ctx-scope");
    git_init_repo(&ws.root);
    let store = FactoryStore::new(&ws.root);
    store.ensure().unwrap();

    let plan = ws.state_dir.join("plan.md");
    let playbook = ws.state_dir.join("playbook.md");
    let agent_def = ws.state_dir.join("agent.md");
    fs::write(&plan, "# Plan\n").unwrap();
    fs::write(&playbook, "# Playbook\n").unwrap();
    fs::write(&agent_def, "# Agent\n").unwrap();

    let grant = ToolGrant {
        agent: WorkflowAgent::Orchestrator,
        stage: "plan".to_string(),
        target: "workspace".to_string(),
        tools: vec![WorkflowTool::RepoRead, WorkflowTool::IssueContext],
        read_only: true,
        has_repo_write: false,
    };

    let assembled = assemble_context(&ContextInputs {
        plan_snapshot_path: plan,
        acceptance_criteria: vec![],
        playbook_path: playbook,
        agent_def_path: agent_def,
        dependency_summaries: vec![],
        retry_rationale: None,
        tool_grant: Some(grant.clone()),
    })
    .unwrap();

    let job = prepare_job(
        &store,
        &JobSpec {
            run: "r-perm0001".to_string(),
            stage: "plan".to_string(),
            attempt: 1,
            agent: "tech-lead-orchestrator".to_string(),
            provider: "claude".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            tool_grants: grant.tools.iter().map(|t| t.as_str().to_string()).collect(),
            cwd: ws.root.to_string_lossy().to_string(),
            time_limit_secs: 600,
        },
        &assembled,
    )
    .unwrap();

    let input_raw = fs::read_to_string(job.dir.join("input.json")).unwrap();
    let input_json: Value = serde_json::from_str(&input_raw).unwrap();
    assert_eq!(
        input_json["context_manifest"]["tool_grant"]["read_only"],
        Value::Bool(true)
    );
    assert_eq!(
        input_json["context_manifest"]["tool_grant"]["stage"],
        Value::String("plan".to_string())
    );

    // Scope check passes when clean, fails when read-only stage modifies a file
    let artifact_dir = ws.state_dir.join("artifacts");
    fs::create_dir_all(&artifact_dir).unwrap();
    assert!(verify_scope_with_grant(&ws.root, &artifact_dir, &grant).is_ok());

    fs::write(ws.root.join("unexpected.txt"), "oops\n").unwrap();
    let scope_err = verify_scope_with_grant(&ws.root, &artifact_dir, &grant).unwrap_err();
    assert_eq!(scope_err.check, "scope_readonly");
}

#[test]
fn runner_pauses_with_action_request_on_policy_denial_and_records_grant_on_start() {
    let ws = TestWorkspace::new("runner-deny");
    let (store, _idea_id, run_id) = setup_queued_run(&ws.root);

    // Deny issue_context on target "workspace" -> plan stage must pause immediately
    let mut settings = store.settings().unwrap();
    settings
        .denied_tools
        .insert("workspace".to_string(), vec![WorkflowTool::IssueContext]);
    save_settings(&store, &settings).unwrap();

    let mut exec = NoopExecutor;
    drive_run_step(&store, &ws.root, &run_id, &mut exec).unwrap();

    let run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(run.state, RunState::Paused);
    assert_eq!(
        run.pause_reason.as_deref(),
        Some("policy denied issue_context for orchestrator in plan")
    );
    assert!(
        !run.action_commands.is_empty(),
        "policy denial must include a clear action request"
    );

    // Clear denial, resume, and verify stage.started carries the ToolGrant
    let mut settings = store.settings().unwrap();
    settings.denied_tools.clear();
    save_settings(&store, &settings).unwrap();

    let user = Actor {
        kind: ActorKind::User,
        id: "alice".to_string(),
    };
    resume_paused_run(&store, &run_id, &user).unwrap();

    drive_run_step(&store, &ws.root, &run_id, &mut exec).unwrap();

    let events = read_all(&store.root.join("events.jsonl")).unwrap().events;
    let started_grant = events.iter().find_map(|ev| match ev.to_event_data() {
        Ok(EventData::StageStarted {
            stage,
            grant: Some(g),
            ..
        }) if stage == "plan" => Some(g),
        _ => None,
    });
    let g = started_grant.expect("stage.started must carry ToolGrant");
    assert_eq!(
        g.tools,
        vec![WorkflowTool::RepoRead, WorkflowTool::IssueContext]
    );
    assert!(g.read_only);
}

#[test]
fn cli_permissions_allow_deny_show_are_user_only() {
    let ws = TestWorkspace::new("cli-perms");

    // Agent caller is rejected on allow and deny
    let out_agent_allow = ws.run_cli(
        &[
            "factory",
            "permissions",
            "allow",
            "implementer",
            "repo_read",
        ],
        &[("TANDEM_ROLE", "implementer")],
    );
    assert_eq!(out_agent_allow.status.code(), Some(3));

    let out_agent_deny = ws.run_cli(
        &["factory", "permissions", "deny", "workspace", "shell"],
        &[("TANDEM_ROLE", "implementer")],
    );
    assert_eq!(out_agent_deny.status.code(), Some(3));

    // User caller succeeds on allow, deny, and show
    let out_deny = ws.run_cli(
        &[
            "factory",
            "permissions",
            "deny",
            "workspace",
            "shell",
            "--json",
        ],
        &[],
    );
    assert!(
        out_deny.status.success(),
        "deny failed: {}",
        String::from_utf8_lossy(&out_deny.stderr)
    );

    let out_allow = ws.run_cli(
        &[
            "factory",
            "permissions",
            "allow",
            "implementer",
            "repo_read",
            "--json",
        ],
        &[],
    );
    assert!(
        out_allow.status.success(),
        "allow failed: {}",
        String::from_utf8_lossy(&out_allow.stderr)
    );

    let out_show = ws.run_cli(&["factory", "permissions", "show", "--json"], &[]);
    assert!(out_show.status.success());
    let shown: Value = serde_json::from_slice(&out_show.stdout).unwrap();
    let denied_ws = shown["denied_tools"]["workspace"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(denied_ws.contains(&"shell"));

    // Verify allow only narrows what workflows ask for: even if allowed_tools includes repo_write
    // for reviewer, default-delivery review stage does not ask for it, so grant never includes it.
    let mut settings = FactorySettings::default();
    let mut map = BTreeMap::new();
    map.insert(
        "reviewer".to_string(),
        vec![
            WorkflowTool::RepoRead,
            WorkflowTool::DiffReview,
            WorkflowTool::RepoWrite,
        ],
    );
    settings.allowed_tools = map;
    let wf = default_wf();
    let review = wf.stages.iter().find(|s| s.name == "review").unwrap();
    let granted = grant(review.agent, review, "workspace", &wf, &settings, false).unwrap();
    assert!(!granted.tools.contains(&WorkflowTool::RepoWrite));
    assert!(granted.read_only);
}
