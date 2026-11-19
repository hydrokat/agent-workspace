use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;

use tandem::coordination::store::CoordStore;
use tandem::factory::approval::{Caller, mark_ready_with_workflow};
use tandem::factory::artifacts;
use tandem::factory::broker::gate::{AuthType, consent_provider};
use tandem::factory::broker::job::{JobState, job_dir, set_job_state};
use tandem::factory::checks::ChecksConfig;
use tandem::factory::events::Event;
use tandem::factory::executor::{Executor, StageJob, StageOutcome};
use tandem::factory::machine::{Command, EventData};
use tandem::factory::model::{
    Actor, ActorKind, FactorySettings, IdeaState, RunState, Stage, StageState,
};
use tandem::factory::run::get_run;
use tandem::factory::runner::{drive_run_step, drive_workspace_step};
use tandem::factory::settings::save as save_settings;
use tandem::factory::store::FactoryStore;
use tandem::factory::workflow::scheduler::{RunFacts, SchedulerLimits, next};
use tandem::factory::workflow::schema::{
    Policy, StageBudget, StageDef, Workflow, WorkflowAgent, WorkflowTool, WorkflowTrigger,
};
use tandem::snapshot::collect_with_factory;

fn temp_ws(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-parallelism-{}-{}-{}",
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
    for args in [
        &["init", "-b", "trunk"][..],
        &["config", "user.email", "t@example.com"],
        &["config", "user.name", "Test"],
    ] {
        assert!(
            StdCommand::new("git")
                .args(args)
                .current_dir(dir)
                .status()
                .unwrap()
                .success()
        );
    }
    fs::write(dir.join("README.md"), "# Test\n").unwrap();
    for args in [&["add", "README.md"][..], &["commit", "-m", "init"]] {
        assert!(
            StdCommand::new("git")
                .args(args)
                .current_dir(dir)
                .status()
                .unwrap()
                .success()
        );
    }
}

fn add_queued_run(ws: &Path, store: &FactoryStore, title: &str) -> (String, String) {
    let user = Actor {
        kind: ActorKind::User,
        id: "alice".into(),
    };
    let orch = Actor {
        kind: ActorKind::Orchestrator,
        id: "orch".into(),
    };
    let idea = store
        .create_idea(title, "Execute stages", "alice", &user)
        .unwrap();
    store
        .execute(&idea.id, Command::StartDiscovery, &orch)
        .unwrap();
    let feas = ws.join(format!("f-{}.md", idea.id));
    fs::write(&feas, "# Feasible\n").unwrap();
    let feas_ref = artifacts::put(store, &idea.id, "feasibility", &feas).unwrap();
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
    let plan = ws.join(format!("p-{}.md", idea.id));
    fs::write(
        &plan,
        "# Plan\n\n## Goal\nRun\n\n## Acceptance Criteria\n- Works\n\n## Files\n- a\n\n## Tests\n- test\n",
    )
    .unwrap();
    let plan_ref = artifacts::put(store, &idea.id, "plan", &plan).unwrap();
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
    let caller = Caller::cli(true, &|_| None, &|_| None);
    let ready =
        mark_ready_with_workflow(store, ws, &idea.id, &idea.id, "alice", None, false, &caller)
            .unwrap();
    (idea.id, ready.run.unwrap().id)
}

fn setup_workspace(name: &str, max_parallel_agents: usize) -> (PathBuf, FactoryStore) {
    let ws = temp_ws(name);
    init_git_repo(&ws);
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();
    let mut settings = store.settings().unwrap();
    settings.max_parallel_agents = max_parallel_agents;
    settings.approval_grace_secs = 0;
    save_settings(&store, &settings).unwrap();
    let caller = Caller::cli(true, &|_| None, &|_| None);
    consent_provider(
        &store,
        "claude",
        AuthType::ApiKey,
        "claude",
        "alice",
        &caller,
    )
    .unwrap();
    (ws, store)
}

fn append_run_event(store: &FactoryStore, idea_id: &str, run_id: &str, ts: f64, data: EventData) {
    let sys = Actor {
        kind: ActorKind::System,
        id: "runner".into(),
    };
    let ev = Event::from_data(
        ts,
        sys,
        idea_id.to_string(),
        Some(run_id.to_string()),
        &data,
    )
    .unwrap();
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(store.root.join("events.jsonl"))
        .unwrap();
    writeln!(f, "{}", serde_json::to_string(&ev).unwrap()).unwrap();
    let _ = fs::remove_file(store.root.join("runs").join(run_id).join("run.json"));
}

fn append_stage_started(store: &FactoryStore, idea_id: &str, run_id: &str, stage: &str) {
    append_run_event(
        store,
        idea_id,
        run_id,
        10_000.0,
        EventData::StageStarted {
            run: run_id.to_string(),
            stage: stage.to_string(),
            attempt: 1,
            grant: None,
        },
    );
}

fn append_stage_succeeded(store: &FactoryStore, idea_id: &str, run_id: &str, stage: &str) {
    append_run_event(
        store,
        idea_id,
        run_id,
        20_000.0,
        EventData::StageSucceeded {
            run: run_id.to_string(),
            stage: stage.to_string(),
        },
    );
}

struct RecordingExecutor {
    calls: Vec<(String, String)>,
}

impl RecordingExecutor {
    fn new() -> Self {
        Self { calls: Vec::new() }
    }
}

impl Executor for RecordingExecutor {
    fn run(&mut self, job: &StageJob) -> StageOutcome {
        self.calls.push((job.run.clone(), job.stage.clone()));
        StageOutcome::Succeeded
    }
}

fn make_stages(wf: &Workflow, overrides: &[(&str, StageState, u32)]) -> Vec<Stage> {
    let map: HashMap<&str, (StageState, u32)> = overrides
        .iter()
        .map(|&(name, state, attempts)| (name, (state, attempts)))
        .collect();
    wf.stages
        .iter()
        .map(|s| {
            let (state, attempts) = map
                .get(s.name.as_str())
                .copied()
                .unwrap_or((StageState::Pending, 0));
            Stage {
                name: s.name.clone(),
                agent: s.agent.as_str().to_string(),
                state,
                attempts,
                interrupted_attempts: 0,
                started: None,
                finished: None,
                artifacts: vec![],
                rationale: None,
                retry_hypothesis: None,
            }
        })
        .collect()
}

fn stage_def(name: &str, tools: Vec<WorkflowTool>) -> StageDef {
    StageDef {
        name: name.to_string(),
        agent: WorkflowAgent::Implementer,
        depends_on: vec![],
        tools,
        when: None,
        requires: vec![],
        budget: StageBudget::default(),
        evidence: vec![],
    }
}

fn workflow_with_stages(max_parallel: u32, stages: Vec<StageDef>) -> Workflow {
    Workflow {
        name: "parallel-test".to_string(),
        version: 1,
        trigger: WorkflowTrigger::ReadyForDevelopment,
        policy: Policy {
            max_parallel_agents: max_parallel,
            default_temperature: 0.4,
            require_human_approval: vec![],
        },
        stages,
        sha256: "abc".to_string(),
        raw_text: String::new(),
    }
}

#[test]
fn test_one_writer_per_worktree_and_read_only_overlap() {
    let wf = workflow_with_stages(
        4,
        vec![
            stage_def(
                "write_a",
                vec![WorkflowTool::RepoRead, WorkflowTool::RepoWrite],
            ),
            stage_def("exec_b", vec![WorkflowTool::Shell]),
            stage_def("test_c", vec![WorkflowTool::TestRunner]),
            stage_def("lint_d", vec![WorkflowTool::LintRunner]),
            stage_def("read_e", vec![WorkflowTool::RepoRead]),
            stage_def(
                "read_f",
                vec![WorkflowTool::DiffReview, WorkflowTool::IssueContext],
            ),
        ],
    );
    let settings = FactorySettings {
        max_parallel_agents: 4,
        ..FactorySettings::default()
    };

    let stages = make_stages(&wf, &[]);
    let limits = SchedulerLimits::from_workflow_and_settings(&wf, &settings, &stages);
    let d = next(&wf, &stages, &RunFacts::default(), &limits);
    assert_eq!(d.start, vec!["write_a", "read_e", "read_f"]);

    let stages_writer_running = make_stages(&wf, &[("write_a", StageState::Running, 1)]);
    let limits_running =
        SchedulerLimits::from_workflow_and_settings(&wf, &settings, &stages_writer_running);
    let d2 = next(
        &wf,
        &stages_writer_running,
        &RunFacts::default(),
        &limits_running,
    );
    assert_eq!(d2.start, vec!["read_e", "read_f"]);

    let stages_exec_running = make_stages(
        &wf,
        &[
            ("write_a", StageState::Succeeded, 1),
            ("exec_b", StageState::Running, 1),
        ],
    );
    let limits_exec =
        SchedulerLimits::from_workflow_and_settings(&wf, &settings, &stages_exec_running);
    let d3 = next(
        &wf,
        &stages_exec_running,
        &RunFacts::default(),
        &limits_exec,
    );
    assert_eq!(d3.start, vec!["read_e", "read_f"]);
}

#[test]
fn test_workspace_wide_cap_blocks_excess_runs() {
    let (ws, store) = setup_workspace("cap", 3);
    let (idea1, run1) = add_queued_run(&ws, &store, "Idea 1");
    let (idea2, run2) = add_queued_run(&ws, &store, "Idea 2");
    let (_idea3, run3) = add_queued_run(&ws, &store, "Idea 3");

    let mut settings = store.settings().unwrap();
    settings.max_parallel_agents = 2;
    save_settings(&store, &settings).unwrap();

    append_stage_started(&store, &idea1, &run1, "plan");
    append_stage_started(&store, &idea2, &run2, "plan");

    let mut exec = RecordingExecutor::new();
    drive_run_step(&store, &ws, &run3, &mut exec).unwrap();
    assert!(exec.calls.is_empty());
    assert_eq!(
        get_run(&store, &run3).unwrap().unwrap().state,
        RunState::Queued
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_workspace_runs_served_in_queue_order() {
    let (ws, store) = setup_workspace("queue-order", 3);
    let (_idea1, run1) = add_queued_run(&ws, &store, "Oldest");
    let (_idea2, run2) = add_queued_run(&ws, &store, "Middle");
    let (_idea3, run3) = add_queued_run(&ws, &store, "Newest");

    let mut settings = store.settings().unwrap();
    settings.max_parallel_agents = 1;
    save_settings(&store, &settings).unwrap();

    let mut exec = RecordingExecutor::new();
    drive_workspace_step(&store, &ws, &mut exec).unwrap();

    assert_eq!(
        exec.calls,
        vec![
            (run1.clone(), "plan".to_string()),
            (run2.clone(), "plan".to_string()),
            (run3.clone(), "plan".to_string()),
        ]
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_live_lowering_stops_new_jobs_until_below_new_limit() {
    let (ws, store) = setup_workspace("live-lower", 3);
    let (idea1, run1) = add_queued_run(&ws, &store, "Idea 1");
    let (idea2, run2) = add_queued_run(&ws, &store, "Idea 2");
    let (_idea3, run3) = add_queued_run(&ws, &store, "Idea 3");

    append_stage_started(&store, &idea1, &run1, "plan");
    append_stage_started(&store, &idea2, &run2, "plan");

    let mut settings = store.settings().unwrap();
    settings.max_parallel_agents = 1;
    save_settings(&store, &settings).unwrap();

    let mut exec = RecordingExecutor::new();
    drive_run_step(&store, &ws, &run3, &mut exec).unwrap();
    assert!(exec.calls.is_empty());

    assert_eq!(
        get_run(&store, &run1)
            .unwrap()
            .unwrap()
            .stages
            .iter()
            .find(|s| s.name == "plan")
            .unwrap()
            .state,
        StageState::Running
    );
    assert_eq!(
        get_run(&store, &run2)
            .unwrap()
            .unwrap()
            .stages
            .iter()
            .find(|s| s.name == "plan")
            .unwrap()
            .state,
        StageState::Running
    );

    append_stage_succeeded(&store, &idea1, &run1, "plan");
    drive_run_step(&store, &ws, &run3, &mut exec).unwrap();
    assert!(exec.calls.is_empty());

    append_stage_succeeded(&store, &idea2, &run2, "plan");
    drive_run_step(&store, &ws, &run3, &mut exec).unwrap();
    assert_eq!(exec.calls, vec![(run3.clone(), "plan".to_string())]);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_single_run_default_delivery_unchanged() {
    let (ws, store) = setup_workspace("default-delivery", 3);
    let (idea_id, run_id) = add_queued_run(&ws, &store, "Single run");

    let mut settings = store.settings().unwrap();
    settings.checks.insert(
        "workspace".to_string(),
        ChecksConfig {
            test: vec![vec!["true".to_string()]],
            lint: vec![],
            timeout_secs: 30,
            ui_paths: vec![],
            confirmed_by: Some("alice".to_string()),
            confirmed_at: Some(1.0),
        },
    );
    save_settings(&store, &settings).unwrap();

    let mut exec = RecordingExecutor::new();
    for _ in 0..7 {
        drive_run_step(&store, &ws, &run_id, &mut exec).unwrap();
    }

    let executed_stages: Vec<&str> = exec.calls.iter().map(|(_, s)| s.as_str()).collect();
    assert_eq!(
        executed_stages,
        vec!["plan", "create-tests", "develop", "test", "review"]
    );
    let run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(run.state, RunState::Review);
    assert_eq!(
        store.get_idea(&idea_id).unwrap().unwrap().state,
        IdeaState::Review
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_snapshot_reflects_workspace_total_and_limit() {
    let (ws, store) = setup_workspace("snapshot-total", 3);
    let (idea1, run1) = add_queued_run(&ws, &store, "Idea 1");
    let (idea2, run2) = add_queued_run(&ws, &store, "Idea 2");

    append_stage_started(&store, &idea1, &run1, "plan");
    append_stage_started(&store, &idea2, &run2, "plan");

    let jdir1 = job_dir(&store, &run1, "plan", 1);
    fs::create_dir_all(&jdir1).unwrap();
    set_job_state(&jdir1, JobState::Running).unwrap();

    let jdir2 = job_dir(&store, &run2, "plan", 1);
    fs::create_dir_all(&jdir2).unwrap();
    set_job_state(&jdir2, JobState::Running).unwrap();

    let coord = CoordStore::new(ws.clone());
    let snap = collect_with_factory(&ws, &coord, Duration::from_secs(1), true);
    let factory = snap.factory.as_ref().unwrap();
    for card in &factory.ideas {
        let rs = card.run.as_ref().unwrap();
        assert_eq!(rs.running_jobs, 2);
        assert_eq!(rs.job_limit, 3);
    }

    let _ = fs::remove_dir_all(&ws);
}
