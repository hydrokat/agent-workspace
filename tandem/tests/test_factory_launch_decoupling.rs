mod common;

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use common::TempDir;
use tandem::adapters::AgentSpec;
use tandem::cli::select_launch_specs;
use tandem::commands::{resume_specs, resume_specs_for, store_snapshot};
use tandem::state::{SavedState, load, state_path_for};
use tandem::wizard::app::WizardApp;
use tandem::wizard::flow::{Step, WizardFlow};

fn which_all(_: &str) -> Option<PathBuf> {
    Some(PathBuf::from("/bin/true"))
}

fn installed_all() -> HashMap<String, bool> {
    ["claude", "agy", "opencode"]
        .iter()
        .map(|c| (c.to_string(), true))
        .collect()
}

fn mixed_state() -> SavedState {
    let mut orch = AgentSpec::new("claude", "orchestrator", "opus");
    orch.idea = Some("i-7k2m9q4d".to_string());
    SavedState::new(
        vec![
            AgentSpec::new("claude", "planner", "opus"),
            orch,
            AgentSpec::new("agy", "implementer", "m"),
        ],
        Default::default(),
        None,
        None,
        0.0,
    )
}

#[test]
fn test_factory_launch_never_runs_tandem_wizard() {
    let mut called = false;
    let out = select_launch_specs(true, None, || {
        called = true;
        Ok(None)
    })
    .unwrap();
    assert!(!called, "factory launch must not open the Tandem wizard");
    assert_eq!(out, Some(Vec::new()));
}

#[test]
fn test_factory_launch_keeps_resumed_orchestrators() {
    let specs = vec![AgentSpec::new("claude", "orchestrator", "opus")];
    let out = select_launch_specs(true, Some(specs.clone()), || panic!("no wizard")).unwrap();
    assert_eq!(out, Some(specs));
}

#[test]
fn test_normal_launch_still_runs_wizard_and_rejects_empty() {
    let mut called = false;
    let out = select_launch_specs(false, None, || {
        called = true;
        Ok(Some(vec![AgentSpec::new("claude", "planner", "opus")]))
    })
    .unwrap();
    assert!(called);
    assert_eq!(out.map(|s| s.len()), Some(1));

    let cancelled = select_launch_specs(false, None, || Ok(Some(Vec::new()))).unwrap();
    assert_eq!(cancelled, None);

    let cancelled = select_launch_specs(false, None, || Ok(None)).unwrap();
    assert_eq!(cancelled, None);

    let err = select_launch_specs(false, None, || Err(io::Error::other("boom")));
    assert!(err.is_err());
}

#[test]
fn test_factory_resume_drops_non_orchestrator_panes_with_notice() {
    let st = mixed_state();
    let (specs, notes) = resume_specs_for(&st, std::path::Path::new("/w"), which_all, true);
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].role, "orchestrator");
    assert!(
        notes
            .iter()
            .any(|n| n.contains("planner") && n.contains("Factory")),
        "notes: {notes:?}"
    );
    assert!(notes.iter().any(|n| n.contains("implementer")));
}

#[test]
fn test_normal_resume_keeps_every_role() {
    let st = mixed_state();
    let (specs, notes) = resume_specs(&st, std::path::Path::new("/w"), which_all);
    assert_eq!(specs.len(), 3);
    assert!(notes.is_empty());
}

#[test]
fn test_wizard_app_factory_session_locks_role_and_back_returns_to_cli() {
    use std::sync::Arc;
    let loader: tandem::wizard::app::ModelLoader = Arc::new(|_| Ok(Vec::new()));
    let app = WizardApp::new_for_session(installed_all(), loader.clone(), true, true);
    assert_eq!(app.flow.step(), Step::Cli);
    let normal = WizardApp::new_for_session(installed_all(), loader, true, false);
    assert_eq!(normal.flow.draft_role(), "planner");
    assert_eq!(app.flow.draft_role(), "orchestrator");
}

#[test]
fn test_factory_flow_back_from_model_returns_to_cli() {
    let mut f = WizardFlow::new_for_session(installed_all(), true);
    f.choose_cli("opencode").unwrap();
    assert_eq!(f.step(), Step::Model);
    f.back();
    assert_eq!(f.step(), Step::Cli);

    let mut n = WizardFlow::new(installed_all());
    n.choose_cli("claude").unwrap();
    n.choose_role("planner").unwrap();
    assert_eq!(n.step(), Step::Model);
    n.back();
    assert_eq!(n.step(), Step::Role);
}

#[test]
fn test_factory_snapshot_with_zero_orchestrators_saves_cleanly() {
    let tmp = TempDir::new();
    let ws = tmp.path();
    let path = state_path_for(ws, true);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "stale").unwrap();

    let empty = SavedState::new(Vec::new(), Default::default(), None, None, 0.0);
    let saved = store_snapshot(ws, empty.clone(), true).expect("factory empty ok");
    assert!(saved.agents.is_empty());
    assert!(load(&path).is_none(), "no stale resume state left behind");

    let err = store_snapshot(ws, empty, false).unwrap_err();
    assert!(err.to_string().contains("no agents to save"));
}
