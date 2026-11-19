use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use tandem::factory::approval::mark_ready;
use tandem::factory::artifacts;
use tandem::factory::events::{Event, append, read_all};
use tandem::factory::executor::{Executor, StageJob, StageOutcome};
use tandem::factory::handoff::{
    build_handoff_markdown, export_specs, refresh_run_prs_with_gh, write_and_record_handoff,
};
use tandem::factory::machine::{self, Command, EventData, Facts};
use tandem::factory::model::{
    Actor, ActorKind, FeasibilityVerdict, IdeaState, PrLink, Risk, RunState,
};
use tandem::factory::run::{CreateRunOptions, create_run, get_run};
use tandem::factory::runner::{deliver_if_pr_created, drive_run_step};
use tandem::factory::stages::review::{
    Finding, FindingSeverity, FindingsReport, ReviewVerdict, record_review_round,
};
use tandem::factory::store::FactoryStore;
use tandem::factory::workflow::schema::EvidenceKind;
use tandem::factory::workspace::{cleanup_worktree, prepare_worktree};

fn temp_ws(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-delivered-{}-{}-{}",
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

fn enable_factory_mode(ws: &Path) {
    fs::create_dir_all(ws.join(".agent-workspace")).unwrap();
    fs::write(ws.join(".agent-workspace").join("factory-mode"), "1\n").unwrap();
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
        id: "orch-1".to_string(),
    }
}

fn sys_actor() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "runner".to_string(),
    }
}

fn run_dir(store: &FactoryStore, run_id: &str) -> PathBuf {
    store.root.join("runs").join(run_id)
}

fn events_path(store: &FactoryStore) -> PathBuf {
    store.root.join("events.jsonl")
}

fn lock_path(store: &FactoryStore) -> PathBuf {
    store.root.join("factory.lock")
}

fn idea_path(store: &FactoryStore, idea_id: &str) -> PathBuf {
    store.root.join("ideas").join(format!("{idea_id}.json"))
}

fn run_json_path(store: &FactoryStore, run_id: &str) -> PathBuf {
    run_dir(store, run_id).join("run.json")
}

fn init_git_repo(dir: &Path, branch: &str) {
    assert!(
        StdCommand::new("git")
            .args(["init", "-b", branch])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    fs::write(dir.join("README.md"), "# Test Repo\n").unwrap();
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

struct NoopExecutor;
impl Executor for NoopExecutor {
    fn run(&mut self, _job: &StageJob) -> StageOutcome {
        panic!("executor should not be called in Review state")
    }
}

fn setup_review_run(ws: &Path) -> (FactoryStore, String, String) {
    enable_factory_mode(ws);
    init_git_repo(ws, "trunk");
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();

    let idea = store
        .create_idea(
            "Add export-specs and handoff",
            "Ship delivery",
            "alice",
            &user_actor(),
        )
        .unwrap();
    store
        .execute(&idea.id, Command::StartDiscovery, &orch_actor())
        .unwrap();

    let feas_src = ws.join("feasibility.md");
    fs::write(&feas_src, "# Feasibility\n\nFeasible.\n").unwrap();
    let feas_ref = artifacts::put(&store, &idea.id, "feasibility", &feas_src).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: feas_ref,
                verdict: Some(FeasibilityVerdict::Feasible),
                risk: Some(Risk::Low),
                workflow: None,
                missing_sections: vec![],
            },
            &orch_actor(),
        )
        .unwrap();

    let plan_src = ws.join("plan.md");
    fs::write(
        &plan_src,
        "## Scope\nA\n## Acceptance criteria\nB\n## Risks\nC\n## Estimates\nD\n## Workflow recommendation\ndefault-delivery\n",
    )
    .unwrap();
    let plan_ref = artifacts::put(&store, &idea.id, "plan", &plan_src).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".to_string(),
                artifact: plan_ref,
                verdict: None,
                risk: None,
                workflow: Some("default-delivery".to_string()),
                missing_sections: vec![],
            },
            &orch_actor(),
        )
        .unwrap();
    store
        .execute(&idea.id, Command::SelectDirection, &user_actor())
        .unwrap();

    let caller = tandem::factory::approval::Caller::cli(true, &|_| None, &|_| None);
    mark_ready(&store, &idea.id, &idea.id, "alice", &caller).unwrap();

    let run = create_run(
        &store,
        ws,
        &idea.id,
        CreateRunOptions::default(),
        &user_actor(),
    )
    .unwrap();
    prepare_worktree(&store, ws, &run.id).unwrap();

    let mut current_idea = store.get_idea(&idea.id).unwrap().unwrap();
    let mut current_run = get_run(&store, &run.id).unwrap().unwrap();
    let mut events = vec![EventData::RunStarted {
        run: run.id.clone(),
    }];
    for stage in &current_run.stages {
        events.push(EventData::StageStarted {
            run: run.id.clone(),
            stage: stage.name.clone(),
            attempt: 1,
            grant: None,
        });
        events.push(EventData::StageSucceeded {
            run: run.id.clone(),
            stage: stage.name.clone(),
        });
    }
    events.push(EventData::RunStagesComplete {
        run: run.id.clone(),
    });

    let mut log_events = Vec::new();
    for ev_data in &events {
        current_idea = machine::apply(Some(current_idea), ev_data, 100.0).unwrap();
        current_run = machine::apply_run(Some(current_run), ev_data, &idea.id, 100.0).unwrap();
        log_events.push(
            Event::from_data(
                100.0,
                sys_actor(),
                idea.id.clone(),
                Some(run.id.clone()),
                ev_data,
            )
            .unwrap(),
        );
    }
    let redacted_logs = tandem::factory::events::redact_events(
        &log_events,
        &tandem::factory::redact::RedactContext::new(),
    );
    append(&events_path(&store), &lock_path(&store), &redacted_logs).unwrap();
    fs::write(
        idea_path(&store, &idea.id),
        serde_json::to_vec_pretty(&current_idea).unwrap(),
    )
    .unwrap();
    fs::write(
        run_json_path(&store, &run.id),
        serde_json::to_vec_pretty(&current_run).unwrap(),
    )
    .unwrap();

    (store, idea.id, run.id)
}

fn record_pr(store: &FactoryStore, idea_id: &str, run_id: &str, pr: PrLink) {
    let ev_data = EventData::PrCreated {
        run: run_id.to_string(),
        pr,
    };
    let ev = Event::from_data(
        200.0,
        sys_actor(),
        idea_id.to_string(),
        Some(run_id.to_string()),
        &ev_data,
    )
    .unwrap();
    let redacted_ev =
        tandem::factory::events::redact_event(ev, &tandem::factory::redact::RedactContext::new());
    append(&events_path(store), &lock_path(store), &[redacted_ev]).unwrap();
    let idea = store.get_idea(idea_id).unwrap().unwrap();
    let run = get_run(store, run_id).unwrap().unwrap();
    let updated_idea = machine::apply(Some(idea), &ev_data, 200.0).unwrap();
    let updated_run = machine::apply_run(Some(run), &ev_data, idea_id, 200.0).unwrap();
    fs::write(
        idea_path(store, idea_id),
        serde_json::to_vec_pretty(&updated_idea).unwrap(),
    )
    .unwrap();
    fs::write(
        run_json_path(store, run_id),
        serde_json::to_vec_pretty(&updated_run).unwrap(),
    )
    .unwrap();
}

#[test]
fn deliver_requires_pr_link_and_moves_idea_and_run_to_delivered_and_keeps_worktree() {
    let ws = temp_ws("deliver-transition");
    let (store, idea_id, run_id) = setup_review_run(&ws);
    let idea = store.get_idea(&idea_id).unwrap().unwrap();

    let err = machine::decide(
        Some(&idea),
        &Command::Deliver {
            run: run_id.clone(),
            prs: vec![],
        },
        &sys_actor(),
        &Facts::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("pull request link"));

    assert!(!deliver_if_pr_created(&store, &ws, &run_id).unwrap());

    let pr = PrLink {
        repo: "org/repo".to_string(),
        number: 42,
        url: "https://github.com/org/repo/pull/42".to_string(),
        state: None,
        checks_summary: None,
    };
    record_pr(&store, &idea_id, &run_id, pr.clone());

    let mut exec = NoopExecutor;
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();

    let delivered_idea = store.get_idea(&idea_id).unwrap().unwrap();
    let delivered_run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(delivered_idea.state, IdeaState::Delivered);
    assert_eq!(delivered_run.state, RunState::Delivered);
    assert_eq!(delivered_idea.prs, vec![pr.clone()]);
    assert_eq!(delivered_run.prs, vec![pr]);

    let wt_path = PathBuf::from(delivered_run.worktree_path.as_deref().unwrap());
    let branch = delivered_run.branch.as_deref().unwrap().to_string();
    assert!(wt_path.exists());
    let branch_check = StdCommand::new("git")
        .args(["rev-parse", "--verify", &branch])
        .current_dir(&ws)
        .status()
        .unwrap();
    assert!(branch_check.success());

    let caller = tandem::factory::approval::Caller::cli(true, &|_| None, &|_| None);
    cleanup_worktree(&store, &ws, &run_id, &user_actor(), &caller).unwrap();
    assert!(!wt_path.exists());

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn handoff_writes_documented_sections_and_records_evidence() {
    let ws = temp_ws("handoff-sections");
    let (store, idea_id, run_id) = setup_review_run(&ws);
    let artifacts_dir = run_dir(&store, &run_id).join("artifacts");
    fs::create_dir_all(artifacts_dir.join("specs/p-001-repo")).unwrap();
    fs::write(artifacts_dir.join("specs/p-001-repo/impl.md"), "# Impl\n").unwrap();
    fs::write(
        artifacts_dir.join("slices.json"),
        serde_json::json!({
            "phase": "p-001-repo",
            "slices": [{
                "id": "t-001",
                "title": "Build handoff module",
                "type": "New Feature",
                "paths": ["README.md"],
                "dependencies": [],
                "acceptance_criteria": ["Works"],
                "ui": false
            }]
        })
        .to_string(),
    )
    .unwrap();

    let round1 = FindingsReport {
        verdict: ReviewVerdict::ChangesRequested,
        findings: vec![Finding {
            id: "F-1".to_string(),
            severity: FindingSeverity::Major,
            file: "README.md".to_string(),
            line: 1,
            title: "Fix typo".to_string(),
            detail: "Typo".to_string(),
            suggestion: "Fix the typo".to_string(),
        }],
    };
    record_review_round(&artifacts_dir, 1, &round1).unwrap();
    let round2 = FindingsReport {
        verdict: ReviewVerdict::Approve,
        findings: vec![Finding {
            id: "F-2".to_string(),
            severity: FindingSeverity::Nit,
            file: "README.md".to_string(),
            line: 2,
            title: "Consider rewording".to_string(),
            detail: "Nit".to_string(),
            suggestion: "Reword sentence".to_string(),
        }],
    };
    record_review_round(&artifacts_dir, 2, &round2).unwrap();

    record_pr(
        &store,
        &idea_id,
        &run_id,
        PrLink {
            repo: "org/repo".to_string(),
            number: 7,
            url: "https://github.com/org/repo/pull/7".to_string(),
            state: None,
            checks_summary: None,
        },
    );

    let (handoff_path, ev_item) =
        write_and_record_handoff(&store, &ws, &run_id, &sys_actor()).unwrap();
    assert_eq!(handoff_path, run_dir(&store, &run_id).join("handoff.md"));
    assert!(handoff_path.is_file());
    assert_eq!(ev_item.kind, EvidenceKind::Handoff);

    let md = fs::read_to_string(&handoff_path).unwrap();
    for heading in [
        "## What was built",
        "## Tests",
        "## Review",
        "## Risks and follow-up",
        "## Pull request links",
        "## Where the work is",
        "## What the user still owns",
    ] {
        assert!(md.contains(heading), "missing heading {heading} in:\n{md}");
    }
    assert!(md.contains("t-001"));
    assert!(md.contains("Build handoff module"));
    assert!(md.contains("Rounds: 2"));
    assert!(md.contains("Findings fixed: 1"));
    assert!(md.contains("Consider rewording"));
    assert!(md.contains("https://github.com/org/repo/pull/7"));
    assert!(md.contains("Deployment stays gated unless the user authorized it."));
    assert!(md.contains("specs/p-001-repo/impl.md"));

    let idea = store.get_idea(&idea_id).unwrap().unwrap();
    let run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(
        build_handoff_markdown(&store, &ws, &idea, &run).as_str(),
        md
    );
    assert!(run.evidence.iter().any(|e| e.kind == EvidenceKind::Handoff));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn prs_command_and_refresh_updates_state_and_checks_without_polling() {
    let ws = temp_ws("prs-refresh");
    let (store, idea_id, run_id) = setup_review_run(&ws);
    record_pr(
        &store,
        &idea_id,
        &run_id,
        PrLink {
            repo: "org/repo".to_string(),
            number: 99,
            url: "https://github.com/org/repo/pull/99".to_string(),
            state: None,
            checks_summary: None,
        },
    );

    let fake_bin = ws.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let gh_path = fake_bin.join("gh");
    let calls_log = ws.join("gh-calls.log");
    let script = format!(
        "#!/bin/sh\necho \"$@\" >> \"{}\"\necho '{{\"state\":\"MERGED\",\"statusCheckRollup\":[{{\"name\":\"ci\",\"conclusion\":\"SUCCESS\"}},{{\"name\":\"lint\",\"conclusion\":\"SUCCESS\"}}]}}'\n",
        calls_log.display()
    );
    fs::write(&gh_path, script).unwrap();
    let mut perms = fs::metadata(&gh_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&gh_path, perms).unwrap();

    let mut new_path = OsString::from(fake_bin.as_os_str());
    if let Some(orig) = std::env::var_os("PATH") {
        new_path.push(":");
        new_path.push(orig);
    }
    let out_no_refresh = StdCommand::new(env!("CARGO_BIN_EXE_tandem"))
        .args(["factory", "run", "prs", &run_id, "--json"])
        .env("PATH", &new_path)
        .env("TANDEM_WORKSPACE", &ws)
        .env("TANDEM_FACTORY", "1")
        .output()
        .unwrap();
    assert!(
        out_no_refresh.status.success(),
        "{}",
        String::from_utf8_lossy(&out_no_refresh.stderr)
    );
    assert!(!calls_log.exists());

    let updated = refresh_run_prs_with_gh(&store, &ws, &run_id, &user_actor(), &gh_path).unwrap();
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].state.as_deref(), Some("merged"));
    assert_eq!(updated[0].checks_summary.as_deref(), Some("2 passed"));

    let run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(run.prs[0].state.as_deref(), Some("merged"));
    let idea = store.get_idea(&idea_id).unwrap().unwrap();
    assert_eq!(idea.prs[0].state.as_deref(), Some("merged"));
    let log = read_all(&events_path(&store)).unwrap();
    assert!(log.events.iter().any(|e| e.r#type == "pr.status_refreshed"));

    let out_refresh = StdCommand::new(env!("CARGO_BIN_EXE_tandem"))
        .args(["factory", "run", "prs", &run_id, "--refresh", "--json"])
        .env("PATH", &new_path)
        .env("TANDEM_WORKSPACE", &ws)
        .env("TANDEM_FACTORY", "1")
        .output()
        .unwrap();
    assert!(
        out_refresh.status.success(),
        "{}",
        String::from_utf8_lossy(&out_refresh.stderr)
    );
    let recorded_calls = fs::read_to_string(&calls_log).unwrap();
    assert_eq!(recorded_calls.lines().count(), 2);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn export_specs_is_user_only_allocates_next_phase_commits_nothing_and_refuses_overwrite() {
    let ws = temp_ws("export-specs");
    let (store, _idea_id, run_id) = setup_review_run(&ws);

    fs::create_dir_all(ws.join("specs/p-002-existing")).unwrap();
    fs::write(ws.join("specs/p-002-existing/impl.md"), "# Existing\n").unwrap();

    let run_specs = run_dir(&store, &run_id).join("artifacts/specs/p-001-draft");
    fs::create_dir_all(&run_specs).unwrap();
    fs::write(run_specs.join("impl.md"), "# Exported Impl\n").unwrap();
    fs::write(run_specs.join("t-001.md"), "# Task 1\n").unwrap();

    let head_before = StdCommand::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&ws)
        .output()
        .unwrap()
        .stdout;

    let agent_env = |k: &str| {
        if k == "TANDEM_ROLE" {
            Some("orchestrator".to_string())
        } else {
            None
        }
    };
    let agent_caller = tandem::factory::approval::Caller::cli(true, &agent_env, &|_| None);
    let user_caller = tandem::factory::approval::Caller::cli(true, &|_| None, &|_| None);
    let err = export_specs(&store, &ws, &run_id, &agent_caller).unwrap_err();
    assert!(err.to_string().contains("user") || err.to_string().contains("agent"));

    let res = export_specs(&store, &ws, &run_id, &user_caller).unwrap();
    assert!(res.phase_dir.starts_with("p-003-"));
    assert!(res.destination.join("impl.md").is_file());
    assert!(res.destination.join("t-001.md").is_file());

    let head_after = StdCommand::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&ws)
        .output()
        .unwrap()
        .stdout;
    assert_eq!(head_before, head_after);

    let target_suffix = res.phase_dir.strip_prefix("p-003-").unwrap();
    let p004 = ws.join("specs").join(format!("p-004-{target_suffix}"));
    fs::create_dir_all(&p004).unwrap();
    let specs_py_dir = ws.join(".agents/skills/spec-planner/scripts");
    fs::create_dir_all(&specs_py_dir).unwrap();
    fs::write(
        specs_py_dir.join("specs.py"),
        "import json, sys\nprint(json.dumps({'next': '004'}))\n",
    )
    .unwrap();
    let collide_err = export_specs(&store, &ws, &run_id, &user_caller).unwrap_err();
    assert!(
        collide_err.to_string().contains("already exists"),
        "expected already exists error, got: {collide_err}"
    );
    fs::remove_file(specs_py_dir.join("specs.py")).unwrap();

    let state_dir = ws.join(".test-state");
    tandem::web_manager::save_web_config(
        &tandem::web_manager::web_config_path(&state_dir.join("tandem")),
        &tandem::web_manager::WebConfig {
            auth_token: "test-token".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let out = StdCommand::new(env!("CARGO_BIN_EXE_tandem"))
        .args(["factory", "run", "export-specs", &run_id, "--json"])
        .env("TANDEM_WORKSPACE", &ws)
        .env("XDG_STATE_HOME", &state_dir)
        .env("TANDEM_AUTH_TOKEN", "test-token")
        .env("TANDEM_INTERNAL_SURFACE", "web")
        .env("TANDEM_FACTORY", "1")
        .env_remove("TANDEM_ROLE")
        .env_remove("TANDEM_SESSION_ID")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        ws.join("specs")
            .join(format!("p-005-{target_suffix}"))
            .join("impl.md")
            .is_file()
    );

    let _ = fs::remove_dir_all(&ws);
}
