use std::fs;
use std::path::PathBuf;

use tandem::factory::workflow::parse::{MAX_WORKFLOW_BYTES, ParseError, load_workflow_file};

const DEFAULT_WORKFLOW: &str = r#"name: default-delivery
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
    tools: [browser, screenshot]
  - name: deploy
    when: deploy_requested
    requires: [human_approval]
    agent: release-engineer
    tools: [ci, deployment]
"#;

fn temp_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(format!(
        "/tmp/tandem-wf-parse-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn test_loads_default_workflow_and_preserves_lines() {
    let dir = temp_dir("ok");
    let path = dir.join("default-delivery.yaml");
    fs::write(&path, DEFAULT_WORKFLOW).unwrap();

    let raw = load_workflow_file(&path).unwrap();
    assert_eq!(raw.name.as_ref().unwrap().value, "default-delivery");
    assert_eq!(raw.name.as_ref().unwrap().line, 1);
    assert_eq!(raw.stages.len(), 7);
    assert_eq!(raw.stages[0].name.as_ref().unwrap().value, "plan");
    assert_eq!(raw.stages[0].line, 9);
    assert_eq!(
        raw.stages[6].requires.as_ref().unwrap().items[0].value,
        "human_approval"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_rejects_unsafe_yaml_constructs_with_line_numbers() {
    let dir = temp_dir("bad");

    let cases = [
        ("alias", "name: &a foo\ntrigger: *a\n", "anchor"),
        (
            "billion",
            "name: x\nstages:\n  - name: &x foo\n    depends_on: [*x]\n",
            "anchor",
        ),
        ("tag", "name: !custom foo\n", "tag"),
        ("merge", "name: foo\n<<: bar\n", "merge"),
        ("multidoc", "name: foo\n---\nname: bar\n", "document"),
        ("dup", "name: foo\nname: bar\n", "duplicate"),
        (
            "unknown",
            "name: foo\nversion: 1\npolicy:\n  max_parallel_agents: 2\nstages:\n  - name: plan\n    agent: orchestrator\n    tool: [repo_read]\n",
            "tools",
        ),
    ];

    for (label, content, expected_substr) in cases {
        let p = dir.join(format!("{label}.yaml"));
        fs::write(&p, content).unwrap();
        let err = load_workflow_file(&p).unwrap_err();
        let msg = err.to_string().to_ascii_lowercase();
        assert!(
            msg.contains(expected_substr),
            "{label}: expected '{expected_substr}' in '{msg}'"
        );
    }

    // Reject oversized (> 64 KiB)
    let big = dir.join("big.yaml");
    fs::write(&big, vec![b'a'; MAX_WORKFLOW_BYTES + 1]).unwrap();
    assert!(matches!(
        load_workflow_file(&big),
        Err(ParseError::TooLarge { .. })
    ));

    // Reject directory
    assert!(load_workflow_file(&dir).is_err());

    // Reject symlink
    #[cfg(unix)]
    {
        let target = dir.join("target.yaml");
        fs::write(&target, DEFAULT_WORKFLOW).unwrap();
        let link = dir.join("link.yaml");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(matches!(
            load_workflow_file(&link),
            Err(ParseError::Symlink(_))
        ));
    }

    let _ = fs::remove_dir_all(&dir);
}
