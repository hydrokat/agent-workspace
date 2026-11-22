use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use clap::Parser;
use tandem::cli::Cli;
use tandem::config::Config;
use tandem::factory::approval::{ApprovalError, MarkReadyOptions, mark_ready_with_options};
use tandem::factory::artifacts;
use tandem::factory::budget::{
    BudgetCheck, EstimateHistory, EstimateRange, HistoricalRun, RunBudget, Spent,
    StageTokenEstimate, check, cost_cap_label, effective_stage_budget, estimate,
    provider_reports_usage,
};
use tandem::factory::commands::run_with_caller;
use tandem::factory::events::{Event, append, read_all};
use tandem::factory::executor::{Executor, StageJob, StageOutcome};
use tandem::factory::machine::{Command, EventData};
use tandem::factory::model::{
    Actor, ActorKind, Budget, FactorySettings, FeasibilityVerdict, ModelPrice, Risk, RunState,
    Stage, StageState,
};
use tandem::factory::run::get_run;
use tandem::factory::runner::{RunnerError, change_run_budget, drive_run_step};
use tandem::factory::settings::save as save_settings;
use tandem::factory::store::FactoryStore;
use tandem::factory::workflow::load_default_workflow;
use tandem::factory::workflow::schema::{StageBudget as WorkflowStageBudget, Workflow};
use tandem::factory::workflow::validate::validate as validate_workflow;

fn temp_ws(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-budget-{name}-{}-{}",
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

fn no_env(_: &str) -> Option<String> {
    None
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

fn sample_workflow(settings: &FactorySettings) -> Workflow {
    let (raw, _) = load_default_workflow(Path::new("/tmp/nonexistent-ws")).unwrap();
    validate_workflow(&raw, settings).unwrap()
}

fn setup_planning_idea(ws: &Path) -> (FactoryStore, String) {
    init_git_repo(ws);
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();
    let user = user_actor();
    let orch = orch_actor();
    let idea = store
        .create_idea("Budget test", "Verify budget caps", "alice", &user)
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
    store
        .execute(&idea.id, Command::SelectDirection, &user)
        .unwrap();
    let plan = ws.join("p.md");
    fs::write(&plan, "# Plan\n## Scope\n## Tasks\n").unwrap();
    let plan_ref = artifacts::put(&store, &idea.id, "plan", &plan).unwrap();
    store
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
    (store, idea.id)
}

fn setup_queued_run_with_budget(
    ws: &Path,
    default_budget: Budget,
) -> (FactoryStore, String, String) {
    let (store, idea_id) = setup_planning_idea(ws);
    let mut settings = store.settings().unwrap();
    settings.default_budget = default_budget;
    settings.approval_grace_secs = 0;
    save_settings(&store, &settings).unwrap();
    let caller = tandem::factory::approval::Caller::cli(true, &no_env, &|_| None);

    let ready = mark_ready_with_options(
        &store,
        ws,
        &idea_id,
        &idea_id,
        "alice",
        MarkReadyOptions::default(),
        &caller,
    )
    .unwrap();
    let run_id = ready.run.unwrap().id;
    (store, idea_id, run_id)
}

type OnCallHook = Box<dyn FnMut(&FactoryStore, &StageJob)>;

struct SequenceExecutor {
    outcomes: Vec<StageOutcome>,
    calls: Vec<String>,
    on_call: Option<OnCallHook>,
    store: FactoryStore,
}

impl Executor for SequenceExecutor {
    fn ready_clis(&self) -> Vec<tandem::factory::executor::ReadyCli> {
        vec![tandem::factory::executor::ReadyCli::new(
            "claude",
            Some(tandem::factory::broker::terms::AuthType::ApiKey),
        )]
    }

    fn run(&mut self, job: &StageJob) -> StageOutcome {
        self.calls.push(format!("{}:{}", job.stage, job.attempt));
        if let Some(cb) = self.on_call.as_mut() {
            cb(&self.store, job);
        }
        if self.outcomes.is_empty() {
            StageOutcome::Succeeded
        } else {
            self.outcomes.remove(0)
        }
    }
}

#[test]
fn run_budget_inherits_settings_and_stage_budget_bounded_by_remaining() {
    let settings = FactorySettings {
        default_budget: Budget {
            max_cost_usd: Some(10.0),
            max_duration_secs: Some(3600),
            max_retries: Some(3),
        },
        ..Default::default()
    };

    let base = RunBudget::from_settings(&settings);
    assert_eq!(base.max_cost_usd, Some(10.0));
    assert_eq!(base.max_duration_secs, Some(3600));
    assert_eq!(base.max_retries, Some(3));

    // User can lower within settings limit.
    let lowered = base
        .with_overrides(Some(4.0), Some(1200), Some(1), &settings)
        .unwrap();
    assert_eq!(lowered.max_cost_usd, Some(4.0));
    assert_eq!(lowered.max_duration_secs, Some(1200));
    assert_eq!(lowered.max_retries, Some(1));

    // User cannot raise above settings limit.
    assert!(
        base.with_overrides(Some(15.0), None, None, &settings)
            .is_err()
    );
    assert!(
        base.with_overrides(None, Some(7200), None, &settings)
            .is_err()
    );
    assert!(base.with_overrides(None, None, Some(5), &settings).is_err());

    // Stage budget is bounded by remaining run budget.
    let stage_def = WorkflowStageBudget {
        max_cost_usd: Some(8.0),
        max_duration_secs: Some(1800),
        max_retries: Some(2),
    };
    let spent = Spent {
        cost_usd: Some(7.5),
        duration_secs: 3000,
        retries: 2,
    };
    let eff = effective_stage_budget(&stage_def, &base, &spent);
    assert!((eff.max_cost_usd.unwrap() - 2.5).abs() < 1e-6);
    assert_eq!(eff.max_duration_secs, Some(600));
    assert_eq!(eff.max_retries, Some(1));
}

#[test]
fn estimate_returns_ranges_or_unknown_never_single_number() {
    let mut settings = FactorySettings::default();
    let wf = sample_workflow(&settings);
    let mut routes = BTreeMap::new();
    routes.insert("plan".to_string(), "claude-sonnet-4-6".to_string());
    routes.insert("develop".to_string(), "claude-sonnet-4-6".to_string());

    // Empty history & empty price table -> both unknown.
    let est_empty = estimate(
        &wf,
        &routes,
        &settings.price_table,
        &EstimateHistory::default(),
    );
    assert!(matches!(
        est_empty.duration_secs,
        EstimateRange::Unknown { .. }
    ));
    assert!(matches!(est_empty.cost_usd, EstimateRange::Unknown { .. }));

    // Add single historical run + price table -> both must be ranges (min < max).
    settings.price_table.insert(
        "claude-sonnet-4-6".to_string(),
        ModelPrice {
            input: 3.0,
            output: 15.0,
            as_of: "2026-11-18".to_string(),
        },
    );
    let history = EstimateHistory {
        runs: vec![HistoricalRun {
            workflow: wf.name.clone(),
            duration_secs: 1000,
        }],
        stage_tokens: BTreeMap::from([
            (
                "plan".to_string(),
                StageTokenEstimate {
                    min_input: 10_000,
                    max_input: 20_000,
                    min_output: 2_000,
                    max_output: 4_000,
                },
            ),
            (
                "develop".to_string(),
                StageTokenEstimate {
                    min_input: 50_000,
                    max_input: 100_000,
                    min_output: 10_000,
                    max_output: 20_000,
                },
            ),
        ]),
    };

    let est = estimate(&wf, &routes, &settings.price_table, &history);
    match est.duration_secs {
        EstimateRange::Known { min, max, .. } => {
            assert!(min < max, "duration must be a range, got {min}..{max}");
        }
        EstimateRange::Unknown { reason } => panic!("expected known duration, got {reason}"),
    }
    match est.cost_usd {
        EstimateRange::Known { min, max, .. } => {
            assert!(min < max, "cost must be a range, got {min}..{max}");
        }
        EstimateRange::Unknown { reason } => panic!("expected known cost, got {reason}"),
    }
}

#[test]
fn idea_ready_requires_accept_estimate_when_upper_end_exceeds_cap() {
    let ws = temp_ws("ready-accept");
    let (store, idea_id) = setup_planning_idea(&ws);

    // Record a completed historical run for default-delivery so estimate has duration history.
    let mut settings = store.settings().unwrap();
    settings.approval_grace_secs = 0;
    settings.default_budget = Budget {
        max_cost_usd: Some(5.0),
        max_duration_secs: Some(600),
        max_retries: Some(2),
    };
    save_settings(&store, &settings).unwrap();

    let sys = Actor {
        kind: ActorKind::System,
        id: "test".to_string(),
    };
    let past_run_ev = Event::from_data(
        1000.0,
        sys.clone(),
        "i-past0001".to_string(),
        Some("r-past0001".to_string()),
        &EventData::RunCreated {
            run: "r-past0001".to_string(),
            workflow: "default-delivery".to_string(),
            version: "1".to_string(),
            sha256: "abc".to_string(),
            source: "embedded".to_string(),
            plan_sha256: "def".to_string(),
            deploy_requested: false,
            stages: vec![Stage {
                name: "plan".to_string(),
                agent: "business-analyst".to_string(),
                state: StageState::Succeeded,
                attempts: 1,
                interrupted_attempts: 0,
                started: Some(1000.0),
                finished: Some(2200.0),
                artifacts: vec![],
                rationale: None,
                retry_hypothesis: None,
            }],
            budget: None,
        },
    )
    .unwrap();
    let stage_start_ev = Event::from_data(
        1000.0,
        sys.clone(),
        "i-past0001".to_string(),
        Some("r-past0001".to_string()),
        &EventData::StageStarted {
            run: "r-past0001".to_string(),
            stage: "plan".to_string(),
            attempt: 1,
            grant: None,
        },
    )
    .unwrap();
    let stage_end_ev = Event::from_data(
        2200.0,
        sys,
        "i-past0001".to_string(),
        Some("r-past0001".to_string()),
        &EventData::StageSucceeded {
            run: "r-past0001".to_string(),
            stage: "plan".to_string(),
        },
    )
    .unwrap();
    let redacted_evs = tandem::factory::events::redact_events(
        &[past_run_ev, stage_start_ev, stage_end_ev],
        &tandem::factory::redact::RedactContext::new(),
    );
    append(
        &store.root.join("events.jsonl"),
        &store.root.join(".lock"),
        &redacted_evs,
    )
    .unwrap();

    // Without --accept-estimate: refused because 1200s estimate > 600s cap.
    let caller = tandem::factory::approval::Caller::cli(true, &no_env, &|_| None);
    let err = mark_ready_with_options(
        &store,
        &ws,
        &idea_id,
        &idea_id,
        "alice",
        MarkReadyOptions::default(),
        &caller,
    )
    .unwrap_err();
    assert!(matches!(err, ApprovalError::Gate(_)));
    assert!(err.to_string().contains("--accept-estimate"));

    // With --accept-estimate and lowered cost cap: succeeds and records budget.
    let ready = mark_ready_with_options(
        &store,
        &ws,
        &idea_id,
        &idea_id,
        "alice",
        MarkReadyOptions {
            max_cost_usd: Some(3.0),
            accept_estimate: true,
            ..Default::default()
        },
        &caller,
    )
    .unwrap();
    assert_eq!(ready.budget.as_ref().unwrap().max_cost_usd, Some(3.0));
    assert!(ready.estimate.is_some());
    assert_eq!(
        ready
            .run
            .as_ref()
            .unwrap()
            .budget
            .as_ref()
            .unwrap()
            .max_cost_usd,
        Some(3.0)
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn runner_pauses_on_cost_breach_before_job_and_records_action_command() {
    let ws = temp_ws("cost-breach");
    let (store, idea_id, run_id) = setup_queued_run_with_budget(
        &ws,
        Budget {
            max_cost_usd: Some(2.0),
            max_duration_secs: None,
            max_retries: None,
        },
    );

    // Emit telemetry usage event that exceeds the $2.00 cost cap.
    let sys = Actor {
        kind: ActorKind::System,
        id: "telemetry".to_string(),
    };
    let usage_ev = Event {
        v: 1,
        seq: 0,
        ts: 100.0,
        actor: sys,
        r#type: "telemetry.usage".to_string(),
        idea: idea_id.clone(),
        run: Some(run_id.clone()),
        data: serde_json::json!({
            "cost_usd": 2.50,
            "input_tokens": 100000,
            "output_tokens": 20000,
        }),
        prev: None,
    };
    let redacted_usage = tandem::factory::events::redact_event(
        usage_ev,
        &tandem::factory::redact::RedactContext::new(),
    );
    append(
        &store.root.join("events.jsonl"),
        &store.root.join(".lock"),
        &[redacted_usage],
    )
    .unwrap();

    let mut exec = SequenceExecutor {
        outcomes: vec![StageOutcome::Succeeded],
        calls: Vec::new(),
        on_call: None,
        store: store.clone(),
    };
    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();

    // Job must NOT have started.
    assert!(
        exec.calls.is_empty(),
        "expected no job to start on cost breach"
    );
    let run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(run.state, RunState::Paused);
    assert!(
        run.action_commands
            .iter()
            .any(|c| c.contains("tandem factory run budget") && c.contains(&run_id)),
        "expected budget action command, got {:?}",
        run.action_commands
    );

    let events = read_all(&store.root.join("events.jsonl")).unwrap().events;
    let paused = events
        .iter()
        .rev()
        .find(|e| e.r#type == "run.paused" && e.run.as_deref() == Some(&run_id))
        .expect("run.paused event");
    assert_eq!(paused.data["cap"], "cost");
    assert_eq!(paused.data["spent"], 2.5);
    assert_eq!(paused.data["limit"], 2.0);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn runner_pauses_on_duration_breach_after_job_and_stops_running_jobs() {
    let ws = temp_ws("dur-breach");
    let (store, idea_id, run_id) = setup_queued_run_with_budget(
        &ws,
        Budget {
            max_cost_usd: None,
            max_duration_secs: Some(60),
            max_retries: None,
        },
    );

    // During the job, simulate an earlier 120s stage duration event so after-job check breaches 60s.
    let mut exec = SequenceExecutor {
        outcomes: vec![StageOutcome::Succeeded],
        calls: Vec::new(),
        on_call: Some(Box::new({
            let idea_id = idea_id.clone();
            let run_id = run_id.clone();
            move |st, _job| {
                let sys = Actor {
                    kind: ActorKind::System,
                    id: "test".to_string(),
                };
                let s0 = Event::from_data(
                    100.0,
                    sys.clone(),
                    idea_id.clone(),
                    Some(run_id.clone()),
                    &EventData::StageStarted {
                        run: run_id.clone(),
                        stage: "pre".to_string(),
                        attempt: 1,
                        grant: None,
                    },
                )
                .unwrap();
                let s1 = Event::from_data(
                    220.0,
                    sys,
                    idea_id.clone(),
                    Some(run_id.clone()),
                    &EventData::StageSucceeded {
                        run: run_id.clone(),
                        stage: "pre".to_string(),
                    },
                )
                .unwrap();
                let redacted_s = tandem::factory::events::redact_events(
                    &[s0, s1],
                    &tandem::factory::redact::RedactContext::new(),
                );
                append(
                    &st.root.join("events.jsonl"),
                    &st.root.join(".lock"),
                    &redacted_s,
                )
                .unwrap();
            }
        })),
        store: store.clone(),
    };

    drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    assert_eq!(exec.calls.len(), 1);

    let run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(run.state, RunState::Paused);
    assert!(
        run.action_commands
            .iter()
            .any(|c| c.contains("tandem factory run budget") && c.contains(&run_id))
    );

    let events = read_all(&store.root.join("events.jsonl")).unwrap().events;
    let paused = events
        .iter()
        .rev()
        .find(|e| e.r#type == "run.paused" && e.run.as_deref() == Some(&run_id))
        .expect("run.paused event");
    assert_eq!(paused.data["cap"], "duration");
    assert!(paused.data["spent"].as_f64().unwrap() >= 120.0);
    assert_eq!(paused.data["limit"], 60.0);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn run_budget_command_is_user_only_validates_upper_bound_and_emits_budget_changed() {
    let ws = temp_ws("run-budget-cmd");
    let (store, _idea_id, run_id) = setup_queued_run_with_budget(
        &ws,
        Budget {
            max_cost_usd: Some(10.0),
            max_duration_secs: Some(3600),
            max_retries: Some(3),
        },
    );
    let user = user_actor();

    // Agent caller rejected.
    let agent_env = |k: &str| (k == "TANDEM_ROLE").then(|| "orchestrator".to_string());
    let agent_caller = tandem::factory::approval::Caller::cli(true, &agent_env, &|_| None);
    let user_caller = tandem::factory::approval::Caller::cli(true, &no_env, &|_| None);
    let err = change_run_budget(&store, &run_id, Some(5.0), None, None, &user, &agent_caller)
        .unwrap_err();
    assert!(matches!(
        err,
        RunnerError::Approval(ApprovalError::AgentCaller)
    ));

    // Above settings upper-bound rejected.
    let err = change_run_budget(&store, &run_id, Some(20.0), None, None, &user, &user_caller)
        .unwrap_err();
    assert!(
        err.to_string().contains("exceeds settings limit"),
        "got: {err}"
    );

    // Valid CLI invocation updates budget and emits budget.changed.
    let cli = Cli::try_parse_from([
        "tandem",
        "factory",
        "run",
        "budget",
        &run_id,
        "--max-cost",
        "6.5",
        "--max-duration",
        "1800",
        "--max-retries",
        "2",
    ])
    .unwrap();
    let Some(tandem::cli::Command::Factory(f_args)) = cli.command else {
        panic!("expected factory");
    };
    let cfg = Config {
        factory: true,
        ..Default::default()
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_with_caller(&f_args, &ws, &cfg, &user_caller, &mut stdout, &mut stderr);
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));

    let run = get_run(&store, &run_id).unwrap().unwrap();
    let b = run.budget.unwrap();
    assert_eq!(b.max_cost_usd, Some(6.5));
    assert_eq!(b.max_duration_secs, Some(1800));
    assert_eq!(b.max_retries, Some(2));

    let events = read_all(&store.root.join("events.jsonl")).unwrap().events;
    let changed = events
        .iter()
        .find(|e| e.r#type == "budget.changed" && e.run.as_deref() == Some(&run_id))
        .expect("budget.changed event");
    assert_eq!(changed.data["old"]["max_cost_usd"], 10.0);
    assert_eq!(changed.data["new"]["max_cost_usd"], 6.5);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn honest_limits_labels_cost_cap_unenforceable_for_provider_without_usage() {
    assert!(provider_reports_usage("claude"));
    assert!(!provider_reports_usage("agy"));

    let budget = RunBudget {
        max_cost_usd: Some(5.0),
        max_duration_secs: Some(3600),
        max_retries: Some(2),
    };
    let label_agy = cost_cap_label(&budget, Some("agy"));
    assert_eq!(label_agy, "not enforceable for agy");

    let label_claude = cost_cap_label(&budget, Some("claude"));
    assert_eq!(label_claude, "$5.00");

    // Check helper does not false-breach when spent.cost_usd is None.
    let res = check(
        "r-12345678",
        &budget,
        &Spent {
            cost_usd: None,
            duration_secs: 100,
            retries: 0,
        },
        true,
    );
    assert_eq!(res, BudgetCheck::Ok);
}

#[test]
fn estimate_routes_cover_every_stage_without_any_provider_consent() {
    use tandem::factory::budget::{provider_reports_usage, routes_for_workflow};
    use tandem::factory::workflow::DEFAULT_WORKFLOW_YAML;
    use tandem::factory::workflow::parse::parse_workflow_str;
    use tandem::factory::workflow::validate::validate;

    let settings = tandem::factory::model::FactorySettings::default();
    let raw = parse_workflow_str(DEFAULT_WORKFLOW_YAML).unwrap();
    let wf = validate(&raw, &settings).unwrap();

    let (routes, primary) = routes_for_workflow(
        &wf,
        &settings,
        &["claude", "agy", "opencode"],
        Some("claude".into()),
    );
    assert_eq!(primary.as_deref(), Some("claude"));
    for stage in ["plan", "develop", "review"] {
        assert!(
            routes.contains_key(stage),
            "no route for {stage}: {routes:?}"
        );
    }
    assert!(provider_reports_usage("claude"));
    assert!(!provider_reports_usage("agy"));
    assert!(!provider_reports_usage("opencode"));
}
