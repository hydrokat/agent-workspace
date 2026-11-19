use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tandem::factory::approval::mark_ready_with_workflow;
use tandem::factory::artifacts;
use tandem::factory::broker::gate::{AuthType, consent_provider};
use tandem::factory::checks::{
    CheckKind, ChecksConfig, ChecksError, ConfirmedChecks, DEFAULT_CHECK_TIMEOUT_SECS,
    discover_checks, require_confirmed_checks, set_checks, show_checks, validate_checks_config,
};
use tandem::factory::executor::{Executor, StageJob, StageOutcome};
use tandem::factory::machine::Command;
use tandem::factory::model::{Actor, ActorKind, FactorySettings, RunState};
use tandem::factory::run::get_run;
use tandem::factory::runner::{drive_run_step, resume_paused_run};
use tandem::factory::store::FactoryStore;

fn temp_ws(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "tandem-factory-checks-{label}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn no_env(_: &str) -> Option<String> {
    None
}

fn user_actor() -> Actor {
    Actor {
        kind: ActorKind::User,
        id: "alice".to_string(),
    }
}

struct ScriptedExecutor {
    calls: Vec<String>,
}

impl Executor for ScriptedExecutor {
    fn run(&mut self, job: &StageJob) -> StageOutcome {
        self.calls.push(job.stage.clone());
        StageOutcome::Succeeded
    }
}

fn init_git_repo(dir: &Path) {
    let run = |args: &[&str]| {
        let out = std::process::Command::new("git")
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
    fs::write(dir.join("README.md"), "# test\n").unwrap();
    run(&["add", "README.md"]);
    run(&["commit", "-m", "init"]);
}

#[test]
fn discover_reads_cargo_ci_package_json_workflow_and_check_all_without_executing() {
    let target = temp_ws("discover-all");
    let marker = target.join("should-not-exist");

    fs::write(
        target.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::create_dir_all(target.join(".github/workflows")).unwrap();
    fs::write(
        target.join(".github/workflows/ci.yml"),
        "jobs:\n  lint:\n    steps:\n      - run: cargo clippy --all-targets -- -D warnings\n",
    )
    .unwrap();

    fs::write(target.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").unwrap();
    fs::write(
        target.join("package.json"),
        format!(
            "{{\n  \"name\": \"demo\",\n  \"scripts\": {{\n    \"test\": \"touch {}\",\n    \"lint\": \"eslint .\"\n  }}\n}}\n",
            marker.display()
        ),
    )
    .unwrap();

    fs::write(
        target.join("AGENTS.md"),
        "# Agents\nRun tests via `python3 .agents/skills/test-failures/scripts/run_tests.py -- cargo test`.\n",
    )
    .unwrap();

    fs::create_dir_all(target.join("scripts")).unwrap();
    fs::write(
        target.join("scripts/check-all.sh"),
        "#!/bin/bash\nrun_cmd \"fmt\" cargo fmt --manifest-path \"$ROOT/tandem/Cargo.toml\" --check\nrun_cmd \"test\" cargo test --manifest-path \"$ROOT/tandem/Cargo.toml\" --locked\n",
    )
    .unwrap();

    let report = discover_checks("demo-repo", &target, None).unwrap();
    assert!(!marker.exists(), "discover must never execute commands");
    assert_eq!(report.target, "demo-repo");

    let has = |kind: CheckKind, argv: &[&str], file: &str| {
        report.proposals.iter().any(|p| {
            p.kind == kind
                && p.argv == argv.iter().map(|s| s.to_string()).collect::<Vec<_>>()
                && p.source_file == file
                && p.source_line >= 1
        })
    };

    assert!(has(
        CheckKind::Lint,
        &[
            "cargo",
            "fmt",
            "--manifest-path",
            "tandem/Cargo.toml",
            "--check"
        ],
        "scripts/check-all.sh"
    ));
    assert!(has(
        CheckKind::Test,
        &[
            "cargo",
            "test",
            "--manifest-path",
            "tandem/Cargo.toml",
            "--locked"
        ],
        "scripts/check-all.sh"
    ));
    assert!(has(CheckKind::Test, &["cargo", "test"], "Cargo.toml"));
    assert!(has(
        CheckKind::Lint,
        &["cargo", "clippy", "--all-targets", "--", "-D", "warnings"],
        ".github/workflows/ci.yml"
    ));
    assert!(has(CheckKind::Test, &["pnpm", "test"], "package.json"));
    assert!(has(
        CheckKind::Lint,
        &["pnpm", "run", "lint"],
        "package.json"
    ));
    assert!(has(
        CheckKind::Test,
        &[
            "python3",
            ".agents/skills/test-failures/scripts/run_tests.py",
            "--",
            "cargo",
            "test"
        ],
        "AGENTS.md"
    ));

    let _ = fs::remove_dir_all(&target);
}

#[test]
fn set_and_show_enforce_user_caller_confirm_and_record_metadata() {
    let ws = temp_ws("set-show");
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();

    let agent_env = |k: &str| {
        if k == "TANDEM_ROLE" {
            Some("implementer".into())
        } else {
            None
        }
    };
    let no_pane = |_: &str| None;
    let agent_caller = tandem::factory::approval::Caller::cli(true, &agent_env, &no_pane);
    let user_caller = tandem::factory::approval::Caller::cli(true, &no_env, &no_pane);
    let agent_err = set_checks(
        &store,
        "workspace",
        vec![vec!["cargo".into(), "test".into()]],
        vec![],
        None,
        vec![],
        "workspace",
        "alice",
        &agent_caller,
    )
    .unwrap_err();
    assert!(matches!(agent_err, ChecksError::AgentCaller));

    let confirm_err = set_checks(
        &store,
        "workspace",
        vec![vec!["cargo".into(), "test".into()]],
        vec![],
        None,
        vec![],
        "wrong-target",
        "alice",
        &user_caller,
    )
    .unwrap_err();
    assert!(matches!(confirm_err, ChecksError::InvalidInput(_)));

    let saved = set_checks(
        &store,
        "workspace",
        vec![vec!["cargo".into(), "test".into()]],
        vec![vec!["cargo".into(), "clippy".into()]],
        None,
        vec!["tandem-web/**".into()],
        "workspace",
        "alice",
        &user_caller,
    )
    .unwrap();
    assert_eq!(saved.timeout_secs, DEFAULT_CHECK_TIMEOUT_SECS);
    assert_eq!(saved.confirmed_by.as_deref(), Some("alice"));
    assert!(saved.confirmed_at.is_some());
    assert_eq!(saved.ui_paths, vec!["tandem-web/**".to_string()]);

    let shown = show_checks(&store, Some("workspace")).unwrap();
    assert_eq!(shown.get("workspace"), Some(&saved));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn validation_rejects_malformed_control_chars_shells_and_bounds() {
    let base = ChecksConfig {
        test: vec![vec!["cargo".into(), "test".into()]],
        lint: vec![],
        timeout_secs: 1800,
        ui_paths: vec![],
        confirmed_by: Some("alice".into()),
        confirmed_at: Some(1.0),
    };
    assert!(validate_checks_config(&base).is_ok());

    let mut empty_cmd = base.clone();
    empty_cmd.test = vec![vec![]];
    assert!(validate_checks_config(&empty_cmd).is_err());

    let mut ctrl = base.clone();
    ctrl.test = vec![vec!["cargo".into(), "test\n--all".into()]];
    assert!(validate_checks_config(&ctrl).is_err());

    for shell_argv in [
        vec!["sh", "-c", "true"],
        vec!["/bin/bash", "-c", "cargo test"],
        vec!["zsh", "-c", "echo hi"],
        vec!["cmd", "/c", "dir"],
        vec!["cmd.exe", "/C", "dir"],
    ] {
        let mut shell_cfg = base.clone();
        shell_cfg.test = vec![shell_argv.into_iter().map(String::from).collect()];
        let err = validate_checks_config(&shell_cfg).unwrap_err();
        assert!(
            err.to_string().contains("inline script"),
            "expected inline script refusal for {:?}, got: {err}",
            shell_cfg.test
        );
    }

    let mut too_many_cmds = base.clone();
    too_many_cmds.test = vec![vec!["cargo".into(), "test".into()]; 9];
    assert!(validate_checks_config(&too_many_cmds).is_err());

    let mut too_many_args = base.clone();
    too_many_args.lint = vec![vec!["arg".into(); 33]];
    assert!(validate_checks_config(&too_many_args).is_err());

    let mut zero_timeout = base;
    zero_timeout.timeout_secs = 0;
    assert!(validate_checks_config(&zero_timeout).is_err());
}

#[test]
fn confirmed_checks_sealed_type_requires_confirmation_metadata() {
    let unconfirmed = ChecksConfig {
        test: vec![vec!["cargo".into(), "test".into()]],
        lint: vec![],
        timeout_secs: 1800,
        ui_paths: vec![],
        confirmed_by: None,
        confirmed_at: None,
    };
    assert!(ConfirmedChecks::from_config("workspace", &unconfirmed).is_err());

    let mut settings = FactorySettings::default();
    settings.checks.insert("workspace".into(), unconfirmed);
    assert!(require_confirmed_checks(&settings, "workspace").is_err());

    let confirmed = ChecksConfig {
        test: vec![vec!["cargo".into(), "test".into()]],
        lint: vec![vec!["cargo".into(), "clippy".into()]],
        timeout_secs: 900,
        ui_paths: vec![],
        confirmed_by: Some("alice".into()),
        confirmed_at: Some(1234.5),
    };
    settings.checks.insert("workspace".into(), confirmed);
    let sealed = require_confirmed_checks(&settings, "workspace").unwrap();
    assert_eq!(sealed.target(), "workspace");
    assert_eq!(
        sealed.test(),
        &[vec!["cargo".to_string(), "test".to_string()]]
    );
    assert_eq!(sealed.timeout_secs(), 900);
    assert_eq!(sealed.confirmed_by(), "alice");
}

#[test]
fn runner_pauses_at_test_stage_when_checks_unconfirmed_then_resumes_after_confirm() {
    let ws = temp_ws("runner-pause");
    init_git_repo(&ws);
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();

    let orch = Actor {
        kind: ActorKind::Orchestrator,
        id: "orch".into(),
    };
    let idea = store
        .create_idea(
            "Add checks gate",
            "Verify test stage pauses",
            "alice",
            &user_actor(),
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

    let no_pane = |_: &str| None;
    let user_caller = tandem::factory::approval::Caller::cli(true, &no_env, &no_pane);
    let zero_grace = tandem::factory::model::FactorySettings {
        approval_grace_secs: 0,
        ..Default::default()
    };
    tandem::factory::settings::save(&store, &zero_grace).unwrap();
    let ready = mark_ready_with_workflow(
        &store,
        &ws,
        &idea.id,
        &idea.id,
        "alice",
        None,
        false,
        &user_caller,
    )
    .unwrap();
    let run_id = ready.run.unwrap().id;

    consent_provider(
        &store,
        "claude",
        AuthType::ApiKey,
        "claude",
        "alice",
        &user_caller,
    )
    .unwrap();

    let mut exec = ScriptedExecutor { calls: vec![] };
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    assert_eq!(exec.calls, vec!["plan", "create-tests", "develop"]);

    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    let paused = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(paused.state, RunState::Paused);
    assert_eq!(
        paused.pause_reason.as_deref(),
        Some("test and lint commands are not confirmed")
    );
    assert!(
        paused
            .action_commands
            .iter()
            .any(|c| c.contains("tandem factory checks discover")),
        "expected action_commands to mention discover, got {:?}",
        paused.action_commands
    );
    assert_eq!(exec.calls, vec!["plan", "create-tests", "develop"]);

    set_checks(
        &store,
        "workspace",
        vec![vec!["cargo".into(), "test".into()]],
        vec![vec!["cargo".into(), "clippy".into()]],
        None,
        vec![],
        "workspace",
        "alice",
        &user_caller,
    )
    .unwrap();
    resume_paused_run(&store, &run_id, &user_actor()).unwrap();
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    assert_eq!(exec.calls, vec!["plan", "create-tests", "develop", "test"]);

    let _ = fs::remove_dir_all(&ws);
}
