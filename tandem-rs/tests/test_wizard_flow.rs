use std::collections::HashMap;

use tandem::wizard::flow::{Step, WizardError, WizardFlow};

fn make_flow() -> WizardFlow {
    let mut installed = HashMap::new();
    installed.insert("claude".to_string(), true);
    installed.insert("agy".to_string(), true);
    WizardFlow::new(installed)
}

#[test]
fn test_claude_planner_skips_perms_and_cwd() {
    let mut f = make_flow();
    assert_eq!(f.step(), Step::Cli);
    f.choose_cli("claude").unwrap();
    f.choose_role("planner").unwrap();
    assert_eq!(f.step(), Step::Model);
    f.choose_model("opus").unwrap();
    assert_eq!(f.step(), Step::Summary);
    assert_eq!(f.agents.len(), 1);
    let a = &f.agents[0];
    assert_eq!(
        (a.cli.as_str(), a.role.as_str(), a.model.as_str(), a.index),
        ("claude", "planner", "opus", 1)
    );
}

#[test]
fn test_agy_asks_permissions_default_no() {
    let mut f = make_flow();
    f.choose_cli("agy").unwrap();
    f.choose_role("planner").unwrap();
    f.choose_model("gemini-3.8-flash-high").unwrap();
    assert_eq!(f.step(), Step::Perms);
    f.choose_skip_permissions(false);
    assert_eq!(f.step(), Step::Summary);
    assert!(!f.agents[0].skip_permissions);
}

#[test]
fn test_implementer_asks_for_cwd() {
    let dir = std::env::temp_dir();
    let mut f = make_flow();
    f.choose_cli("claude").unwrap();
    f.choose_role("implementer").unwrap();
    f.choose_model("sonnet").unwrap();
    assert_eq!(f.step(), Step::Cwd);
    f.choose_cwd(dir.to_str().unwrap()).unwrap();
    assert_eq!(f.step(), Step::Summary);
    assert_eq!(f.agents[0].cwd, Some(dir.to_str().unwrap().to_string()));
}

#[test]
fn test_blank_cwd_means_workspace_root() {
    let mut f = make_flow();
    f.choose_cli("claude").unwrap();
    f.choose_role("implementer").unwrap();
    f.choose_model("sonnet").unwrap();
    f.choose_cwd("").unwrap();
    assert_eq!(f.agents[0].cwd, None);
}

#[test]
fn test_missing_cwd_rejected() {
    let mut f = make_flow();
    f.choose_cli("claude").unwrap();
    f.choose_role("implementer").unwrap();
    f.choose_model("sonnet").unwrap();
    let bad_path = std::env::temp_dir().join("nonexistent_tandem_cwd_12345");
    let res = f.choose_cwd(bad_path.to_str().unwrap());
    assert_eq!(res, Err(WizardError::DirectoryNotFound));
    assert_eq!(f.step(), Step::Cwd);
}

#[test]
fn test_uninstalled_cli_cannot_be_chosen() {
    let mut installed = HashMap::new();
    installed.insert("claude".to_string(), true);
    installed.insert("agy".to_string(), false);
    let mut f = WizardFlow::new(installed);
    let res = f.choose_cli("agy");
    assert!(res.is_err());
}

#[test]
fn test_bad_model_rejected_and_step_kept() {
    let mut f = make_flow();
    f.choose_cli("claude").unwrap();
    f.choose_role("planner").unwrap();
    let res = f.choose_model("--evil");
    assert!(res.is_err());
    assert_eq!(f.step(), Step::Model);
}

#[test]
fn test_indexes_count_per_role_and_duplicates_allowed() {
    let mut f = make_flow();
    for _ in 0..2 {
        f.add_another();
        f.choose_cli("claude").unwrap();
        f.choose_role("planner").unwrap();
        f.choose_model("opus").unwrap();
    }
    let indexes: Vec<usize> = f.agents.iter().map(|a| a.index).collect();
    assert_eq!(indexes, vec![1, 2]);
}

#[test]
fn test_delete_renumbers() {
    let mut f = make_flow();
    for _ in 0..2 {
        f.add_another();
        f.choose_cli("claude").unwrap();
        f.choose_role("planner").unwrap();
        f.choose_model("opus").unwrap();
    }
    f.delete(0);
    let indexes: Vec<usize> = f.agents.iter().map(|a| a.index).collect();
    assert_eq!(indexes, vec![1]);
}

#[test]
fn test_edit_removes_and_restarts_flow() {
    let mut f = make_flow();
    f.choose_cli("claude").unwrap();
    f.choose_role("planner").unwrap();
    f.choose_model("opus").unwrap();
    f.edit(0);
    assert!(f.agents.is_empty());
    assert_eq!(f.step(), Step::Cli);
}

#[test]
fn test_back_goes_one_step() {
    let mut f = make_flow();
    f.choose_cli("claude").unwrap();
    f.choose_role("planner").unwrap();
    f.back();
    assert_eq!(f.step(), Step::Role);
    f.back();
    assert_eq!(f.step(), Step::Cli);
}

#[test]
fn test_launch_requires_an_agent() {
    let mut f = make_flow();
    assert!(!f.can_launch());
    f.choose_cli("claude").unwrap();
    f.choose_role("planner").unwrap();
    f.choose_model("opus").unwrap();
    assert!(f.can_launch());
}
