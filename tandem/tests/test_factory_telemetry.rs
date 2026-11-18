use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;

use clap::Parser;
use tandem::cli::Cli;
use tandem::config::Config;
use tandem::coordination::store::CoordStore;
use tandem::factory::approval::{MarkReadyOptions, mark_ready_with_options};
use tandem::factory::artifacts;
use tandem::factory::broker::gate::{AuthType, consent_provider};
use tandem::factory::broker::job::job_dir;
use tandem::factory::broker::result::JobUsage;
use tandem::factory::budget::{
    BudgetCheck, RunBudget, check as check_budget, spent as compute_spent,
};
use tandem::factory::commands::run_with_io;
use tandem::factory::events::{Event, append, read_all};
use tandem::factory::executor::{Executor, StageJob, StageOutcome};
use tandem::factory::machine::Command;
use tandem::factory::model::{
    Actor, ActorKind, Budget, FactorySettings, FeasibilityVerdict, ModelPrice, Risk,
};
use tandem::factory::routing::ModelTier;
use tandem::factory::runner::drive_run_step;
use tandem::factory::settings::save as save_settings;
use tandem::factory::store::FactoryStore;
use tandem::factory::telemetry::{
    Cost, CostBasis, RecordJobUsageInput, compute_cost, compute_rework_signals, record_job_usage,
    run_usage,
};
use tandem::snapshot::collect_with_factory;

fn temp_ws(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-telemetry-{name}-{}-{}",
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
    fs::write(dir.join("README.md"), "# test\n").unwrap();
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

fn no_agent_env(key: &str) -> Option<String> {
    match key {
        "TANDEM_ROLE" | "TANDEM_SESSION_ID" => None,
        _ => std::env::var(key).ok(),
    }
}

fn setup_queued_run(ws: &Path, budget: Budget) -> (FactoryStore, String, String) {
    init_git_repo(ws);
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();

    let settings = FactorySettings {
        default_budget: budget,
        ..Default::default()
    };
    save_settings(&store, &settings).unwrap();
    consent_provider(
        &store,
        "claude",
        AuthType::ApiKey,
        "claude",
        "alice",
        &no_agent_env,
    )
    .unwrap();

    let orch = Actor {
        kind: ActorKind::Orchestrator,
        id: "orch-1".to_string(),
    };
    let user = user_actor();
    let idea = store
        .create_idea(
            "Telemetry test",
            "Verify cost basis and rework signals",
            "alice",
            &user,
        )
        .unwrap();
    store
        .execute(&idea.id, Command::StartDiscovery, &orch)
        .unwrap();
    let feas_file = ws.join("feas.md");
    fs::write(&feas_file, "# Feasibility\nFeasible.\n").unwrap();
    let feas_ref = artifacts::put(&store, &idea.id, "feasibility", &feas_file).unwrap();
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
            &orch,
        )
        .unwrap();
    store
        .execute(&idea.id, Command::SelectDirection, &user)
        .unwrap();

    let plan_file = ws.join("plan.md");
    fs::write(
        &plan_file,
        "# Plan\n## Scope\nA\n## Approach\nB\n## Files\nC\n## Tests\nD\n## Acceptance\n- pass\n",
    )
    .unwrap();
    let plan_ref = artifacts::put(&store, &idea.id, "plan", &plan_file).unwrap();
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
            &orch,
        )
        .unwrap();

    let res = mark_ready_with_options(
        &store,
        ws,
        &idea.id,
        &idea.id,
        "cli",
        "alice",
        MarkReadyOptions::default(),
        &no_agent_env,
    )
    .unwrap();
    let run_id = res.run.unwrap().id;
    (store, idea.id, run_id)
}

#[test]
fn cost_basis_and_formatting_are_honest() {
    let empty_prices: BTreeMap<String, ModelPrice> = BTreeMap::new();
    let mut prices = BTreeMap::new();
    prices.insert(
        "claude-sonnet-4-6".to_string(),
        ModelPrice {
            input: 3.0,
            output: 15.0,
            as_of: "2026-11-01".to_string(),
        },
    );

    assert!(FactorySettings::default().price_table.is_empty());

    let reported_cost = JobUsage {
        input_tokens: Some(1000),
        output_tokens: Some(500),
        cost_usd: Some(0.84),
    };
    let c1 = compute_cost(Some(&reported_cost), "claude-sonnet-4-6", &empty_prices);
    assert_eq!(c1.basis, CostBasis::Actual);
    assert!((c1.value.unwrap() - 0.84).abs() < 1e-6);
    assert_eq!(c1.format(), "$0.84");

    let tokens_only = JobUsage {
        input_tokens: Some(100_000),
        output_tokens: Some(20_000),
        cost_usd: None,
    };
    let c2 = compute_cost(Some(&tokens_only), "claude-sonnet-4-6", &prices);
    assert_eq!(c2.basis, CostBasis::Actual);
    assert!((c2.value.unwrap() - 0.60).abs() < 1e-6);
    assert_eq!(c2.format(), "$0.60");

    let c2_no_price = compute_cost(Some(&tokens_only), "claude-sonnet-4-6", &empty_prices);
    assert_eq!(c2_no_price.basis, CostBasis::Unknown);
    assert_eq!(c2_no_price.value, None);
    assert_eq!(c2_no_price.format(), "unknown");

    let c3 = compute_cost(None, "claude-sonnet-4-6", &prices);
    assert_eq!(c3.basis, CostBasis::Estimated);
    assert!(c3.value.is_some());
    assert!(c3.format().starts_with("about $"));
    assert!(c3.format().ends_with(" (estimated)"));

    let est_80 = Cost {
        value: Some(0.80),
        basis: CostBasis::Estimated,
    };
    assert_eq!(est_80.format(), "about $0.80 (estimated)");

    let c4 = compute_cost(None, "claude-sonnet-4-6", &empty_prices);
    assert_eq!(c4.basis, CostBasis::Unknown);
    assert_eq!(c4.value, None);
    assert_eq!(c4.format(), "unknown");
}

#[test]
fn mixed_basis_totals_never_upgrade_certainty() {
    let a = Cost {
        value: Some(0.50),
        basis: CostBasis::Actual,
    };
    let b = Cost {
        value: Some(0.30),
        basis: CostBasis::Actual,
    };
    let e = Cost {
        value: Some(0.20),
        basis: CostBasis::Estimated,
    };
    let u = Cost::unknown();

    let aa = a.combine(&b);
    assert_eq!(aa.basis, CostBasis::Actual);
    assert!((aa.value.unwrap() - 0.80).abs() < 1e-6);

    let ae = a.combine(&e);
    assert_eq!(ae.basis, CostBasis::Estimated);
    assert!((ae.value.unwrap() - 0.70).abs() < 1e-6);
    assert_eq!(ae.format(), "about $0.70 (estimated)");

    let au = a.combine(&u);
    assert_eq!(au.basis, CostBasis::Unknown);
    assert_eq!(au.value, None);
    assert_eq!(au.format(), "unknown");

    let eu = e.combine(&u);
    assert_eq!(eu.basis, CostBasis::Unknown);
    assert_eq!(eu.value, None);
}

#[test]
fn runner_records_job_usage_event_after_each_job() {
    let ws = temp_ws("record");
    let (store, _idea_id, run_id) = setup_queued_run(&ws, Budget::default());

    struct UsageExecutor {
        store: FactoryStore,
    }
    impl Executor for UsageExecutor {
        fn run(&mut self, job: &StageJob) -> StageOutcome {
            let dir = job_dir(&self.store, &job.run, &job.stage, job.attempt);
            fs::create_dir_all(&dir).unwrap();
            let res = serde_json::json!({
                "status": "succeeded",
                "summary": "planned",
                "files_changed": [],
                "artifacts": [],
                "usage": {
                    "input_tokens": 1200,
                    "output_tokens": 300,
                    "cost_usd": 0.42
                }
            });
            fs::write(dir.join("result.json"), serde_json::to_vec(&res).unwrap()).unwrap();
            StageOutcome::Succeeded
        }
    }

    let mut exec = UsageExecutor {
        store: store.clone(),
    };
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();

    let log = read_all(&store.root.join("events.jsonl")).unwrap();
    let usage_ev = log
        .events
        .iter()
        .find(|ev| ev.r#type == "job.usage" && ev.run.as_deref() == Some(run_id.as_str()))
        .expect("expected job.usage event");
    assert_eq!(usage_ev.data["stage"], "plan");
    assert_eq!(usage_ev.data["provider"], "claude");
    assert_eq!(usage_ev.data["model"], "claude-sonnet-4-6");
    assert_eq!(usage_ev.data["tier"], "standard");
    assert!(usage_ev.data["duration_secs"].as_f64().is_some());
    assert_eq!(usage_ev.data["input_tokens"], 1200);
    assert_eq!(usage_ev.data["output_tokens"], 300);
    assert_eq!(usage_ev.data["cost"]["basis"], "actual");
    assert!((usage_ev.data["cost"]["value"].as_f64().unwrap() - 0.42).abs() < 1e-6);

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn rework_signals_derived_from_events() {
    let sys = Actor {
        kind: ActorKind::System,
        id: "runner".to_string(),
    };
    let ev = |seq: u64, ts: f64, r#type: &str, data: serde_json::Value| Event {
        v: 1,
        seq,
        ts,
        actor: sys.clone(),
        r#type: r#type.to_string(),
        idea: "idea-1".to_string(),
        run: Some("run-1".to_string()),
        data,
    };

    let events = vec![
        ev(
            1,
            10.0,
            "stage.started",
            serde_json::json!({"run": "run-1", "stage": "develop", "attempt": 1}),
        ),
        ev(
            2,
            12.0,
            "stage.failed",
            serde_json::json!({"run": "run-1", "stage": "develop", "reason": "Compile error in foo.rs"}),
        ),
        ev(
            3,
            13.0,
            "stage.model_upgraded",
            serde_json::json!({"run": "run-1", "stage": "develop", "from": "standard", "to": "premium"}),
        ),
        ev(
            4,
            14.0,
            "stage.started",
            serde_json::json!({"run": "run-1", "stage": "develop", "attempt": 2}),
        ),
        ev(
            5,
            16.0,
            "stage.failed",
            serde_json::json!({"run": "run-1", "stage": "develop", "reason": "  compile error in foo.rs  "}),
        ),
        ev(
            6,
            17.0,
            "stage.started",
            serde_json::json!({"run": "run-1", "stage": "develop", "attempt": 3}),
        ),
        ev(
            7,
            19.0,
            "stage.interrupted",
            serde_json::json!({"run": "run-1", "stage": "develop"}),
        ),
        ev(
            8,
            20.0,
            "stage.started",
            serde_json::json!({"run": "run-1", "stage": "develop", "attempt": 4}),
        ),
        ev(
            9,
            25.0,
            "stage.succeeded",
            serde_json::json!({"run": "run-1", "stage": "develop"}),
        ),
        ev(
            10,
            26.0,
            "stage.started",
            serde_json::json!({"run": "run-1", "stage": "test", "attempt": 1}),
        ),
        ev(
            11,
            28.0,
            "stage.failed",
            serde_json::json!({"run": "run-1", "stage": "test", "reason": "unit test failed"}),
        ),
        ev(
            12,
            28.0,
            "stage.reopened",
            serde_json::json!({"run": "run-1", "stage": "develop", "reason": "unit test failed"}),
        ),
        ev(
            13,
            30.0,
            "stage.started",
            serde_json::json!({"run": "run-1", "stage": "develop", "attempt": 5}),
        ),
        ev(
            14,
            32.0,
            "stage.succeeded",
            serde_json::json!({"run": "run-1", "stage": "develop"}),
        ),
        ev(
            15,
            33.0,
            "stage.started",
            serde_json::json!({"run": "run-1", "stage": "test", "attempt": 2}),
        ),
        ev(
            16,
            35.0,
            "stage.succeeded",
            serde_json::json!({"run": "run-1", "stage": "test"}),
        ),
        ev(
            17,
            36.0,
            "stage.started",
            serde_json::json!({"run": "run-1", "stage": "review", "attempt": 1}),
        ),
        ev(
            18,
            40.0,
            "stage.succeeded",
            serde_json::json!({"run": "run-1", "stage": "review"}),
        ),
        ev(
            19,
            40.0,
            "run.stages_complete",
            serde_json::json!({"run": "run-1"}),
        ),
        ev(
            20,
            40.0,
            "run.changes_requested",
            serde_json::json!({"run": "run-1"}),
        ),
        ev(
            21,
            40.0,
            "stage.reopened",
            serde_json::json!({"run": "run-1", "stage": "develop", "reason": "missing edge case"}),
        ),
        ev(
            22,
            41.0,
            "stage.started",
            serde_json::json!({"run": "run-1", "stage": "develop", "attempt": 6}),
        ),
        ev(
            23,
            45.0,
            "stage.succeeded",
            serde_json::json!({"run": "run-1", "stage": "develop"}),
        ),
        ev(
            24,
            46.0,
            "stage.started",
            serde_json::json!({"run": "run-1", "stage": "review", "attempt": 2}),
        ),
        ev(
            25,
            50.0,
            "stage.succeeded",
            serde_json::json!({"run": "run-1", "stage": "review"}),
        ),
    ];

    let signals = compute_rework_signals("run-1", &events);
    assert_eq!(signals.retries_by_stage["develop"].retries, 4);
    assert_eq!(signals.retries_by_stage["develop"].repeated_rationales, 1);
    assert_eq!(signals.retries_by_stage["test"].retries, 1);
    assert_eq!(signals.retries_by_stage["review"].retries, 1);
    assert_eq!(signals.repeated_rationales, 1);
    assert_eq!(signals.total_retries, 6);
    assert_eq!(signals.develop_reopened_by_test, 1);
    assert_eq!(signals.develop_reopened_by_review, 1);
    assert_eq!(signals.review_rounds, 2);
    assert_eq!(signals.model_upgrades, 1);
    assert_eq!(signals.interrupted_jobs, 1);
}

#[test]
fn cli_run_usage_json_and_snapshot_summary_report_basis_and_rework() {
    let ws = temp_ws("cli-usage");
    let (store, idea_id, run_id) = setup_queued_run(&ws, Budget::default());

    let mut settings = store.settings().unwrap();
    settings.price_table.insert(
        "claude-sonnet-4-6".to_string(),
        ModelPrice {
            input: 3.0,
            output: 15.0,
            as_of: "2026-11-01".to_string(),
        },
    );
    save_settings(&store, &settings).unwrap();

    let sys = Actor {
        kind: ActorKind::System,
        id: "telemetry".to_string(),
    };
    let actual_usage = JobUsage {
        input_tokens: Some(10_000),
        output_tokens: Some(2_000),
        cost_usd: Some(0.50),
    };
    record_job_usage(RecordJobUsageInput {
        store: &store,
        idea_id: &idea_id,
        run_id: &run_id,
        stage: "plan",
        attempt: 1,
        provider: "claude",
        model: "claude-sonnet-4-6",
        tier: ModelTier::Standard,
        duration_secs: 12.5,
        usage: Some(&actual_usage),
        price_table: &settings.price_table,
        actor: &sys,
    })
    .unwrap();

    record_job_usage(RecordJobUsageInput {
        store: &store,
        idea_id: &idea_id,
        run_id: &run_id,
        stage: "develop",
        attempt: 1,
        provider: "claude",
        model: "claude-sonnet-4-6",
        tier: ModelTier::Standard,
        duration_secs: 20.0,
        usage: None,
        price_table: &settings.price_table,
        actor: &sys,
    })
    .unwrap();

    let report = run_usage(&store, &run_id).unwrap();
    assert_eq!(report.stages.len(), 2);
    assert_eq!(report.stages[0].cost.basis, CostBasis::Actual);
    assert_eq!(report.stages[1].cost.basis, CostBasis::Estimated);
    assert_eq!(report.total_cost.basis, CostBasis::Estimated);
    assert!(report.total_cost_display.starts_with("about $"));
    assert!(report.total_cost_display.ends_with(" (estimated)"));

    let cli =
        Cli::try_parse_from(["tandem", "factory", "run", "usage", &run_id, "--json"]).unwrap();
    let Some(tandem::cli::Command::Factory(f_args)) = cli.command else {
        panic!("expected factory command");
    };
    let cfg = Config {
        factory: true,
        ..Default::default()
    };
    let mut stdin = std::io::Cursor::new(Vec::<u8>::new());
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_with_io(&f_args, &ws, &cfg, &mut stdin, &mut out, &mut err);
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    let parsed: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(parsed["run"], run_id);
    assert_eq!(parsed["total_cost"]["basis"], "estimated");
    assert_eq!(parsed["stages"].as_array().unwrap().len(), 2);
    assert!(parsed["rework"].is_object());

    let coord = CoordStore::new(ws.join("coord"));
    let snap = collect_with_factory(&ws, &coord, Duration::from_secs(1), true);
    let card = snap
        .factory
        .unwrap()
        .ideas
        .into_iter()
        .find(|i| i.id == idea_id)
        .unwrap();
    let rs = card.run.unwrap();
    assert!(rs.elapsed_secs.is_some());
    assert_eq!(rs.cost.as_ref().unwrap().basis, CostBasis::Estimated);
    assert_eq!(
        rs.cost_display.as_deref(),
        Some(report.total_cost_display.as_str())
    );
    assert_eq!(rs.retry_count, 0);

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn budget_does_not_breach_when_total_cost_is_unknown() {
    let ws = temp_ws("unknown-budget");
    let (store, idea_id, run_id) = setup_queued_run(
        &ws,
        Budget {
            max_cost_usd: Some(1.00),
            max_duration_secs: None,
            max_retries: None,
        },
    );

    let sys = Actor {
        kind: ActorKind::System,
        id: "telemetry".to_string(),
    };
    let empty_prices = BTreeMap::new();
    let actual = JobUsage {
        input_tokens: None,
        output_tokens: None,
        cost_usd: Some(0.90),
    };
    record_job_usage(RecordJobUsageInput {
        store: &store,
        idea_id: &idea_id,
        run_id: &run_id,
        stage: "plan",
        attempt: 1,
        provider: "claude",
        model: "claude-sonnet-4-6",
        tier: ModelTier::Standard,
        duration_secs: 5.0,
        usage: Some(&actual),
        price_table: &empty_prices,
        actor: &sys,
    })
    .unwrap();
    record_job_usage(RecordJobUsageInput {
        store: &store,
        idea_id: &idea_id,
        run_id: &run_id,
        stage: "develop",
        attempt: 1,
        provider: "agy",
        model: "gemini-2.0-flash",
        tier: ModelTier::Standard,
        duration_secs: 5.0,
        usage: None,
        price_table: &empty_prices,
        actor: &sys,
    })
    .unwrap();

    let log = read_all(&store.root.join("events.jsonl")).unwrap();
    let run = tandem::factory::run::get_run(&store, &run_id)
        .unwrap()
        .unwrap();
    let sp = compute_spent(&run, &log.events, 100.0);
    assert_eq!(sp.cost_usd, None);
    let budget = RunBudget {
        max_cost_usd: Some(1.00),
        max_duration_secs: None,
        max_retries: None,
    };
    assert_eq!(check_budget(&run_id, &budget, &sp, true), BudgetCheck::Ok);

    // Ensure non-telemetry event still doesn't break anything.
    let extra = Event {
        v: 1,
        seq: 0,
        ts: 101.0,
        actor: sys,
        r#type: "stage.started".to_string(),
        idea: idea_id,
        run: Some(run_id.clone()),
        data: serde_json::json!({"run": run_id, "stage": "test", "attempt": 1}),
    };
    let redacted_extra = tandem::factory::events::redact_event(
        extra,
        &tandem::factory::redact::RedactContext::new(),
    );
    append(
        &store.root.join("events.jsonl"),
        &store.root.join(".lock"),
        &[redacted_extra],
    )
    .unwrap();

    let _ = fs::remove_dir_all(ws);
}
