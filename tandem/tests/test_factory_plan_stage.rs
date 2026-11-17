use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use tandem::factory::broker::result::{JobResult, JobResultStatus};
use tandem::factory::stages::{StageContext, contract_for_stage};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-plan-stage-{}-{}-{}",
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
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("src/lib.rs"), "pub fn ok() {}\n").unwrap();
    assert!(
        StdCommand::new("git")
            .args(["add", "."])
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

struct PlanFixture {
    root: PathBuf,
    artifact_dir: PathBuf,
    repo_dir: PathBuf,
    plan_snapshot_path: PathBuf,
    result: JobResult,
}

impl PlanFixture {
    fn new(name: &str) -> Self {
        let root = temp_dir(name);
        let artifact_dir = root.join("artifacts");
        let repo_dir = root.join("repo");
        fs::create_dir_all(artifact_dir.join("specs/p-001-demo")).unwrap();
        fs::create_dir_all(&repo_dir).unwrap();
        init_git_repo(&repo_dir);

        let plan_snapshot_path = root.join("plan-snapshot.md");
        fs::write(
            &plan_snapshot_path,
            "# Plan\n\n## Scope\nAdd feature\n\n## Acceptance criteria\n- Criterion one\n- Criterion two\n\n## Risks\nLow\n\n## Estimates\n1 day\n\n## Workflow recommendation\ndefault-delivery\n",
        )
        .unwrap();

        fs::write(artifact_dir.join("specs/p-001-demo/impl.md"), "# Impl\n").unwrap();
        fs::write(artifact_dir.join("specs/p-001-demo/t-001.md"), "# t-001\n").unwrap();
        fs::write(artifact_dir.join("specs/p-001-demo/t-002.md"), "# t-002\n").unwrap();

        fs::write(
            artifact_dir.join("slices.json"),
            r#"{
  "phase": "p-001-demo",
  "slices": [
    {
      "id": "t-001",
      "title": "First slice",
      "type": "New Feature",
      "paths": ["src/lib.rs"],
      "dependencies": [],
      "acceptance_criteria": ["Criterion one"],
      "ui": false
    },
    {
      "id": "t-002",
      "title": "Second slice",
      "type": "Enhancement",
      "paths": ["src/ui.rs"],
      "dependencies": ["t-001"],
      "acceptance_criteria": ["Criterion two"],
      "ui": true
    }
  ]
}"#,
        )
        .unwrap();

        let result = JobResult {
            status: JobResultStatus::Succeeded,
            summary: "Planned phase p-001-demo".to_string(),
            files_changed: vec![],
            artifacts: vec![
                "specs/p-001-demo/impl.md".to_string(),
                "specs/p-001-demo/t-001.md".to_string(),
                "specs/p-001-demo/t-002.md".to_string(),
                "slices.json".to_string(),
            ],
            rationale: None,
            hypothesis: None,
            usage: None,
        };

        Self {
            root,
            artifact_dir,
            repo_dir,
            plan_snapshot_path,
            result,
        }
    }

    fn verify(&self) -> Result<(), tandem::factory::stages::ContractFailure> {
        let contract = contract_for_stage("plan").expect("plan stage contract registered");
        let ctx = StageContext {
            run_id: "r-001",
            stage: "plan",
            artifact_dir: &self.artifact_dir,
            target_repo: &self.repo_dir,
            worktree_path: &self.repo_dir,
            plan_snapshot_path: &self.plan_snapshot_path,
            result: &self.result,
        };
        contract.verify(&ctx)
    }
}

impl Drop for PlanFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn plan_contract_succeeds_when_all_checks_pass() {
    let fx = PlanFixture::new("ok");
    fx.verify().expect("valid plan stage should pass");
}

#[test]
fn plan_contract_fails_when_slices_schema_is_invalid() {
    let fx = PlanFixture::new("bad-schema");
    fs::write(fx.artifact_dir.join("slices.json"), r#"{"slices": []}"#).unwrap();
    let err = fx.verify().unwrap_err();
    assert_eq!(err.check, "slices_schema");
    assert!(err.to_string().contains("slices_schema"));
}

#[test]
fn plan_contract_fails_when_acceptance_criterion_is_uncovered() {
    let fx = PlanFixture::new("missing-ac");
    fs::write(
        fx.artifact_dir.join("slices.json"),
        r#"{
  "phase": "p-001-demo",
  "slices": [
    {
      "id": "t-001",
      "title": "Only first",
      "type": "New Feature",
      "paths": ["src/lib.rs"],
      "dependencies": [],
      "acceptance_criteria": ["Criterion one"],
      "ui": false
    }
  ]
}"#,
    )
    .unwrap();
    let err = fx.verify().unwrap_err();
    assert_eq!(err.check, "acceptance_criteria_coverage");
    assert!(err.detail.contains("Criterion two"));
}

#[test]
fn plan_contract_fails_when_slice_path_escapes_target_repo() {
    let fx = PlanFixture::new("escape-path");
    fs::write(
        fx.artifact_dir.join("slices.json"),
        r#"{
  "phase": "p-001-demo",
  "slices": [
    {
      "id": "t-001",
      "title": "Escape",
      "type": "New Feature",
      "paths": ["../outside.rs"],
      "dependencies": [],
      "acceptance_criteria": ["Criterion one", "Criterion two"],
      "ui": false
    }
  ]
}"#,
    )
    .unwrap();
    let err = fx.verify().unwrap_err();
    assert_eq!(err.check, "slice_paths_in_target");
    assert!(err.detail.contains("../outside.rs"));
}

#[test]
fn plan_contract_fails_when_named_spec_file_is_missing() {
    let fx = PlanFixture::new("missing-spec");
    fs::remove_file(fx.artifact_dir.join("specs/p-001-demo/t-002.md")).unwrap();
    let err = fx.verify().unwrap_err();
    assert_eq!(err.check, "result_spec_files_exist");
    assert!(err.detail.contains("t-002.md"));
}

#[test]
fn plan_contract_fails_when_worktree_has_uncommitted_changes() {
    let fx = PlanFixture::new("dirty-wt");
    fs::write(fx.repo_dir.join("src/lib.rs"), "pub fn modified() {}\n").unwrap();
    let err = fx.verify().unwrap_err();
    assert_eq!(err.check, "worktree_unchanged");
}
