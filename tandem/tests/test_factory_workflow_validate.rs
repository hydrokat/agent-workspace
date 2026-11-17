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

    let def = Cli::try_parse_from(["tandem", "factory", "workflow", "default"]).unwrap();
    assert!(matches!(
        def.command,
        Some(tandem::cli::Command::Factory(args))
            if matches!(args.command, FactoryCommand::Workflow(ref w) if matches!(w.command, FactoryWorkflowCommand::Default))
    ));
}

#[test]
fn test_default_workflow_is_valid_and_matches_template() {
    use tandem::factory::workflow::DEFAULT_WORKFLOW_YAML;

    let raw = parse_workflow_str(DEFAULT_WORKFLOW_YAML).expect("default workflow should parse");
    let wf = validate(&raw, &FactorySettings::default()).expect("default workflow should validate");
    assert_eq!(wf.name, "default-delivery");
    assert_eq!(wf.version, 1);
    assert_eq!(wf.stages.len(), 7);

    let demo = wf.stages.iter().find(|s| s.name == "demo").unwrap();
    assert_eq!(demo.depends_on, vec!["test"]);
    let deploy = wf.stages.iter().find(|s| s.name == "deploy").unwrap();
    assert_eq!(deploy.depends_on, vec!["review"]);

    let template_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("templates")
        .join("workflows")
        .join("default-delivery.yaml");
    let template = fs::read_to_string(&template_path).expect("template default-delivery.yaml should exist");
    assert_eq!(
        DEFAULT_WORKFLOW_YAML, template,
        "embedded default-delivery.yaml must be byte-identical to templates/workflows/default-delivery.yaml"
    );
}

#[test]
fn test_workflow_default_cli_prints_embedded_default() {
    use tandem::config::Config;
    use tandem::factory::commands::run_with_io;
    use tandem::factory::workflow::DEFAULT_WORKFLOW_YAML;

    let cli = Cli::try_parse_from(["tandem", "factory", "workflow", "default"]).unwrap();
    let Some(tandem::cli::Command::Factory(args)) = cli.command else {
        panic!("expected factory command");
    };
    let tmp = std::env::temp_dir().join(format!("tandem-wf-def-{}", std::process::id()));
    let _ = fs::create_dir_all(&tmp);
    let mut cfg = Config::default();
    cfg.state_dir = tmp.join("state");
    let mut stdin = std::io::empty();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_with_io(&args, &tmp, &cfg, &mut stdin, &mut stdout, &mut stderr);
    let _ = fs::remove_dir_all(&tmp);
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert_eq!(String::from_utf8(stdout).unwrap(), DEFAULT_WORKFLOW_YAML);
}

#[test]
fn test_load_default_workflow_falls_back_to_embedded() {
    use tandem::factory::workflow::{DEFAULT_WORKFLOW_YAML, WorkflowSource, load_default_workflow};

    let tmp = std::env::temp_dir().join(format!("tandem-wf-fallback-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    let (raw, source) = load_default_workflow(&tmp).expect("fallback to embedded should succeed");
    assert_eq!(source, WorkflowSource::Embedded);
    assert_eq!(raw.raw_text, DEFAULT_WORKFLOW_YAML);

    fs::create_dir_all(tmp.join("workflows")).unwrap();
    fs::write(tmp.join("workflows/default-delivery.yaml"), VALID_WORKFLOW).unwrap();
    let (raw2, source2) = load_default_workflow(&tmp).expect("workspace workflow should load");
    assert_eq!(source2, WorkflowSource::Workspace);
    assert_eq!(raw2.raw_text, VALID_WORKFLOW);

    let _ = fs::remove_dir_all(&tmp);
}
