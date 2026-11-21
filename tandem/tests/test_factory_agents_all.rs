mod common;

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use common::TempDir;
use common::run_fixture::{setup_run, user_actor};
use tandem::factory::broker::job::job_dir;
use tandem::factory::model::{Run, RunState};
use tandem::factory::run::withdraw_run;
use tandem::factory::store::FactoryStore;
use tandem::factory::team::{
    GlobalAgentSummary, MAX_FINISHED_RUNS, MAX_RUNS_SCANNED, list_all_agents, select_runs,
};
use tandem::tmux::SESSION;
use tandem::tmux::client::{Result as TmuxCmdResult, TmuxClient, TmuxRunner};

#[derive(Clone, Default)]
struct PaneRunner {
    panes: Arc<Mutex<Vec<(String, String, String)>>>,
    calls: Arc<Mutex<Vec<Vec<String>>>>,
}

impl PaneRunner {
    fn add_worker(&self, pane: &str, run: &str, job: &str) {
        self.panes
            .lock()
            .unwrap()
            .push((pane.to_string(), run.to_string(), job.to_string()));
    }

    fn list_panes_calls(&self) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|argv| argv.iter().any(|a| a == "list-panes"))
            .count()
    }
}

impl TmuxRunner for PaneRunner {
    fn run(&self, argv: &[String]) -> TmuxCmdResult {
        self.calls.lock().unwrap().push(argv.to_vec());
        if argv.iter().any(|a| a == "list-panes") {
            let out = self
                .panes
                .lock()
                .unwrap()
                .iter()
                .map(|(pane, run, job)| {
                    [
                        pane.as_str(),
                        "worker",
                        "factory-worker",
                        "factory-worker",
                        "1",
                        "develop",
                        "0",
                        "0",
                        "",
                        "",
                        "",
                        "",
                        "",
                        run.as_str(),
                        job.as_str(),
                        "develop",
                        "claude",
                        "",
                    ]
                    .join("\t")
                })
                .collect::<Vec<_>>()
                .join("\n");
            return TmuxCmdResult {
                code: 0,
                out,
                err: String::new(),
            };
        }
        TmuxCmdResult {
            code: 0,
            out: String::new(),
            err: String::new(),
        }
    }
}

fn tmux_with(runner: &PaneRunner) -> TmuxClient {
    TmuxClient::with_runner(SESSION, None, Arc::new(runner.clone()))
}

fn write_job(store: &FactoryStore, run_id: &str, stage: &str, state: &str, pane: Option<&str>) {
    let dir = job_dir(store, run_id, stage, 1);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("input.json"),
        serde_json::json!({
            "correlation_id": format!("{run_id}.{stage}.1"),
            "run": run_id,
            "stage": stage,
            "attempt": 1,
            "agent": "senior-backend-dev",
            "provider": "claude",
            "model": "claude-sonnet-4-6",
            "tool_grants": ["repo_read"],
            "cwd": "/tmp",
            "time_limit_secs": 60,
            "context_manifest": { "items": [], "total_bytes": 0, "excluded_paths": [] }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(dir.join("state"), format!("{state}\n")).unwrap();
    if let Some(pane) = pane {
        fs::write(dir.join("pane_id"), format!("{pane}\n")).unwrap();
    }
}

fn write_selection(store: &FactoryStore, run_id: &str, stage: &str) {
    fs::write(
        store.run_dir(run_id).join("assignments.json"),
        serde_json::json!({
            "stages": {
                stage: {
                    "stage": stage,
                    "cli": "claude",
                    "model": "claude-sonnet-4-6",
                    "role": "senior-backend-dev",
                    "source": "default",
                    "updated": 1.0,
                    "updated_by": "system",
                    "selection": {
                        "required_tier": "standard",
                        "selected_tier": "economy",
                        "cost_basis": "price_table",
                        "candidates": 2,
                        "rationale": "cheapest capable",
                        "source": "live"
                    }
                }
            }
        })
        .to_string(),
    )
    .unwrap();
}

fn run_json(id: &str, state: &str, updated: f64) -> Run {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "idea": "i-12345678",
        "workflow": "default-delivery",
        "workflow_version": "1",
        "workflow_sha256": "abc",
        "state": state,
        "created": updated,
        "updated": updated,
    }))
    .unwrap()
}

fn workspace() -> (TempDir, FactoryStore) {
    let tmp = TempDir::new();
    let store = FactoryStore::new(tmp.path());
    store.ensure().unwrap();
    (tmp, store)
}

fn run_workspace(ws: &Path) -> (FactoryStore, String, String) {
    setup_run(ws)
}

#[test]
fn an_empty_workspace_lists_no_agents_and_makes_no_tmux_call() {
    let (_tmp, store) = workspace();
    let runner = PaneRunner::default();
    let list = list_all_agents(&store, &tmux_with(&runner), false).unwrap();
    assert!(list.is_empty());
    assert_eq!(runner.list_panes_calls(), 0);
}

#[test]
fn lists_every_job_with_its_idea_run_and_selection_tier() {
    let tmp = TempDir::new();
    let (store, idea_id, run_id) = run_workspace(tmp.path());
    write_job(&store, &run_id, "develop", "running", Some("%21"));
    write_job(&store, &run_id, "plan", "finished", None);
    write_selection(&store, &run_id, "develop");
    let runner = PaneRunner::default();
    runner.add_worker("%21", &run_id, &format!("{run_id}.develop.1"));

    let list = list_all_agents(&store, &tmux_with(&runner), false).unwrap();
    assert_eq!(list.len(), 2);
    let develop = list.iter().find(|a| a.agent.stage == "develop").unwrap();
    assert_eq!(develop.idea_id, idea_id);
    assert_eq!(develop.idea_title, "Export team activity as CSV");
    assert_eq!(develop.agent.run_id, run_id);
    assert_eq!(develop.agent.pane_id.as_deref(), Some("%21"));
    assert!(develop.agent.pane_alive);
    assert_eq!(
        serde_json::to_value(develop).unwrap()["tier"],
        serde_json::json!("economy")
    );
    let plan = list.iter().find(|a| a.agent.stage == "plan").unwrap();
    assert_eq!(plan.tier, None);
}

#[test]
fn active_keeps_only_running_jobs() {
    let tmp = TempDir::new();
    let (store, _idea, run_id) = run_workspace(tmp.path());
    write_job(&store, &run_id, "develop", "running", Some("%21"));
    write_job(&store, &run_id, "plan", "finished", None);
    let runner = PaneRunner::default();
    runner.add_worker("%21", &run_id, &format!("{run_id}.develop.1"));

    let list = list_all_agents(&store, &tmux_with(&runner), true).unwrap();
    let stages: Vec<_> = list.iter().map(|a| a.agent.stage.as_str()).collect();
    assert_eq!(stages, vec!["develop"]);
}

#[test]
fn a_running_job_whose_pane_died_is_listed_as_not_alive() {
    let tmp = TempDir::new();
    let (store, _idea, run_id) = run_workspace(tmp.path());
    write_job(&store, &run_id, "develop", "running", Some("%77"));
    let runner = PaneRunner::default();

    let list = list_all_agents(&store, &tmux_with(&runner), true).unwrap();
    assert_eq!(list.len(), 1);
    assert!(!list[0].agent.pane_alive);
    assert_eq!(list[0].agent.pane_id.as_deref(), Some("%77"));
}

#[test]
fn panes_of_other_runs_are_never_attributed_to_a_job() {
    let tmp = TempDir::new();
    let (store, _idea, run_id) = run_workspace(tmp.path());
    write_job(&store, &run_id, "develop", "running", Some("%21"));
    let runner = PaneRunner::default();
    runner.add_worker("%21", "r-other", &format!("{run_id}.develop.1"));

    let list = list_all_agents(&store, &tmux_with(&runner), true).unwrap();
    assert!(!list[0].agent.pane_alive);
}

#[test]
fn finished_runs_cost_no_tmux_calls_and_are_hidden_by_active() {
    let tmp = TempDir::new();
    let (store, _idea, run_id) = run_workspace(tmp.path());
    write_job(&store, &run_id, "develop", "finished", None);
    withdraw_run(&store, &run_id, &user_actor()).unwrap();
    let runner = PaneRunner::default();

    let all = list_all_agents(&store, &tmux_with(&runner), false).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(runner.list_panes_calls(), 0, "no scan for finished runs");

    let active = list_all_agents(&store, &tmux_with(&runner), true).unwrap();
    assert!(active.is_empty());
}

#[test]
fn an_active_run_triggers_exactly_one_tmux_scan() {
    let tmp = TempDir::new();
    let (store, _idea, run_id) = run_workspace(tmp.path());
    write_job(&store, &run_id, "develop", "running", Some("%21"));
    write_job(&store, &run_id, "plan", "finished", None);
    let runner = PaneRunner::default();

    list_all_agents(&store, &tmux_with(&runner), false).unwrap();
    assert_eq!(runner.list_panes_calls(), 1);
}

#[test]
fn run_selection_keeps_every_active_run_and_bounds_finished_ones() {
    let mut runs = Vec::new();
    for n in 0..(MAX_FINISHED_RUNS + 10) {
        runs.push(run_json(&format!("r-done{n:04}"), "succeeded", n as f64));
    }
    runs.push(run_json("r-live0001", "running", 1.0));
    runs.push(run_json("r-live0002", "paused", 2.0));

    let all = select_runs(&runs, false);
    assert_eq!(all.len(), MAX_FINISHED_RUNS + 2);
    assert!(all.iter().any(|r| r.id == "r-live0001"));
    assert!(all.iter().any(|r| r.id == "r-live0002"));
    let newest_done = format!("r-done{:04}", MAX_FINISHED_RUNS + 9);
    assert!(all.iter().any(|r| r.id == newest_done), "newest kept");
    assert!(!all.iter().any(|r| r.id == "r-done0000"), "oldest dropped");

    let active = select_runs(&runs, true);
    let ids: Vec<_> = active.iter().map(|r| r.state).collect();
    assert_eq!(ids.len(), 2);
    assert!(
        ids.iter()
            .all(|s| matches!(s, RunState::Running | RunState::Paused))
    );
}

#[test]
fn run_selection_never_scans_more_than_the_cap() {
    let runs: Vec<Run> = (0..(MAX_RUNS_SCANNED + 50))
        .map(|n| run_json(&format!("r-live{n:04}"), "running", n as f64))
        .collect();
    assert_eq!(select_runs(&runs, true).len(), MAX_RUNS_SCANNED);
}

#[test]
fn global_summaries_round_trip_through_json() {
    let tmp = TempDir::new();
    let (store, _idea, run_id) = run_workspace(tmp.path());
    write_job(&store, &run_id, "develop", "running", Some("%21"));
    let runner = PaneRunner::default();
    let list = list_all_agents(&store, &tmux_with(&runner), true).unwrap();

    let json = serde_json::to_string(&list).unwrap();
    let back: Vec<GlobalAgentSummary> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, list);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        value[0].get("job_id").is_some(),
        "agent fields are flattened"
    );
    assert!(value[0].get("idea_title").is_some());
}

#[test]
fn snapshot_counts_running_agents_across_ideas() {
    let tmp = TempDir::new();
    let (store, _idea, run_id) = run_workspace(tmp.path());
    write_job(&store, &run_id, "develop", "running", Some("%21"));

    let summary = tandem::snapshot::collect_factory_summary(tmp.path(), &[], 200.0);
    assert_eq!(summary.agents_active, 1);

    let old = serde_json::json!({ "enabled": true });
    let parsed: tandem::snapshot::FactorySummary = serde_json::from_value(old).unwrap();
    assert_eq!(parsed.agents_active, 0);
}
