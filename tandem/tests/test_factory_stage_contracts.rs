use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use tandem::factory::broker::git::AllowedGitSubcommand;
use tandem::factory::broker::result::{JobResult, JobResultStatus};
use tandem::factory::checks::{ChecksConfig, ConfirmedChecks};
use tandem::factory::stages::scope::{ScopeCheckInput, verify_scope};
use tandem::factory::stages::{StageContext, commit_stage_changes, contract_for_stage};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-stage-contracts-{}-{}-{}",
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

fn git(dir: &Path, args: &[&str]) -> String {
    let out = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn init_git_repo(dir: &Path) {
    git(dir, &["init", "-b", "trunk"]);
    git(dir, &["config", "user.email", "dev@example.com"]);
    git(dir, &["config", "user.name", "Repo Author"]);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(dir.join("tests")).unwrap();
    fs::write(
        dir.join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
    )
    .unwrap();
    fs::write(dir.join("tests/basic.rs"), "// baseline\n").unwrap();
    git(dir, &["add", "."]);
    git(dir, &["commit", "-m", "init"]);
}

fn confirmed_checks_with(test_cmd: Vec<String>) -> ConfirmedChecks {
    let cfg = ChecksConfig {
        test: vec![test_cmd],
        lint: vec![],
        timeout_secs: 30,
        ui_paths: vec![],
        confirmed_by: Some("alice".to_string()),
        confirmed_at: Some(1000.0),
    };
    ConfirmedChecks::from_config("workspace", &cfg).unwrap()
}

struct StageFixture {
    root: PathBuf,
    artifact_dir: PathBuf,
    repo_dir: PathBuf,
    plan_snapshot_path: PathBuf,
}

impl StageFixture {
    fn new(name: &str) -> Self {
        let root = temp_dir(name);
        let artifact_dir = root.join("artifacts");
        let repo_dir = root.join("repo");
        fs::create_dir_all(&artifact_dir).unwrap();
        fs::create_dir_all(&repo_dir).unwrap();
        init_git_repo(&repo_dir);

        let plan_snapshot_path = root.join("plan-snapshot.md");
        fs::write(
            &plan_snapshot_path,
            "# Plan\n\n## Acceptance criteria\n- Works\n",
        )
        .unwrap();

        fs::write(
            artifact_dir.join("slices.json"),
            r#"{
  "phase": "p-001-demo",
  "slices": [
    {
      "id": "t-001",
      "title": "Feature slice",
      "type": "New Feature",
      "paths": ["src/lib.rs", "tests/"],
      "dependencies": [],
      "acceptance_criteria": ["Works"],
      "ui": false
    }
  ]
}"#,
        )
        .unwrap();

        Self {
            root,
            artifact_dir,
            repo_dir,
            plan_snapshot_path,
        }
    }

    fn base_result() -> JobResult {
        JobResult {
            status: JobResultStatus::Succeeded,
            summary: "done".to_string(),
            files_changed: vec![],
            artifacts: vec![],
            rationale: None,
            hypothesis: None,
            usage: None,
            no_behaviour_change: false,
        }
    }
}

impl Drop for StageFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn git_subcommands_allow_add_and_commit_but_never_push() {
    assert_eq!(
        AllowedGitSubcommand::parse("add"),
        Some(AllowedGitSubcommand::Add)
    );
    assert_eq!(
        AllowedGitSubcommand::parse("commit"),
        Some(AllowedGitSubcommand::Commit)
    );
    assert_eq!(
        AllowedGitSubcommand::parse("status"),
        Some(AllowedGitSubcommand::Status)
    );
    assert_eq!(
        AllowedGitSubcommand::parse("diff"),
        Some(AllowedGitSubcommand::Diff)
    );
    assert_eq!(
        AllowedGitSubcommand::parse("push"),
        Some(AllowedGitSubcommand::Push)
    );
    assert_eq!(AllowedGitSubcommand::parse("merge"), None);
    assert_eq!(AllowedGitSubcommand::Add.as_str(), "add");
    assert_eq!(AllowedGitSubcommand::Commit.as_str(), "commit");
}

#[test]
fn scope_check_rejects_out_of_slice_and_lists_offending_files() {
    let fx = StageFixture::new("scope-out");
    fs::write(fx.repo_dir.join("README.md"), "stray change\n").unwrap();

    let err = verify_scope(&ScopeCheckInput {
        worktree_path: &fx.repo_dir,
        artifact_dir: &fx.artifact_dir,
        has_repo_write: true,
    })
    .unwrap_err();
    assert!(err.detail.contains("README.md"), "got: {}", err.detail);
}

#[test]
fn scope_check_allows_lock_files_but_rejects_protected_paths() {
    let fx = StageFixture::new("scope-protected");
    fs::write(fx.repo_dir.join("Cargo.lock"), "# lock\n").unwrap();
    let ok = verify_scope(&ScopeCheckInput {
        worktree_path: &fx.repo_dir,
        artifact_dir: &fx.artifact_dir,
        has_repo_write: true,
    });
    assert!(ok.is_ok(), "lockfile should be allowed: {ok:?}");

    fs::create_dir_all(fx.repo_dir.join(".github/workflows")).unwrap();
    fs::write(fx.repo_dir.join(".github/workflows/ci.yml"), "name: ci\n").unwrap();
    fs::create_dir_all(fx.repo_dir.join(".agent-workspace")).unwrap();
    fs::write(fx.repo_dir.join(".agent-workspace/settings.json"), "{}\n").unwrap();

    let err = verify_scope(&ScopeCheckInput {
        worktree_path: &fx.repo_dir,
        artifact_dir: &fx.artifact_dir,
        has_repo_write: true,
    })
    .unwrap_err();
    assert!(
        err.detail.contains(".github/workflows/ci.yml"),
        "got: {}",
        err.detail
    );
    assert!(
        err.detail.contains(".agent-workspace/settings.json"),
        "got: {}",
        err.detail
    );
}

#[test]
fn scope_check_rejects_changes_from_read_only_stage() {
    let fx = StageFixture::new("scope-readonly");
    fs::write(fx.repo_dir.join("src/lib.rs"), "pub fn changed() {}\n").unwrap();

    let err = verify_scope(&ScopeCheckInput {
        worktree_path: &fx.repo_dir,
        artifact_dir: &fx.artifact_dir,
        has_repo_write: false,
    })
    .unwrap_err();
    assert!(err.detail.contains("src/lib.rs"), "got: {}", err.detail);
}

#[test]
fn create_tests_fails_when_no_test_files_changed() {
    let fx = StageFixture::new("ct-no-tests");
    fs::write(fx.repo_dir.join("src/lib.rs"), "pub fn x() {}\n").unwrap();
    let checks = confirmed_checks_with(vec!["false".to_string()]);
    let result = StageFixture::base_result();

    let contract = contract_for_stage("create-tests").expect("create-tests contract registered");
    let ctx = StageContext {
        run_id: "r-001",
        idea_id: "idea-001",
        stage: "create-tests",
        attempt: 1,
        has_repo_write: true,
        artifact_dir: &fx.artifact_dir,
        target_repo: &fx.repo_dir,
        worktree_path: &fx.repo_dir,
        plan_snapshot_path: &fx.plan_snapshot_path,
        confirmed_checks: Some(&checks),
        result: &result,
    };
    let err = contract.verify(&ctx).unwrap_err();
    assert_eq!(err.check, "create_tests_files");
}

#[test]
fn create_tests_fails_when_tests_pass_without_no_behaviour_change() {
    let fx = StageFixture::new("ct-tests-pass");
    fs::write(fx.repo_dir.join("tests/feature_test.rs"), "// new test\n").unwrap();
    let checks = confirmed_checks_with(vec!["true".to_string()]);
    let result = StageFixture::base_result();

    let contract = contract_for_stage("create-tests").unwrap();
    let ctx = StageContext {
        run_id: "r-001",
        idea_id: "idea-001",
        stage: "create-tests",
        attempt: 1,
        has_repo_write: true,
        artifact_dir: &fx.artifact_dir,
        target_repo: &fx.repo_dir,
        worktree_path: &fx.repo_dir,
        plan_snapshot_path: &fx.plan_snapshot_path,
        confirmed_checks: Some(&checks),
        result: &result,
    };
    let err = contract.verify(&ctx).unwrap_err();
    assert_eq!(err.check, "create_tests_must_fail");
}

#[test]
fn create_tests_succeeds_when_tests_fail_and_records_evidence() {
    let fx = StageFixture::new("ct-tests-fail");
    fs::write(
        fx.repo_dir.join("tests/feature_test.rs"),
        "// failing test\n",
    )
    .unwrap();
    let checks = confirmed_checks_with(vec!["false".to_string()]);
    let result = StageFixture::base_result();

    let contract = contract_for_stage("create-tests").unwrap();
    let ctx = StageContext {
        run_id: "r-001",
        idea_id: "idea-001",
        stage: "create-tests",
        attempt: 1,
        has_repo_write: true,
        artifact_dir: &fx.artifact_dir,
        target_repo: &fx.repo_dir,
        worktree_path: &fx.repo_dir,
        plan_snapshot_path: &fx.plan_snapshot_path,
        confirmed_checks: Some(&checks),
        result: &result,
    };
    contract.verify(&ctx).unwrap();
    let evidence = fs::read_to_string(fx.artifact_dir.join("create-tests-evidence.json")).unwrap();
    assert!(evidence.contains("tests/feature_test.rs"));
}

#[test]
fn create_tests_allows_no_behaviour_change_with_rationale_and_rejects_without() {
    let fx = StageFixture::new("ct-no-behav");
    fs::write(fx.repo_dir.join("tests/basic.rs"), "// refactor\n").unwrap();
    let checks = confirmed_checks_with(vec!["true".to_string()]);

    let mut missing_rationale = StageFixture::base_result();
    missing_rationale.no_behaviour_change = true;
    let contract = contract_for_stage("create-tests").unwrap();
    let err = contract
        .verify(&StageContext {
            run_id: "r-001",
            idea_id: "idea-001",
            stage: "create-tests",
            attempt: 1,
            has_repo_write: true,
            artifact_dir: &fx.artifact_dir,
            target_repo: &fx.repo_dir,
            worktree_path: &fx.repo_dir,
            plan_snapshot_path: &fx.plan_snapshot_path,
            confirmed_checks: Some(&checks),
            result: &missing_rationale,
        })
        .unwrap_err();
    assert_eq!(err.check, "create_tests_rationale");

    let mut with_rationale = StageFixture::base_result();
    with_rationale.no_behaviour_change = true;
    with_rationale.rationale = Some("Internal refactor preserving existing behaviour".to_string());
    contract
        .verify(&StageContext {
            run_id: "r-001",
            idea_id: "idea-001",
            stage: "create-tests",
            attempt: 1,
            has_repo_write: true,
            artifact_dir: &fx.artifact_dir,
            target_repo: &fx.repo_dir,
            worktree_path: &fx.repo_dir,
            plan_snapshot_path: &fx.plan_snapshot_path,
            confirmed_checks: Some(&checks),
            result: &with_rationale,
        })
        .unwrap();
    let evidence = fs::read_to_string(fx.artifact_dir.join("create-tests-evidence.json")).unwrap();
    assert!(evidence.contains("no_behaviour_change"));
    assert!(evidence.contains("Internal refactor preserving existing behaviour"));
}

#[test]
fn develop_rejects_empty_diff_and_deleted_or_unexplained_test_changes() {
    let fx = StageFixture::new("dev-contract");
    let develop = contract_for_stage("develop").expect("develop contract registered");
    let result = StageFixture::base_result();

    let empty_err = develop
        .verify(&StageContext {
            run_id: "r-001",
            idea_id: "idea-001",
            stage: "develop",
            attempt: 1,
            has_repo_write: true,
            artifact_dir: &fx.artifact_dir,
            target_repo: &fx.repo_dir,
            worktree_path: &fx.repo_dir,
            plan_snapshot_path: &fx.plan_snapshot_path,
            confirmed_checks: None,
            result: &result,
        })
        .unwrap_err();
    assert_eq!(empty_err.check, "develop_non_empty_diff");

    // Simulate create-tests having written and committed tests/feature_test.rs.
    fs::write(fx.repo_dir.join("tests/feature_test.rs"), "// test\n").unwrap();
    git(&fx.repo_dir, &["add", "tests/feature_test.rs"]);
    git(
        &fx.repo_dir,
        &["commit", "-m", "test(idea-001): create-tests"],
    );
    fs::write(
        fx.artifact_dir.join("create-tests-evidence.json"),
        r#"{"test_files":["tests/feature_test.rs"],"no_behaviour_change":false,"rationale":null}"#,
    )
    .unwrap();

    // Deleting the test file must fail.
    fs::remove_file(fx.repo_dir.join("tests/feature_test.rs")).unwrap();
    fs::write(fx.repo_dir.join("src/lib.rs"), "pub fn impl_fn() {}\n").unwrap();
    let del_err = develop
        .verify(&StageContext {
            run_id: "r-001",
            idea_id: "idea-001",
            stage: "develop",
            attempt: 1,
            has_repo_write: true,
            artifact_dir: &fx.artifact_dir,
            target_repo: &fx.repo_dir,
            worktree_path: &fx.repo_dir,
            plan_snapshot_path: &fx.plan_snapshot_path,
            confirmed_checks: None,
            result: &result,
        })
        .unwrap_err();
    assert_eq!(del_err.check, "develop_tests_preserved");
    assert!(del_err.detail.contains("tests/feature_test.rs"));

    // Modifying the test file without rationale must fail.
    fs::write(fx.repo_dir.join("tests/feature_test.rs"), "// weakened\n").unwrap();
    let mod_err = develop
        .verify(&StageContext {
            run_id: "r-001",
            idea_id: "idea-001",
            stage: "develop",
            attempt: 1,
            has_repo_write: true,
            artifact_dir: &fx.artifact_dir,
            target_repo: &fx.repo_dir,
            worktree_path: &fx.repo_dir,
            plan_snapshot_path: &fx.plan_snapshot_path,
            confirmed_checks: None,
            result: &result,
        })
        .unwrap_err();
    assert_eq!(mod_err.check, "develop_tests_preserved");

    // Modifying the test file WITH rationale succeeds and records evidence.
    let mut with_rationale = StageFixture::base_result();
    with_rationale.rationale = Some("Updated test fixture import path".to_string());
    develop
        .verify(&StageContext {
            run_id: "r-001",
            idea_id: "idea-001",
            stage: "develop",
            attempt: 1,
            has_repo_write: true,
            artifact_dir: &fx.artifact_dir,
            target_repo: &fx.repo_dir,
            worktree_path: &fx.repo_dir,
            plan_snapshot_path: &fx.plan_snapshot_path,
            confirmed_checks: None,
            result: &with_rationale,
        })
        .unwrap();
    let dev_evidence = fs::read_to_string(fx.artifact_dir.join("develop-evidence.json")).unwrap();
    assert!(dev_evidence.contains("tests/feature_test.rs"));
    assert!(dev_evidence.contains("Updated test fixture import path"));
}

#[test]
fn commit_stage_changes_commits_in_scope_files_with_conventional_message() {
    let fx = StageFixture::new("commit-stage");
    fs::write(fx.repo_dir.join("src/lib.rs"), "pub fn impl_fn() {}\n").unwrap();

    commit_stage_changes(
        &fx.repo_dir,
        "idea-042",
        "develop",
        &["src/lib.rs".to_string()],
    )
    .unwrap();

    let subject = git(&fx.repo_dir, &["log", "-1", "--format=%s"]);
    assert!(subject.contains("idea-042"), "subject: {subject}");
    assert!(subject.contains("develop"), "subject: {subject}");
    assert!(subject.starts_with("feat(idea-042):"), "subject: {subject}");

    let author = git(&fx.repo_dir, &["log", "-1", "--format=%an <%ae>"]);
    assert_eq!(author.trim(), "Repo Author <dev@example.com>");

    let status = git(&fx.repo_dir, &["status", "--porcelain"]);
    assert!(
        status.trim().is_empty(),
        "worktree should be clean: {status}"
    );
}
