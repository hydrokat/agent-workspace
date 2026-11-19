use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tandem::factory::broker::context::{ContextError, ContextInputs, assemble_context_with_policy};
use tandem::factory::broker::job::{JobSpec, prepare_job};
use tandem::factory::data_policy::{
    DataClass, PROVIDER_LIMITATION_SENTENCE, build_run_data, check_slices_against_exclusions,
    classify_path,
};
use tandem::factory::model::{Actor, ActorKind, FactorySettings, ProviderGateState};
use tandem::factory::permissions::ToolGrant;
use tandem::factory::routing::{RoutingContext, Unroutable, route};
use tandem::factory::settings::{save as save_settings, validate as validate_settings};
use tandem::factory::store::FactoryStore;
use tandem::factory::workflow::DEFAULT_WORKFLOW_YAML;
use tandem::factory::workflow::parse::parse_workflow_str;
use tandem::factory::workflow::validate::validate as validate_workflow;
use tandem::factory::workspace::prepare_worktree;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-data-policy-{}-{}-{}",
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

fn default_wf() -> tandem::factory::workflow::schema::Workflow {
    let raw = parse_workflow_str(DEFAULT_WORKFLOW_YAML).unwrap();
    validate_workflow(&raw, &FactorySettings::default()).unwrap()
}

fn init_git_repo(dir: &Path) {
    let run_git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run_git(&["init", "-b", "main"]);
    run_git(&["config", "user.email", "test@example.com"]);
    run_git(&["config", "user.name", "Test User"]);
}

fn commit_all(dir: &Path, msg: &str) {
    let run_git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git command");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run_git(&["add", "-A"]);
    run_git(&["commit", "-m", msg]);
}

fn setup_queued_run(ws: &Path) -> (FactoryStore, String) {
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
        .create_idea("Data policy test", "Verify data handling", "alice", &user)
        .unwrap();
    store
        .execute(
            &idea.id,
            tandem::factory::machine::Command::StartDiscovery,
            &orch,
        )
        .unwrap();
    let feas = ws.join("f.md");
    fs::write(&feas, "# Feas\n").unwrap();
    let feas_ref = tandem::factory::artifacts::put(&store, &idea.id, "feasibility", &feas).unwrap();
    store
        .execute(
            &idea.id,
            tandem::factory::machine::Command::StoreArtifact {
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
        .execute(
            &idea.id,
            tandem::factory::machine::Command::SelectDirection,
            &orch,
        )
        .unwrap();
    let plan = ws.join("p.md");
    fs::write(
        &plan,
        "# Plan\n\n## Goal\nPolicy\n\n## Acceptance Criteria\n- Works\n\n## Files\n- a\n\n## Tests\n- test\n",
    )
    .unwrap();
    let plan_ref = tandem::factory::artifacts::put(&store, &idea.id, "plan", &plan).unwrap();
    store
        .execute(
            &idea.id,
            tandem::factory::machine::Command::StoreArtifact {
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
    let caller = tandem::factory::approval::Caller::cli(true, &|_| None, &|_| None);
    let ready = tandem::factory::approval::mark_ready_with_workflow(
        &store, ws, &idea.id, &idea.id, "alice", None, false, &caller,
    )
    .unwrap();
    let run_id = ready.run.unwrap().id;
    (store, run_id)
}

#[test]
fn test_data_class_classification_and_default_secrets_patterns() {
    assert_eq!(classify_path("src/main.rs"), DataClass::Source);
    assert_eq!(
        classify_path("tandem-web/src/app/page.tsx"),
        DataClass::Source
    );

    assert_eq!(classify_path("tests/test_policy.rs"), DataClass::Tests);
    assert_eq!(
        classify_path("src/components/app.test.tsx"),
        DataClass::Tests
    );

    assert_eq!(classify_path("README.md"), DataClass::Docs);
    assert_eq!(classify_path("docs/architecture.md"), DataClass::Docs);

    assert_eq!(classify_path("Cargo.toml"), DataClass::Config);
    assert_eq!(classify_path("config/settings.yaml"), DataClass::Config);

    assert_eq!(classify_path("target/debug/tandem"), DataClass::Generated);
    assert_eq!(classify_path("Cargo.lock"), DataClass::Generated);
    assert_eq!(classify_path("dist/index.js"), DataClass::Generated);

    assert_eq!(classify_path(".env"), DataClass::Secrets);
    assert_eq!(classify_path(".env.example"), DataClass::Secrets);
    assert_eq!(classify_path("sub/.env.local"), DataClass::Secrets);
    assert_eq!(classify_path("config/prod.pem"), DataClass::Secrets);
    assert_eq!(classify_path("keys/server.key"), DataClass::Secrets);
    assert_eq!(classify_path("id_rsa"), DataClass::Secrets);
    assert_eq!(classify_path("keys/id_rsa.pub"), DataClass::Secrets);
    assert_eq!(classify_path("secrets/db_password.txt"), DataClass::Secrets);
    assert_eq!(classify_path(".git/config"), DataClass::Secrets);
}

#[test]
fn test_settings_validate_rejects_secrets_in_allowed_data_classes() {
    let mut settings = FactorySettings::default();
    settings.allowed_data_classes.insert(
        "claude".to_string(),
        vec![DataClass::Source, DataClass::Secrets],
    );
    let err = validate_settings(&settings).expect_err("secrets must never be allowed");
    assert!(err.to_string().contains("secrets"));

    settings.allowed_data_classes.insert(
        "claude".to_string(),
        vec![DataClass::Source, DataClass::Tests],
    );
    settings
        .allowed_models
        .insert("claude".to_string(), vec!["claude-sonnet-4-6".to_string()]);
    settings.denied_path_patterns.push("private/**".to_string());
    assert!(validate_settings(&settings).is_ok());
}

#[test]
fn test_context_policy_denial_returns_file_and_rule() {
    let tmp = temp_dir("context");
    let plan = tmp.join("plan.md");
    let playbook = tmp.join("playbook.md");
    let agent = tmp.join("agent.md");
    fs::write(&plan, "# Plan").unwrap();
    fs::write(&playbook, "# Playbook").unwrap();
    fs::write(&agent, "# Agent").unwrap();

    let inputs = ContextInputs {
        plan_snapshot_path: plan,
        acceptance_criteria: vec!["Criterion 1".to_string()],
        playbook_path: playbook,
        agent_def_path: agent,
        dependency_summaries: Vec::new(),
        retry_rationale: None,
        tool_grant: None,
    };

    let mut settings = FactorySettings::default();
    settings
        .allowed_data_classes
        .insert("claude".to_string(), vec![DataClass::Source]);

    let err = assemble_context_with_policy(&inputs, "claude", &settings)
        .expect_err("docs class should be denied for claude");
    match err {
        ContextError::PolicyDenied(denial) => {
            assert!(denial.path.ends_with("plan.md"));
            assert_eq!(denial.data_class, DataClass::Docs);
            assert!(denial.rule.contains("docs"));
        }
        other => panic!("expected PolicyDenied, got {other:?}"),
    }

    let mut settings2 = FactorySettings::default();
    settings2
        .denied_path_patterns
        .push("**/plan.md".to_string());
    let err2 = assemble_context_with_policy(&inputs, "claude", &settings2)
        .expect_err("denied pattern should block plan.md");
    match err2 {
        ContextError::PolicyDenied(denial) => {
            assert!(denial.path.ends_with("plan.md"));
            assert!(denial.rule.contains("**/plan.md"));
        }
        other => panic!("expected PolicyDenied, got {other:?}"),
    }
}

#[test]
fn test_worktree_sparse_checkout_excludes_secrets_and_denied_patterns() {
    let ws = temp_dir("sparse");
    init_git_repo(&ws);

    fs::create_dir_all(ws.join("src")).unwrap();
    fs::create_dir_all(ws.join("config")).unwrap();
    fs::create_dir_all(ws.join("secrets")).unwrap();
    fs::create_dir_all(ws.join("internal")).unwrap();

    fs::write(ws.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(ws.join(".env.example"), "SECRET=placeholder\n").unwrap();
    fs::write(ws.join("config/prod.pem"), "CERT\n").unwrap();
    fs::write(ws.join("secrets/api.key"), "KEY\n").unwrap();
    fs::write(ws.join("internal/notes.txt"), "PRIVATE\n").unwrap();
    commit_all(&ws, "initial commit");

    let (store, run_id) = setup_queued_run(&ws);
    let mut settings = FactorySettings::default();
    settings
        .denied_path_patterns
        .push("internal/**".to_string());
    save_settings(&store, &settings).unwrap();

    let run_ws = prepare_worktree(&store, &ws, &run_id).expect("prepare_worktree");

    assert!(run_ws.worktree_path.join("src/main.rs").exists());
    assert!(!run_ws.worktree_path.join(".env.example").exists());
    assert!(!run_ws.worktree_path.join("config/prod.pem").exists());
    assert!(!run_ws.worktree_path.join("secrets/api.key").exists());
    assert!(!run_ws.worktree_path.join("internal/notes.txt").exists());

    assert!(
        run_ws
            .excluded_paths
            .iter()
            .any(|e| e.path == ".env.example")
    );
    assert!(
        run_ws
            .excluded_paths
            .iter()
            .any(|e| e.path == "config/prod.pem")
    );
    assert!(
        run_ws
            .excluded_paths
            .iter()
            .any(|e| e.path == "secrets/api.key")
    );
    assert!(
        run_ws
            .excluded_paths
            .iter()
            .any(|e| e.path == "internal/notes.txt")
    );

    let hit = check_slices_against_exclusions(
        &["src/main.rs".to_string(), "config/prod.pem".to_string()],
        &settings,
    );
    assert!(hit.is_some());
    let (file, _rule) = hit.unwrap();
    assert_eq!(file, "config/prod.pem");
}

#[test]
fn test_routing_filters_allowed_models_and_blocks_when_none_left() {
    let wf = default_wf();
    let develop = wf.stages.iter().find(|s| s.name == "develop").unwrap();
    let grant = ToolGrant::from_tools(
        develop.agent,
        &develop.name,
        "workspace",
        develop.tools.clone(),
    );

    let mut settings = FactorySettings::default();
    for g in &mut settings.provider_gates {
        if g.provider == "claude" {
            g.state = ProviderGateState::Open;
        }
    }
    settings
        .allowed_models
        .insert("claude".to_string(), vec!["claude-opus-4-7".to_string()]);

    let routed = route(
        develop,
        &grant,
        &wf,
        &settings,
        &["claude"],
        1,
        &RoutingContext::default(),
    )
    .expect("should fall back to allowed model claude-opus-4-7");
    assert_eq!(routed.model, "claude-opus-4-7");

    settings.allowed_models.insert("claude".to_string(), vec![]);
    let blocked = route(
        develop,
        &grant,
        &wf,
        &settings,
        &["claude"],
        1,
        &RoutingContext::default(),
    )
    .expect_err("no allowed model left should block");
    assert!(matches!(blocked, Unroutable::NoAllowedModel { .. }));
}

#[test]
fn test_run_data_view_includes_manifest_redactions_exclusions_and_limitation() {
    let ws = temp_dir("view");
    init_git_repo(&ws);
    fs::create_dir_all(ws.join("src")).unwrap();
    fs::write(ws.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(ws.join(".env.example"), "TOKEN=abc\n").unwrap();
    commit_all(&ws, "init");

    let (store, run_id) = setup_queued_run(&ws);
    let _run_ws = prepare_worktree(&store, &ws, &run_id).expect("prepare_worktree");

    let plan = ws.join("p2.md");
    let playbook = ws.join("pb.md");
    let agent = ws.join("ag.md");
    fs::write(&plan, "# Plan").unwrap();
    fs::write(&playbook, "# Playbook").unwrap();
    fs::write(&agent, "# Agent").unwrap();

    let grant = ToolGrant::from_tools(
        tandem::factory::workflow::schema::WorkflowAgent::Implementer,
        "develop",
        "workspace",
        vec![
            tandem::factory::workflow::schema::WorkflowTool::RepoRead,
            tandem::factory::workflow::schema::WorkflowTool::RepoWrite,
        ],
    );
    let mut assembled = assemble_context_with_policy(
        &ContextInputs {
            plan_snapshot_path: plan,
            acceptance_criteria: vec![],
            playbook_path: playbook,
            agent_def_path: agent,
            dependency_summaries: vec![],
            retry_rationale: None,
            tool_grant: Some(grant),
        },
        "claude",
        &FactorySettings::default(),
    )
    .unwrap();
    assembled.prompt = "## Plan\n«redacted:api_key»\n".to_string();

    let spec = JobSpec {
        run: run_id.clone(),
        stage: "develop".to_string(),
        attempt: 1,
        agent: "senior-backend-dev".to_string(),
        provider: "claude".to_string(),
        model: "claude-sonnet-4-6".to_string(),
        tool_grants: vec!["repo_read".to_string(), "repo_write".to_string()],
        cwd: ws
            .join("worktrees")
            .join(format!("factory-{run_id}"))
            .to_string_lossy()
            .to_string(),
        time_limit_secs: 600,
    };
    prepare_job(&store, &spec, &assembled).unwrap();

    let run = tandem::factory::run::get_run(&store, &run_id)
        .unwrap()
        .unwrap();
    let view = build_run_data(&store, &run).unwrap();
    assert_eq!(view.limitation, PROVIDER_LIMITATION_SENTENCE);
    assert!(view.excluded_paths.contains(&".env.example".to_string()));
    assert_eq!(view.jobs.len(), 1);
    let job = &view.jobs[0];
    assert_eq!(job.provider, "claude");
    assert_eq!(job.model, "claude-sonnet-4-6");
    assert_eq!(job.sent_files.len(), 3);
    assert_eq!(job.redactions, 1);
    assert!(job.readable_scope.contains("tracked files"));
    assert_eq!(job.tool_grants, vec!["repo_read", "repo_write"]);
}
