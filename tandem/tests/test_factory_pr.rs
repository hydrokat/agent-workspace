use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;

use tandem::factory::approval::mark_ready;
use tandem::factory::artifacts;
use tandem::factory::broker::checks::CheckOutcome;
use tandem::factory::broker::gh::AllowedGhSubcommand;
use tandem::factory::broker::git::{AllowedGitSubcommand, push_branch, run_git};
use tandem::factory::checks::CheckKind;
use tandem::factory::events::read_all;
use tandem::factory::evidence::{
    record_committed_diff_stat, record_create_tests_exception, record_review_findings,
    record_test_stage_evidence,
};
use tandem::factory::machine::{Command, EventData};
use tandem::factory::model::{Actor, ActorKind, PrLink, Risk, RunState};
use tandem::factory::pr::{PrOutcome, open_pull_request, open_pull_request_with_gh};
use tandem::factory::run::{CreateRunOptions, create_run, get_run};
use tandem::factory::stages::review::{
    Finding, FindingSeverity, FindingsReport, ReviewVerdict, record_review_round,
};
use tandem::factory::stages::test::TestStageReport;
use tandem::factory::store::FactoryStore;
use tandem::factory::workspace::prepare_worktree;

fn temp_ws(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-pr-{}-{}-{}",
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

fn enable_factory_mode(ws: &Path) {
    fs::create_dir_all(ws.join(".agent-workspace")).unwrap();
    fs::write(ws.join(".agent-workspace").join("factory-mode"), "1\n").unwrap();
}

fn user_actor() -> Actor {
    Actor {
        kind: ActorKind::User,
        id: "user".to_string(),
    }
}

fn orch_actor() -> Actor {
    Actor {
        kind: ActorKind::Orchestrator,
        id: "orch-1".to_string(),
    }
}

fn sys_actor() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "runner".to_string(),
    }
}

fn run_dir(store: &FactoryStore, run_id: &str) -> PathBuf {
    store.root.join("runs").join(run_id)
}

fn init_git_repo(dir: &Path, branch: &str) {
    fs::create_dir_all(dir).unwrap();
    assert!(
        StdCommand::new("git")
            .args(["init", "-b", branch])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    fs::write(dir.join("README.md"), "# Test Repo\n").unwrap();
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

fn init_bare_remote(dir: &Path, branch: &str) {
    fs::create_dir_all(dir).unwrap();
    assert!(
        StdCommand::new("git")
            .args(["init", "--bare", "-b", branch])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
}

fn write_stub_gh(bin_dir: &Path, script_body: &str) -> PathBuf {
    fs::create_dir_all(bin_dir).unwrap();
    let gh_path = bin_dir.join("gh");
    fs::write(&gh_path, format!("#!/bin/sh\n{script_body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&gh_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    gh_path
}

fn setup_factory_run(ws: &Path, title: &str) -> (FactoryStore, String, String, PathBuf) {
    enable_factory_mode(ws);
    init_git_repo(ws, "main");
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();

    let idea = store
        .create_idea(title, "Implement feature safely", "user", &user_actor())
        .unwrap();
    store
        .execute(&idea.id, Command::StartDiscovery, &orch_actor())
        .unwrap();

    let feas_src = ws.join("feas.md");
    fs::write(&feas_src, "# Feasibility\n\nFeasible.\n").unwrap();
    let feas_ref = artifacts::put(&store, &idea.id, "feasibility", &feas_src).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: feas_ref,
                verdict: Some(tandem::factory::model::FeasibilityVerdict::Feasible),
                risk: Some(Risk::Low),
                workflow: None,
                missing_sections: vec![],
            },
            &orch_actor(),
        )
        .unwrap();

    let plan_src = ws.join("plan.md");
    fs::write(
        &plan_src,
        "## Scope\nA\n## Acceptance criteria\nB\n## Risks\nC\n## Estimates\nD\n## Workflow recommendation\ndefault-delivery\n",
    )
    .unwrap();
    let plan_ref = artifacts::put(&store, &idea.id, "plan", &plan_src).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".to_string(),
                artifact: plan_ref,
                verdict: None,
                risk: None,
                workflow: Some("default-delivery".to_string()),
                missing_sections: vec![],
            },
            &orch_actor(),
        )
        .unwrap();
    store
        .execute(&idea.id, Command::SelectDirection, &user_actor())
        .unwrap();

    mark_ready(&store, &idea.id, &idea.id, "cli", "user", &|_| None).unwrap();

    let run = create_run(
        &store,
        ws,
        &idea.id,
        CreateRunOptions::default(),
        &user_actor(),
    )
    .unwrap();

    let run_ws = prepare_worktree(&store, ws, &run.id).unwrap();
    (store, idea.id, run.id, run_ws.worktree_path)
}

fn commit_change_in_worktree(worktree: &Path, filename: &str, content: &str) {
    fs::write(worktree.join(filename), content).unwrap();
    assert!(
        StdCommand::new("git")
            .args(["add", filename])
            .current_dir(worktree)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args(["commit", "-m", "feat: implement change"])
            .current_dir(worktree)
            .status()
            .unwrap()
            .success()
    );
}

fn record_complete_evidence_and_approve(
    store: &FactoryStore,
    idea_id: &str,
    run_id: &str,
    worktree: &Path,
) {
    let artifact_dir = run_dir(store, run_id);

    let round1 = FindingsReport {
        verdict: ReviewVerdict::ChangesRequested,
        findings: vec![Finding {
            id: "F-1".to_string(),
            severity: FindingSeverity::Major,
            file: "src/lib.rs".to_string(),
            line: 10,
            title: "Missing check".to_string(),
            detail: "Need check".to_string(),
            suggestion: "Add check".to_string(),
        }],
    };
    record_review_round(&artifact_dir, 1, &round1).unwrap();

    let round2 = FindingsReport {
        verdict: ReviewVerdict::Approve,
        findings: vec![],
    };
    fs::write(
        artifact_dir.join("findings.json"),
        serde_json::to_vec_pretty(&round2).unwrap(),
    )
    .unwrap();
    record_review_round(&artifact_dir, 2, &round2).unwrap();

    record_create_tests_exception(
        store,
        idea_id,
        run_id,
        1,
        "docs-only change with no behaviour change",
        &sys_actor(),
    )
    .unwrap();

    record_committed_diff_stat(store, idea_id, run_id, "develop", 1, worktree, &sys_actor())
        .unwrap();

    let log_file = artifact_dir.join("checks").join("test-1.log");
    fs::create_dir_all(log_file.parent().unwrap()).unwrap();
    fs::write(&log_file, "RAW SECRET LOG ghp_secret1234567890abcdef\n").unwrap();

    let report = TestStageReport {
        outcomes: vec![
            CheckOutcome {
                kind: CheckKind::Test,
                argv: vec!["cargo".into(), "test".into()],
                exit_code: Some(0),
                duration_ms: 120,
                timed_out: false,
                log_cap_exceeded: false,
                log_path: log_file.clone(),
                summary: Some("2 passed".into()),
                error: None,
            },
            CheckOutcome {
                kind: CheckKind::Lint,
                argv: vec!["cargo".into(), "clippy".into()],
                exit_code: Some(0),
                duration_ms: 80,
                timed_out: false,
                log_cap_exceeded: false,
                log_path: log_file,
                summary: None,
                error: None,
            },
        ],
        all_passed: true,
        failures_summary: String::new(),
    };
    record_test_stage_evidence(store, idea_id, run_id, 1, &report, &sys_actor()).unwrap();

    record_review_findings(
        store,
        idea_id,
        run_id,
        2,
        b"{\"verdict\":\"approve\",\"findings\":[]}",
        "approve: 0 findings".to_string(),
        &sys_actor(),
    )
    .unwrap();

    let ev = tandem::factory::events::Event::from_data(
        1.0,
        sys_actor(),
        idea_id.to_string(),
        Some(run_id.to_string()),
        &EventData::StageSkipped {
            run: run_id.to_string(),
            stage: "create-tests".to_string(),
            reason: "no_behaviour_change".to_string(),
        },
    )
    .unwrap();
    let _ = tandem::factory::events::append(
        &store.root.join("events.jsonl"),
        &store.root.join("lock"),
        &[ev],
    );
}

#[test]
fn gh_subcommand_allowlist_forbids_merge() {
    assert_eq!(
        AllowedGhSubcommand::parse("auth status"),
        Some(AllowedGhSubcommand::AuthStatus)
    );
    assert_eq!(
        AllowedGhSubcommand::parse("repo view"),
        Some(AllowedGhSubcommand::RepoView)
    );
    assert_eq!(
        AllowedGhSubcommand::parse("pr create"),
        Some(AllowedGhSubcommand::PrCreate)
    );
    assert_eq!(
        AllowedGhSubcommand::parse("pr view"),
        Some(AllowedGhSubcommand::PrView)
    );
    assert_eq!(
        AllowedGhSubcommand::parse("pr list"),
        Some(AllowedGhSubcommand::PrList)
    );
    assert_eq!(AllowedGhSubcommand::parse("pr merge"), None);
    assert_eq!(AllowedGhSubcommand::parse("pr edit"), None);
    assert_eq!(AllowedGhSubcommand::parse("api"), None);
}

#[test]
fn git_push_refuses_protected_branches_and_force_flags() {
    let dir = temp_ws("push-guard");
    init_git_repo(&dir, "main");
    assert_eq!(
        AllowedGitSubcommand::parse("push"),
        Some(AllowedGitSubcommand::Push)
    );

    assert!(push_branch(&dir, "main", Duration::from_secs(5)).is_err());
    assert!(push_branch(&dir, "master", Duration::from_secs(5)).is_err());
    assert!(push_branch(&dir, "trunk", Duration::from_secs(5)).is_err());

    assert!(
        run_git(
            AllowedGitSubcommand::Push,
            &["--force", "origin", "feat/x"],
            &dir,
            Duration::from_secs(5),
        )
        .is_err()
    );
    assert!(
        run_git(
            AllowedGitSubcommand::Push,
            &["-f", "origin", "feat/x"],
            &dir,
            Duration::from_secs(5),
        )
        .is_err()
    );
    assert!(
        run_git(
            AllowedGitSubcommand::Push,
            &["-u", "origin", "main"],
            &dir,
            Duration::from_secs(5),
        )
        .is_err()
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn precondition_pauses_when_review_not_approved() {
    let ws = temp_ws("no-approve");
    let (store, _idea_id, run_id, worktree) = setup_factory_run(&ws, "Add PR support");
    commit_change_in_worktree(&worktree, "change.txt", "hello\n");

    let artifact_dir = run_dir(&store, &run_id);
    let report = FindingsReport {
        verdict: ReviewVerdict::ChangesRequested,
        findings: vec![],
    };
    fs::write(
        artifact_dir.join("findings.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();

    let outcome = open_pull_request(&store, &ws, &run_id, &sys_actor()).unwrap();
    assert!(matches!(outcome, PrOutcome::Paused { .. }));

    let run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(run.state, RunState::Paused);
    assert!(
        run.pause_reason
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains("approve")
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn precondition_pauses_when_evidence_incomplete() {
    let ws = temp_ws("incomplete-evidence");
    let (store, _idea_id, run_id, worktree) = setup_factory_run(&ws, "Add PR support");
    commit_change_in_worktree(&worktree, "change.txt", "hello\n");

    let artifact_dir = run_dir(&store, &run_id);
    let report = FindingsReport {
        verdict: ReviewVerdict::Approve,
        findings: vec![],
    };
    fs::write(
        artifact_dir.join("findings.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();

    let outcome = open_pull_request(&store, &ws, &run_id, &sys_actor()).unwrap();
    assert!(matches!(outcome, PrOutcome::Paused { .. }));

    let run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(run.state, RunState::Paused);
    assert!(
        run.pause_reason
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains("evidence")
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn precondition_pauses_when_no_commits_or_dirty_worktree() {
    let ws = temp_ws("dirty-or-empty");
    let (store, idea_id, run_id, worktree) = setup_factory_run(&ws, "Add PR support");

    record_complete_evidence_and_approve(&store, &idea_id, &run_id, &worktree);

    let outcome_no_commit = open_pull_request(&store, &ws, &run_id, &sys_actor()).unwrap();
    assert!(matches!(outcome_no_commit, PrOutcome::Paused { .. }));

    commit_change_in_worktree(&worktree, "change.txt", "hello\n");
    fs::write(worktree.join("untracked.txt"), "dirty\n").unwrap();

    let outcome_dirty = open_pull_request(&store, &ws, &run_id, &sys_actor()).unwrap();
    assert!(matches!(outcome_dirty, PrOutcome::Paused { .. }));

    let run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(run.state, RunState::Paused);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn precondition_pauses_when_remote_missing_or_not_github() {
    let ws = temp_ws("bad-remote");
    let (store, idea_id, run_id, worktree) = setup_factory_run(&ws, "Add PR support");
    commit_change_in_worktree(&worktree, "change.txt", "hello\n");
    record_complete_evidence_and_approve(&store, &idea_id, &run_id, &worktree);

    let run = get_run(&store, &run_id).unwrap().unwrap();
    let branch = run.branch.clone().unwrap();

    let outcome_missing = open_pull_request(&store, &ws, &run_id, &sys_actor()).unwrap();
    match outcome_missing {
        PrOutcome::Paused { reason, .. } => {
            assert!(reason.contains("open the pull request by hand"), "{reason}");
            assert!(reason.contains(&branch), "{reason}");
        }
        other => panic!("expected Paused, got {other:?}"),
    }

    assert!(
        StdCommand::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://gitlab.com/acme/widget.git"
            ])
            .current_dir(&worktree)
            .status()
            .unwrap()
            .success()
    );
    let outcome_gitlab = open_pull_request(&store, &ws, &run_id, &sys_actor()).unwrap();
    match outcome_gitlab {
        PrOutcome::Paused { reason, .. } => {
            assert!(reason.contains("open the pull request by hand"), "{reason}");
            assert!(reason.contains(&branch), "{reason}");
        }
        other => panic!("expected Paused, got {other:?}"),
    }

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn precondition_pauses_when_gh_missing_or_unauthenticated() {
    let ws = temp_ws("gh-auth");
    let (store, idea_id, run_id, worktree) = setup_factory_run(&ws, "Add PR support");
    commit_change_in_worktree(&worktree, "change.txt", "hello\n");
    record_complete_evidence_and_approve(&store, &idea_id, &run_id, &worktree);

    assert!(
        StdCommand::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/acme/widget.git"
            ])
            .current_dir(&worktree)
            .status()
            .unwrap()
            .success()
    );

    let bin_dir = ws.join("stub-bin");
    let gh_path = write_stub_gh(
        &bin_dir,
        r#"
if [ "$1" = "auth" ] && [ "$2" = "status" ]; then
  echo "not logged in" >&2
  exit 1
fi
exit 0
"#,
    );

    let outcome = open_pull_request_with_gh(&store, &ws, &run_id, &sys_actor(), &gh_path).unwrap();
    match outcome {
        PrOutcome::Paused { reason, commands } => {
            assert!(reason.contains("run gh auth login"), "{reason}");
            assert!(
                commands.iter().any(|c| c.contains("gh auth login")),
                "{commands:?}"
            );
        }
        other => panic!("expected Paused, got {other:?}"),
    }

    let missing_gh = ws.join("nonexistent-bin").join("gh");
    let outcome_missing =
        open_pull_request_with_gh(&store, &ws, &run_id, &sys_actor(), &missing_gh).unwrap();
    match outcome_missing {
        PrOutcome::Paused { reason, commands } => {
            assert!(reason.contains("run gh auth login"), "{reason}");
            assert!(
                commands.iter().any(|c| c.contains("gh auth login")),
                "{commands:?}"
            );
        }
        other => panic!("expected Paused, got {other:?}"),
    }

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn happy_path_pushes_creates_pr_writes_redacted_body_and_is_idempotent() {
    let ws = temp_ws("happy");
    let bare = temp_ws("bare");
    init_bare_remote(&bare, "main");

    let (store, idea_id, run_id, worktree) = setup_factory_run(&ws, "Add pull request integration");
    commit_change_in_worktree(&worktree, "feature.rs", "pub fn ok() -> bool { true }\n");
    record_complete_evidence_and_approve(&store, &idea_id, &run_id, &worktree);

    assert!(
        StdCommand::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/acme/widget.git"
            ])
            .current_dir(&worktree)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args([
                "remote",
                "set-url",
                "--push",
                "origin",
                &bare.display().to_string()
            ])
            .current_dir(&worktree)
            .status()
            .unwrap()
            .success()
    );

    let state_dir = ws.join("gh-state");
    fs::create_dir_all(&state_dir).unwrap();
    let log_file = state_dir.join("gh.log");
    let created_flag = state_dir.join("created");
    let captured_body = state_dir.join("body.md");
    let captured_title = state_dir.join("title.txt");

    let bin_dir = ws.join("stub-bin");
    let script = format!(
        r#"
echo "$@" >> "{log}"
case "$1 $2" in
  "auth status")
    exit 0
    ;;
  "repo view")
    echo '{{"nameWithOwner":"acme/widget","defaultBranchRef":{{"name":"main"}}}}'
    exit 0
    ;;
  "pr list")
    if [ -f "{created}" ]; then
      echo '[{{"number":42,"url":"https://github.com/acme/widget/pull/42"}}]'
    else
      echo '[]'
    fi
    exit 0
    ;;
  "pr create")
    touch "{created}"
    while [ $# -gt 0 ]; do
      case "$1" in
        --body-file)
          cp "$2" "{body}"
          shift 2
          ;;
        --title)
          printf '%s' "$2" > "{title}"
          shift 2
          ;;
        *)
          shift
          ;;
      esac
    done
    echo "https://github.com/acme/widget/pull/42"
    exit 0
    ;;
  "pr view")
    echo '{{"number":42,"url":"https://github.com/acme/widget/pull/42"}}'
    exit 0
    ;;
  *)
    echo "unexpected gh command: $@" >&2
    exit 1
    ;;
esac
"#,
        log = log_file.display(),
        created = created_flag.display(),
        body = captured_body.display(),
        title = captured_title.display(),
    );
    let gh_path = write_stub_gh(&bin_dir, &script);

    let outcome = open_pull_request_with_gh(&store, &ws, &run_id, &sys_actor(), &gh_path).unwrap();
    let expected_pr = PrLink {
        repo: "acme/widget".to_string(),
        number: 42,
        url: "https://github.com/acme/widget/pull/42".to_string(),
        state: None,
        checks_summary: None,
    };
    assert_eq!(outcome, PrOutcome::Created(expected_pr.clone()));

    let run = get_run(&store, &run_id).unwrap().unwrap();
    let branch = run.branch.clone().unwrap();

    let bare_branch = StdCommand::new("git")
        .args([
            "--git-dir",
            &bare.display().to_string(),
            "rev-parse",
            "--verify",
            &format!("refs/heads/{branch}"),
        ])
        .output()
        .unwrap();
    assert!(
        bare_branch.status.success(),
        "run branch not pushed to bare remote"
    );

    assert_eq!(run.prs, vec![expected_pr.clone()]);
    let idea = store.get_idea(&idea_id).unwrap().unwrap();
    assert_eq!(idea.prs, vec![expected_pr.clone()]);
    let events = read_all(&store.root.join("events.jsonl")).unwrap().events;
    assert!(events.iter().any(|e| e.r#type == "pr.created"));

    let title_text = fs::read_to_string(&captured_title).unwrap();
    assert!(title_text.starts_with("feat: "), "title was: {title_text}");

    let body_text = fs::read_to_string(&captured_body).unwrap();
    for section in [
        "## What changed",
        "## Tests",
        "## Review",
        "## Risks",
        "## Follow-up",
        "## Recorded exceptions",
    ] {
        assert!(
            body_text.contains(section),
            "missing {section} in:\n{body_text}"
        );
    }
    assert!(
        body_text.contains(&format!(
            "Factory idea: {idea_id} Add pull request integration"
        )),
        "missing Factory idea footer in:\n{body_text}"
    );
    assert!(
        body_text.contains("cargo test"),
        "missing test command in:\n{body_text}"
    );
    assert!(
        body_text.contains("cargo clippy"),
        "missing lint command in:\n{body_text}"
    );
    assert!(
        !body_text.contains("ghp_secret"),
        "secret leaked in:\n{body_text}"
    );
    assert!(
        !body_text.contains("RAW SECRET LOG"),
        "raw log leaked in:\n{body_text}"
    );
    assert!(
        !body_text.contains(&ws.display().to_string()),
        "local path leaked in:\n{body_text}"
    );

    let outcome_retry =
        open_pull_request_with_gh(&store, &ws, &run_id, &sys_actor(), &gh_path).unwrap();
    assert_eq!(outcome_retry, PrOutcome::AlreadyExists(expected_pr));

    let gh_log = fs::read_to_string(&log_file).unwrap();
    let create_calls = gh_log
        .lines()
        .filter(|l| l.starts_with("pr create"))
        .count();
    assert_eq!(
        create_calls, 1,
        "expected exactly 1 pr create call, log:\n{gh_log}"
    );
    assert!(
        !gh_log.contains("pr merge"),
        "pr merge must never be called"
    );

    let idea_after = store.get_idea(&idea_id).unwrap().unwrap();
    let run_after = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(idea_after.prs.len(), 1);
    assert_eq!(run_after.prs.len(), 1);

    let _ = fs::remove_dir_all(&ws);
    let _ = fs::remove_dir_all(&bare);
}

#[test]
fn pr_reads_review_artifacts_from_artifacts_subdir_and_includes_counts() {
    let ws = temp_ws("artifacts-review");
    let bare = temp_ws("bare-artifacts");
    init_bare_remote(&bare, "main");

    let (store, idea_id, run_id, worktree) = setup_factory_run(&ws, "Artifacts review path");
    commit_change_in_worktree(&worktree, "feature.rs", "pub fn ok() -> bool { true }\n");
    record_complete_evidence_and_approve(&store, &idea_id, &run_id, &worktree);

    let r_dir = run_dir(&store, &run_id);
    let nested_artifacts = r_dir.join("artifacts");
    fs::create_dir_all(&nested_artifacts).unwrap();
    for name in ["findings.json", "review-rounds.json"] {
        fs::rename(r_dir.join(name), nested_artifacts.join(name)).unwrap();
    }

    assert!(
        StdCommand::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/acme/widget.git"
            ])
            .current_dir(&worktree)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        StdCommand::new("git")
            .args([
                "remote",
                "set-url",
                "--push",
                "origin",
                &bare.display().to_string()
            ])
            .current_dir(&worktree)
            .status()
            .unwrap()
            .success()
    );

    let state_dir = ws.join("gh-state");
    fs::create_dir_all(&state_dir).unwrap();
    let captured_body = state_dir.join("body.md");
    let bin_dir = ws.join("stub-bin");
    let script = format!(
        r#"
case "$1 $2" in
  "auth status"|"repo view")
    if [ "$1 $2" = "repo view" ]; then
      echo '{{"nameWithOwner":"acme/widget","defaultBranchRef":{{"name":"main"}}}}'
    fi
    exit 0
    ;;
  "pr list")
    echo '[]'
    exit 0
    ;;
  "pr create")
    while [ $# -gt 0 ]; do
      case "$1" in
        --body-file)
          cp "$2" "{body}"
          shift 2
          ;;
        *)
          shift
          ;;
      esac
    done
    echo "https://github.com/acme/widget/pull/99"
    exit 0
    ;;
  "pr view")
    echo '{{"number":99,"url":"https://github.com/acme/widget/pull/99"}}'
    exit 0
    ;;
  *)
    exit 1
    ;;
esac
"#,
        body = captured_body.display(),
    );
    let gh_path = write_stub_gh(&bin_dir, &script);

    let outcome = open_pull_request_with_gh(&store, &ws, &run_id, &sys_actor(), &gh_path).unwrap();
    assert!(matches!(outcome, PrOutcome::Created(ref pr) if pr.number == 99));

    let body_text = fs::read_to_string(&captured_body).unwrap();
    assert!(
        body_text.contains("Verdict: approve · Rounds: 2 · Findings fixed: 1 · Still open: 0"),
        "expected review counts from artifacts/ subdir in:\n{body_text}"
    );

    let _ = fs::remove_dir_all(&ws);
    let _ = fs::remove_dir_all(&bare);
}
