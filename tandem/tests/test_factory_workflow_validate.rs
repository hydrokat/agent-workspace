use std::fs;
use std::path::PathBuf;

use clap::Parser;
use tandem::cli::Cli;
use tandem::factory::commands::{FactoryCommand, FactoryWorkflowCommand};
use tandem::factory::model::FactorySettings;
use tandem::factory::workflow::parse::parse_workflow_str;
use tandem::factory::workflow::validate::{resolve_workflow_path, validate};

const VALID_WORKFLOW: &str = r#"name: default-delivery
version: 1
trigger: ready-for-development
policy:
  max_parallel_agents: 3
  default_temperature: 0.4
  require_human_approval: [production_deploy]
stages:
  - name: plan
    agent: orchestrator
    tools: [repo_read, issue_context]
  - name: create-tests
    agent: test-engineer
    depends_on: [plan]
    tools: [repo_read, repo_write, test_runner]
  - name: develop
    agent: implementer
    depends_on: [create-tests]
    tools: [repo_read, repo_write, shell]
  - name: test
    agent: verifier
    depends_on: [develop]
    tools: [test_runner, lint_runner]
  - name: review
    agent: reviewer
    depends_on: [test]
    tools: [repo_read, diff_review]
  - name: demo
    when: ui_changed
    agent: demo-maker
    depends_on: [test]
    tools: [browser, screenshot]
  - name: deploy
    when: deploy_requested
    requires: [human_approval]
    agent: release-engineer
    depends_on: [review]
    tools: [ci, deployment]
"#;

fn assert_code(yaml: &str, code: &str, settings: &FactorySettings) {
    let raw = parse_workflow_str(yaml).expect("parse should succeed");
    let diags = validate(&raw, settings).expect_err("expected validation diagnostics");
    assert!(
        diags.iter().any(|d| d.code == code),
        "expected diagnostic {code}, got: {diags:?}"
    );
}

#[test]
fn test_valid_workflow_passes() {
    let raw = parse_workflow_str(VALID_WORKFLOW).unwrap();
    let wf = validate(&raw, &FactorySettings::default()).unwrap();
    assert_eq!(wf.name, "default-delivery");
    assert_eq!(wf.version, 1);
    assert_eq!(wf.stages.len(), 7);
}

#[test]
fn test_rules_w001_through_w013() {
    let s = FactorySettings::default();

    // W001: bad name or non-positive version
    assert_code(
        &VALID_WORKFLOW.replace("name: default-delivery", "name: BadName"),
        "W001",
        &s,
    );
    assert_code(
        &VALID_WORKFLOW.replace("version: 1", "version: 0"),
        "W001",
        &s,
    );

    // W002: unknown trigger
    assert_code(
        &VALID_WORKFLOW.replace("trigger: ready-for-development", "trigger: on-push"),
        "W002",
        &s,
    );

    // W003: duplicate stage name or bad stage name
    assert_code(
        &VALID_WORKFLOW.replace("- name: develop", "- name: plan"),
        "W003",
        &s,
    );
    assert_code(
        &VALID_WORKFLOW.replace("- name: develop", "- name: Bad_Stage"),
        "W003",
        &s,
    );

    // W004: unknown depends_on
    assert_code(
        &VALID_WORKFLOW.replace("depends_on: [plan]", "depends_on: [missing]"),
        "W004",
        &s,
    );

    // W005: cycle
    let cycle = r#"name: cyc
version: 1
trigger: ready-for-development
policy:
  max_parallel_agents: 2
  default_temperature: 0.4
stages:
  - name: root
    agent: orchestrator
    tools: [repo_read]
  - name: a
    agent: implementer
    depends_on: [root, b]
    tools: [test_runner]
  - name: b
    agent: reviewer
    depends_on: [a]
    tools: [diff_review]
"#;
    assert_code(cycle, "W005", &s);

    // W006: two root stages (0 or >1 without depends_on)
    assert_code(
        &VALID_WORKFLOW.replace("    depends_on: [test]\n", ""),
        "W006",
        &s,
    );

    // W007: unknown agent, tool, condition, requirement
    assert_code(
        &VALID_WORKFLOW.replace("agent: reviewer", "agent: wizard"),
        "W007",
        &s,
    );
    assert_code(
        &VALID_WORKFLOW.replace("tools: [repo_read, issue_context]", "tools: [magic_wand]"),
        "W007",
        &s,
    );
    assert_code(
        &VALID_WORKFLOW.replace("when: ui_changed", "when: moon_phase"),
        "W007",
        &s,
    );
    assert_code(
        &VALID_WORKFLOW.replace("requires: [human_approval]", "requires: [coffee]"),
        "W007",
        &s,
    );

    // W008: deployment without requires: [human_approval]
    assert_code(
        &VALID_WORKFLOW.replace("    requires: [human_approval]\n", ""),
        "W008",
        &s,
    );

    // W009: max_parallel_agents above factory setting
    let mut strict = FactorySettings::default();
    strict.max_parallel_agents = 2;
    assert_code(VALID_WORKFLOW, "W009", &strict);

    // W010: temperature out of 0.0..=1.0
    assert_code(
        &VALID_WORKFLOW.replace("default_temperature: 0.4", "default_temperature: 1.5"),
        "W010",
        &s,
    );

    // W011: missing test_runner or diff_review
    assert_code(
        &VALID_WORKFLOW.replace("diff_review", "repo_read"),
        "W011",
        &s,
    );

    // W013: deployment without production_deploy in require_human_approval
    assert_code(
        &VALID_WORKFLOW.replace(
            "require_human_approval: [production_deploy]",
            "require_human_approval: []",
        ),
        "W013",
        &s,
    );
}

#[test]
fn test_workflow_name_path_resolution_is_contained() {
    let ws = PathBuf::from("/tmp/tandem-wf-resolve");
    let _ = fs::create_dir_all(ws.join("workflows"));
    assert!(resolve_workflow_path(&ws, "default-delivery").is_ok());
    assert!(resolve_workflow_path(&ws, "../secret").is_err());
    assert!(resolve_workflow_path(&ws, "/etc/passwd").is_err());
}

#[test]
fn test_workflow_cli_parse() {
    let val = Cli::try_parse_from([
        "tandem",
        "factory",
        "workflow",
        "validate",
        "default-delivery",
        "--json",
    ])
    .unwrap();
    assert!(matches!(
        val.command,
        Some(tandem::cli::Command::Factory(args))
            if matches!(args.command, FactoryCommand::Workflow(ref w) if matches!(w.command, FactoryWorkflowCommand::Validate(_)))
    ));

    let list = Cli::try_parse_from(["tandem", "factory", "workflow", "list", "--json"]).unwrap();
    assert!(matches!(
        list.command,
        Some(tandem::cli::Command::Factory(args))
            if matches!(args.command, FactoryCommand::Workflow(ref w) if matches!(w.command, FactoryWorkflowCommand::List(_)))
    ));
}
