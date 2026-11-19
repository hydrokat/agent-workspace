mod common;

use std::fs;
use std::process::Command as StdCommand;
use std::time::Duration;

use common::factory_fixture::{FactoryFixture, orch_actor, sys_actor, user_actor, user_caller};
use tandem::coordination::store::current_time;
use tandem::coordination::{CoordStore, encode_query_fields};
use tandem::factory::approval::{mark_ready, verify_snapshot};
use tandem::factory::artifacts;
use tandem::factory::budget::{RunBudget, check as check_budget, spent as compute_spent};
use tandem::factory::events::read_all;
use tandem::factory::evidence;
use tandem::factory::machine::Command;
use tandem::factory::model::{Budget, FeasibilityVerdict, IdeaState, Risk, RunState, StageState};
use tandem::factory::questions;
use tandem::factory::run::{CreateRunOptions, create_run, get_run};
use tandem::factory::settings;
use tandem::factory::telemetry::{self, CostBasis};
use tandem::snapshot;

fn write_blocking_question(coord_root: &std::path::Path, idea_id: &str, qid: &str) {
    let mut fields = serde_json::Map::new();
    fields.insert("id".to_string(), serde_json::json!(qid));
    fields.insert("status".to_string(), serde_json::json!("open"));
    fields.insert("from".to_string(), serde_json::json!("orch-1"));
    fields.insert("to".to_string(), serde_json::json!("user"));
    fields.insert(
        "question".to_string(),
        serde_json::json!("Include timezone offset in exported CSV?"),
    );
    fields.insert(
        "task".to_string(),
        serde_json::json!(format!("idea:{idea_id}")),
    );
    fields.insert("blocking_scope".to_string(), serde_json::json!("plan"));
    let q_path = coord_root.join("queries").join(format!("{qid}.md"));
    fs::write(q_path, encode_query_fields(&fields)).unwrap();
}

fn setup_ready_idea(fixture: &FactoryFixture, title: &str) -> String {
    let user = user_actor();
    let orch = orch_actor();

    let idea = fixture
        .store
        .create_idea(
            title,
            "Export team activity as CSV with date range filtering",
            "user",
            &user,
        )
        .unwrap();

    fixture
        .store
        .execute(
            &idea.id,
            Command::BindOrchestrator {
                cli: "claude".to_string(),
                model: "opus".to_string(),
                conversation: Some("conv-e2e-1".to_string()),
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
                text: "Export team activity as CSV filtered by --from and --to dates.".to_string(),
            },
            &orch,
        )
        .unwrap();

    let feas_src = fixture.workspace.join(".agent-workspace/feas.md");
    fs::write(&feas_src, "# Feasibility\n\nFeasible with existing CLI.\n").unwrap();
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
        .execute(&idea.id, Command::SelectDirection, &user)
        .unwrap();
    fixture
        .store
        .execute(
            &idea.id,
            Command::AddAssumption {
                text: "Timestamps in CSV use UTC ISO-8601".to_string(),
            },
            &orch,
        )
        .unwrap();

    let plan_src = fixture.workspace.join(".agent-workspace/plan.md");
    let plan_body = "\
# Plan

## Scope
Add `src/export.sh` to export team activity as CSV with `--from` and `--to` date filters.

## Acceptance criteria
- Export team activity to CSV
- Filter exported activity by date range

## Risks
Low risk; isolated shell script.

## Estimates
1 day.

## Workflow recommendation
default-delivery
";
    fs::write(&plan_src, plan_body).unwrap();
    let plan_ref = artifacts::put(&fixture.store, &idea.id, "plan", &plan_src).unwrap();
    let missing = artifacts::missing_plan_sections(plan_body);
    assert!(missing.is_empty());
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

fn drive_until_settled(fixture: &FactoryFixture, run_id: &str, max_steps: usize) -> RunState {
    for _ in 0..max_steps {
        let Some(run) = get_run(&fixture.store, run_id).unwrap() else {
            panic!("run {run_id} disappeared");
        };
        if matches!(
            run.state,
            RunState::Delivered | RunState::Paused | RunState::Cancelled | RunState::Failed
        ) {
            return run.state;
        }
        fixture.drive_step(run_id).unwrap();
    }
    get_run(&fixture.store, run_id).unwrap().unwrap().state
}

#[test]
fn shipped_binary_exposes_no_gate_bypass_or_clock_override() {
    let bin = env!("CARGO_BIN_EXE_tandem");
    let forbidden_flags = [
        "--test-terms",
        "--fixed-clock",
        "--bypass-gate",
        "--skip-terms",
        "--gate-override",
    ];

    for args in [
        &["factory", "--help"][..],
        &["factory", "provider", "--help"][..],
        &["factory", "provider", "consent", "--help"][..],
        &["factory", "run", "--help"][..],
    ] {
        let out = StdCommand::new(bin).args(args).output().unwrap();
        assert!(out.status.success(), "help for {:?} should succeed", args);
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        for flag in forbidden_flags {
            assert!(
                !text.contains(flag),
                "shipped binary help for {:?} must not expose {flag}",
                args
            );
        }
    }

    let rejected = StdCommand::new(bin)
        .args([
            "factory",
            "provider",
            "consent",
            "claude",
            "--auth",
            "api-key",
            "--confirm",
            "claude",
            "--fixed-clock",
            "2026-11-19",
        ])
        .output()
        .unwrap();
    assert!(
        !rejected.status.success(),
        "shipped binary must reject --fixed-clock"
    );
}

#[test]
fn acceptance_scenario_add_team_activity_export() {
    let fixture = FactoryFixture::new("happy");
    let user = user_actor();
    let orch = orch_actor();
    let sys = sys_actor();

    // 1. Submit: create idea with provider and model.
    let idea = fixture
        .store
        .create_idea(
            "Add team activity export",
            "Export team activity as CSV with date range filtering",
            "user",
            &user,
        )
        .unwrap();
    let idea = fixture
        .store
        .execute(
            &idea.id,
            Command::BindOrchestrator {
                cli: "claude".to_string(),
                model: "opus".to_string(),
                conversation: Some("conv-accept-1".to_string()),
            },
            &user,
        )
        .unwrap();
    assert_eq!(idea.orchestrator.as_ref().unwrap().cli, "claude");
    assert_eq!(idea.orchestrator.as_ref().unwrap().model, "opus");

    // 2. Ideate: feasibility study, blocking question, assumption, and plan.
    fixture
        .store
        .execute(
            &idea.id,
            Command::Describe {
                title: "Add team activity export".to_string(),
                text: "Export team activity as CSV filtered by date range.".to_string(),
            },
            &orch,
        )
        .unwrap();
    let feas_src = fixture.workspace.join(".agent-workspace/feasibility.md");
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

    let coord_root = fixture.workspace.join(".agent-workspace/coordination");
    write_blocking_question(&coord_root, &idea.id, "q-0001");
    let coord = CoordStore::new(coord_root.clone());
    let qs = coord.queries();
    let idea_qs = questions::for_idea(&qs, &idea.id);
    assert_eq!(idea_qs.blocking, 1);

    fixture
        .store
        .execute(&idea.id, Command::SelectDirection, &user)
        .unwrap();
    fixture
        .store
        .execute(
            &idea.id,
            Command::AddAssumption {
                text: "Export uses UTC ISO-8601 dates".to_string(),
            },
            &orch,
        )
        .unwrap();

    let plan_src = fixture.workspace.join(".agent-workspace/plan.md");
    let plan_body = "\
# Plan

## Scope
Export team activity to CSV with date filtering.

## Acceptance criteria
- Export team activity to CSV
- Filter exported activity by date range

## Risks
Low risk.

## Estimates
1 day.

## Workflow recommendation
default-delivery
";
    fs::write(&plan_src, plan_body).unwrap();
    let plan_ref = artifacts::put(&fixture.store, &idea.id, "plan", &plan_src).unwrap();
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
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();

    // 3. Approve: answer blocking question, mark ready, freeze and verify plan snapshot.
    coord
        .answer("q-0001", "Use UTC timestamps.", "cli")
        .unwrap();
    let qs_after = coord.queries();
    assert_eq!(questions::for_idea(&qs_after, &idea.id).blocking, 0);

    let ready = mark_ready(&fixture.store, &idea.id, &idea.id, "user", &user_caller()).unwrap();
    assert_eq!(ready.idea.state, IdeaState::ReadyForDevelopment);
    let snap_check = verify_snapshot(&fixture.store, &ready.idea).unwrap();
    assert!(snap_check.verified, "frozen plan snapshot hash must verify");

    // 4. Specs & Sprint: create run (with budget and worktree) and run plan stage.
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea.id,
        CreateRunOptions::default(),
        &sys,
    )
    .unwrap();
    assert!(run.budget.is_some());

    fixture.drive_step(&run.id).unwrap();
    let after_plan = get_run(&fixture.store, &run.id).unwrap().unwrap();
    assert!(after_plan.worktree_path.is_some());
    assert_eq!(after_plan.baseline_passed, Some(true));
    assert_eq!(
        after_plan
            .stages
            .iter()
            .find(|s| s.name == "plan")
            .unwrap()
            .state,
        StageState::Succeeded
    );
    let artifacts_dir = fixture.store.run_dir(&run.id).join("artifacts");
    assert!(artifacts_dir.join("slices.json").is_file());
    assert!(artifacts_dir.join("specs/t-001.md").is_file());

    // 5. Execute: create-tests -> develop -> test -> review.
    for expected_stage in ["create-tests", "develop", "test", "review"] {
        fixture.drive_step(&run.id).unwrap();
        let current = get_run(&fixture.store, &run.id).unwrap().unwrap();
        let st = current
            .stages
            .iter()
            .find(|s| s.name == expected_stage)
            .unwrap();
        assert_eq!(
            st.state,
            StageState::Succeeded,
            "stage {expected_stage} should succeed"
        );
    }
    for _ in 0..5 {
        let cur = get_run(&fixture.store, &run.id).unwrap().unwrap();
        if cur.state != RunState::Running {
            break;
        }
        fixture.drive_step(&run.id).unwrap();
    }

    // 6. Outcome: evidence, model/tools, cost with basis, and findings.
    let before_handoff = get_run(&fixture.store, &run.id).unwrap().unwrap();
    assert_eq!(before_handoff.state, RunState::Review);
    let ev_items = evidence::list(&fixture.store, &run.id).unwrap();
    assert!(!ev_items.is_empty(), "evidence items must be recorded");
    let usage = telemetry::run_usage(&fixture.store, &run.id).unwrap();
    assert!(!usage.stages.is_empty());
    for stage_name in ["plan", "create-tests", "develop", "review"] {
        let su = usage
            .stages
            .iter()
            .find(|s| s.stage == stage_name)
            .unwrap_or_else(|| panic!("missing telemetry for stage {stage_name}"));
        assert_eq!(su.provider.as_deref(), Some("claude"));
        assert!(su.model.is_some(), "model recorded for {stage_name}");
        assert_eq!(su.cost.basis, CostBasis::Actual);
        assert!(su.cost.value.unwrap_or(0.0) > 0.0);
    }
    let rounds = evidence::load_review_rounds(&fixture.store, &run.id);
    assert_eq!(rounds.rounds.len(), 1);
    assert_eq!(rounds.rounds[0].findings.len(), 1);

    let snap =
        snapshot::collect_with_factory(&fixture.workspace, &coord, Duration::from_secs(5), true);
    let card = snap
        .factory
        .as_ref()
        .unwrap()
        .ideas
        .iter()
        .find(|c| c.id == idea.id)
        .unwrap();
    assert_eq!(
        card.run.as_ref().unwrap().review_rounds[0].total_findings,
        1
    );

    // 7. Handoff: drive Review state -> stub gh called once, Delivered with 1 PR link, no deploy.
    fixture.drive_step(&run.id).unwrap();
    let final_run = get_run(&fixture.store, &run.id).unwrap().unwrap();
    assert_eq!(final_run.state, RunState::Delivered);
    assert!(!final_run.deploy_requested);
    assert_eq!(
        final_run
            .stages
            .iter()
            .find(|s| s.name == "deploy")
            .unwrap()
            .state,
        StageState::Skipped
    );

    let final_idea = fixture.store.get_idea(&idea.id).unwrap().unwrap();
    assert_eq!(final_idea.state, IdeaState::Delivered);
    assert_eq!(final_idea.prs.len(), 1);
    assert_eq!(final_idea.prs[0].number, 42);

    let gh_log = fs::read_to_string(&fixture.gh_log_path).unwrap();
    let pr_create_calls = gh_log
        .lines()
        .filter(|line| line.starts_with("pr create "))
        .count();
    assert_eq!(pr_create_calls, 1, "stub gh pr create must be called once");

    let run_dir = fixture.store.run_dir(&run.id);
    let pr_body = fs::read_to_string(run_dir.join("pr-body.md")).unwrap();
    for section in [
        "## What changed",
        "## Tests",
        "## Review",
        "## Risks",
        "## Follow-up",
    ] {
        assert!(pr_body.contains(section), "PR body missing {section}");
    }
    assert!(run_dir.join("handoff.md").is_file());
}

#[test]
fn stub_scenario_test_fails_once_recovers_and_delivers() {
    let fixture = FactoryFixture::new("test-fails-once");
    let idea_id = setup_ready_idea(&fixture, "Export with initial test failure");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();

    let final_state = drive_until_settled(&fixture, &run.id, 16);
    assert_eq!(final_state, RunState::Delivered);
    let final_run = get_run(&fixture.store, &run.id).unwrap().unwrap();
    let dev = final_run
        .stages
        .iter()
        .find(|s| s.name == "develop")
        .unwrap();
    assert_eq!(dev.attempts, 2);
}

#[test]
fn stub_scenario_review_changes_once_reopens_develop_and_delivers() {
    let fixture = FactoryFixture::new("review-changes-once");
    let idea_id = setup_ready_idea(&fixture, "Export with review round trip");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();

    let final_state = drive_until_settled(&fixture, &run.id, 20);
    assert_eq!(final_state, RunState::Delivered);
    let rounds = evidence::load_review_rounds(&fixture.store, &run.id);
    assert_eq!(rounds.rounds.len(), 2);
}

#[test]
fn stub_scenario_retries_exhausted_pauses_run() {
    let fixture = FactoryFixture::new("retries-exhausted");
    let idea_id = setup_ready_idea(&fixture, "Export with exhausted retries");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();

    let final_state = drive_until_settled(&fixture, &run.id, 20);
    assert_eq!(final_state, RunState::Paused);
}

#[test]
fn stub_scenario_budget_breach_pauses_run() {
    let fixture = FactoryFixture::new("budget-breach");
    let idea_id = setup_ready_idea(&fixture, "Export with budget breach");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions {
            budget: Some(Budget {
                max_cost_usd: Some(1.0),
                ..Budget::default()
            }),
            ..CreateRunOptions::default()
        },
        &sys_actor(),
    )
    .unwrap();

    let final_state = drive_until_settled(&fixture, &run.id, 6);
    assert_eq!(final_state, RunState::Paused);
    let after = get_run(&fixture.store, &run.id).unwrap().unwrap();
    let ev_log = read_all(&fixture.store.root.join("events.jsonl")).unwrap();
    let spent = compute_spent(&after, &ev_log.events, current_time());
    let run_budget = RunBudget {
        max_cost_usd: after.budget.as_ref().and_then(|b| b.max_cost_usd),
        ..RunBudget::default()
    };
    assert!(matches!(
        check_budget(&after.id, &run_budget, &spent, false),
        tandem::factory::budget::BudgetCheck::Breach(_)
    ));
}

#[test]
fn stub_scenario_policy_denial_pauses_run() {
    let fixture = FactoryFixture::new("policy-denial");
    let mut cfg = fixture.store.settings().unwrap();
    cfg.denied_path_patterns = vec![".env".to_string(), "secrets/**".to_string()];
    settings::save(&fixture.store, &cfg).unwrap();

    let idea_id = setup_ready_idea(&fixture, "Export touching excluded secret");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();

    let final_state = drive_until_settled(&fixture, &run.id, 6);
    assert_eq!(final_state, RunState::Paused);
}

#[test]
fn stub_scenario_crash_mid_stage_pauses_run() {
    let fixture = FactoryFixture::new("crash-mid-stage");
    let idea_id = setup_ready_idea(&fixture, "Export crashing mid-stage");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();

    let final_state = drive_until_settled(&fixture, &run.id, 6);
    assert_eq!(final_state, RunState::Paused);
}

#[test]
fn stub_scenario_hang_times_out_and_pauses_run() {
    use std::collections::BTreeMap;
    use tandem::factory::broker::gate::check_with_terms;
    use tandem::factory::broker::spawn::{ProviderCommand, SpawnError, spawn_provider};
    use tandem::factory::workspace::prepare_worktree;

    let fixture = FactoryFixture::new("hang");
    let idea_id = setup_ready_idea(&fixture, "Export hanging stage");
    let run = create_run(
        &fixture.store,
        &fixture.workspace,
        &idea_id,
        CreateRunOptions::default(),
        &sys_actor(),
    )
    .unwrap();
    let ws = prepare_worktree(&fixture.store, &fixture.workspace, &run.id).unwrap();

    let settings = fixture.store.settings().unwrap();
    let pass = check_with_terms(
        "claude",
        &settings,
        common::factory_fixture::FIXED_TODAY,
        common::factory_fixture::TEST_TERMS,
    )
    .unwrap();

    let mut parent_env: BTreeMap<String, String> = std::env::vars().collect();
    let orig_path = parent_env.get("PATH").cloned().unwrap_or_default();
    parent_env.insert(
        "PATH".to_string(),
        format!("{}:{orig_path}", fixture.stub_dir.display()),
    );
    parent_env.insert(
        "CLAUDE_CONFIG_DIR".to_string(),
        fixture.claude_config_dir.display().to_string(),
    );

    let job_dir = fixture.store.run_dir(&run.id).join("jobs/hang-check");
    fs::create_dir_all(&job_dir).unwrap();

    let mut factory_env = BTreeMap::new();
    factory_env.insert("TANDEM_RUN_ID".to_string(), run.id);
    factory_env.insert("TANDEM_STAGE".to_string(), "plan".to_string());
    factory_env.insert("TANDEM_ATTEMPT".to_string(), "1".to_string());
    factory_env.insert(
        "TANDEM_JOB_DIR".to_string(),
        job_dir.to_string_lossy().to_string(),
    );

    let mut running = spawn_provider(
        pass,
        ProviderCommand {
            program: "claude".to_string(),
            args: vec![],
            workspace_root: fixture.workspace.clone(),
            cwd: ws.worktree_path,
            log_path: job_dir.join("log.txt"),
            stdin_prompt: Some("hang test".to_string()),
            factory_env,
            parent_env,
        },
    )
    .unwrap();

    let err = running.wait(Duration::from_millis(150)).unwrap_err();
    assert!(matches!(err, SpawnError::TimedOut { .. }), "got: {err:?}");
    let _ = running.stop(Duration::from_millis(50));
}
