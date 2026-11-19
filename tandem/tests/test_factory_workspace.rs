use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use tandem::factory::broker::git::{AllowedGitSubcommand, GitError, run_git};
use tandem::factory::model::{Actor, ActorKind, RunState};
use tandem::factory::run::get_run;
use tandem::factory::store::FactoryStore;
use tandem::factory::workspace::{
    TargetResolution, WorkspaceError, cleanup_worktree, prepare_worktree, resolve_target,
    slugify_branch_part,
};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-ws-{}-{}-{}",
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

fn init_git_repo(dir: &Path, branch: &str) {
    fs::create_dir_all(dir).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-b", branch])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    fs::write(dir.join("README.md"), "# Test\n").unwrap();
    assert!(
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success()
    );
}

fn setup_queued_run(ws: &Path) -> (FactoryStore, String, String) {
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();
    let mut settings = store.settings().unwrap();
    settings.approval_grace_secs = 0;
    tandem::factory::settings::save(&store, &settings).unwrap();
    let user = Actor {
        kind: ActorKind::User,
        id: "alice".into(),
    };
    let orch = Actor {
        kind: ActorKind::Orchestrator,
        id: "orch".into(),
    };
    let idea = store
        .create_idea("Export team CSV!", "Details", "alice", &user)
        .unwrap();
    store
        .execute(
            &idea.id,
            tandem::factory::machine::Command::StartDiscovery,
            &orch,
        )
        .unwrap();
    let feas = ws.join("f.md");
    fs::write(&feas, "# Feas\n").unwrap();
    let feas_ref = tandem::factory::artifacts::put(&store, &idea.id, "feasibility", &feas).unwrap();
    store
        .execute(
            &idea.id,
            tandem::factory::machine::Command::StoreArtifact {
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
        .execute(
            &idea.id,
            tandem::factory::machine::Command::SelectDirection,
            &orch,
        )
        .unwrap();
    let plan = ws.join("p.md");
    fs::write(
        &plan,
        "# Plan\n\n## Goal\nCSV\n\n## Acceptance Criteria\n- Works\n\n## Files\n- a\n\n## Tests\n- test\n",
    )
    .unwrap();
    let plan_ref = tandem::factory::artifacts::put(&store, &idea.id, "plan", &plan).unwrap();
    store
        .execute(
            &idea.id,
            tandem::factory::machine::Command::StoreArtifact {
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
    let caller = tandem::factory::approval::Caller::cli(true, &|_| None, &|_| None);
    let ready = tandem::factory::approval::mark_ready_with_workflow(
        &store, ws, &idea.id, &idea.id, "alice", None, false, &caller,
    )
    .unwrap();
    let run_id = ready.run.unwrap().id;
    (store, idea.id, run_id)
}

#[test]
fn git_wrapper_allows_only_allowlisted_subcommands() {
    let dir = temp_dir("git");
    init_git_repo(&dir, "trunk");

    let out = run_git(
        AllowedGitSubcommand::RevParse,
        &["--abbrev-ref", "HEAD"],
        &dir,
        Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(out.trim(), "trunk");

    assert!(AllowedGitSubcommand::parse("push").is_some());
    assert!(AllowedGitSubcommand::parse("commit").is_some());
    assert!(AllowedGitSubcommand::parse("checkout").is_none());
    assert!(AllowedGitSubcommand::parse("merge").is_none());
    let _ = GitError::Timeout;

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn target_resolution_handles_workspace_single_and_ambiguous_codebases() {
    let ws = temp_dir("target");
    init_git_repo(&ws, "trunk");

    // 0 linked codebases -> workspace
    assert!(matches!(
        resolve_target(&ws),
        TargetResolution::Resolved { ref name, .. } if name == "workspace"
    ));

    // 1 linked git codebase -> that codebase
    let repo_a = temp_dir("repo-a");
    init_git_repo(&repo_a, "main");
    fs::create_dir_all(ws.join("codebase-symlinks")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&repo_a, ws.join("codebase-symlinks/repo-a")).unwrap();

    assert!(matches!(
        resolve_target(&ws),
        TargetResolution::Resolved { ref name, .. } if name == "repo-a"
    ));

    // 2 linked git codebases -> ambiguous
    let repo_b = temp_dir("repo-b");
    init_git_repo(&repo_b, "main");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&repo_b, ws.join("codebase-symlinks/repo-b")).unwrap();

    assert!(matches!(
        resolve_target(&ws),
        TargetResolution::Ambiguous { ref choices } if choices.len() == 2
    ));

    let _ = fs::remove_dir_all(&ws);
    let _ = fs::remove_dir_all(&repo_a);
    let _ = fs::remove_dir_all(&repo_b);
}

#[test]
fn prepare_creates_idempotent_worktree_outside_artifact_channel_and_cleanup_respects_dirty_state() {
    let ws = temp_dir("prep");
    init_git_repo(&ws, "trunk");
    let (store, idea_id, run_id) = setup_queued_run(&ws);

    assert_eq!(slugify_branch_part("Export team CSV!"), "export-team-csv");

    let rw1 = prepare_worktree(&store, &ws, &run_id).unwrap();
    assert!(rw1.worktree_path.exists());
    assert!(rw1.branch.starts_with(&format!("feat/{idea_id}-")));
    assert_ne!(rw1.branch, "trunk");
    assert_ne!(rw1.branch, "main");
    assert!(!rw1.artifact_dir.starts_with(&rw1.worktree_path));

    // Current checkout in ws is untouched
    let current = run_git(
        AllowedGitSubcommand::RevParse,
        &["--abbrev-ref", "HEAD"],
        &ws,
        Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(current.trim(), "trunk");

    // Idempotent second call returns same worktree
    let rw2 = prepare_worktree(&store, &ws, &run_id).unwrap();
    assert_eq!(rw1, rw2);

    // Run stores target, branch, base_commit
    let run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(run.target.as_deref(), Some("workspace"));
    assert_eq!(run.branch.as_deref(), Some(rw1.branch.as_str()));
    assert!(run.base_commit.is_some());

    // Dirty worktree refuses cleanup
    fs::write(rw1.worktree_path.join("dirty.txt"), "uncommitted").unwrap();
    let user = Actor {
        kind: ActorKind::User,
        id: "alice".into(),
    };
    // Withdraw run first so state allows cleanup
    let withdrawn = tandem::factory::run::withdraw_run(&store, &run_id, &user).unwrap();
    assert_eq!(withdrawn.state, RunState::Withdrawn);

    let caller = tandem::factory::approval::Caller::cli(true, &|_| None, &|_| None);
    let err = cleanup_worktree(&store, &ws, &run_id, &user, &caller).unwrap_err();
    assert!(matches!(err, WorkspaceError::DirtyWorktree(_)));

    // Clean dirty file -> cleanup succeeds and keeps the branch
    fs::remove_file(rw1.worktree_path.join("dirty.txt")).unwrap();
    cleanup_worktree(&store, &ws, &run_id, &user, &caller).unwrap();
    assert!(!rw1.worktree_path.exists());

    let branches = run_git(
        AllowedGitSubcommand::Branch,
        &["--list", &rw1.branch],
        &ws,
        Duration::from_secs(5),
    )
    .unwrap();
    assert!(
        branches.contains(&rw1.branch),
        "branch must never be deleted"
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn set_run_target_rejects_agent_caller_and_cleanup_rejects_escape() {
    let ws = temp_dir("sec-wt");
    init_git_repo(&ws, "trunk");
    let (store, _idea_id, run_id) = setup_queued_run(&ws);

    let agent_env = |k: &str| {
        if k == "TANDEM_ROLE" {
            Some("orchestrator".to_string())
        } else {
            None
        }
    };
    let agent_caller = tandem::factory::approval::Caller::cli(true, &agent_env, &|_| None);
    let user_caller = tandem::factory::approval::Caller::cli(true, &|_| None, &|_| None);
    let err_target = tandem::factory::workspace::set_run_target(
        &store,
        &ws,
        &run_id,
        "workspace",
        &agent_caller,
    )
    .unwrap_err();
    assert!(matches!(err_target, WorkspaceError::Approval(_)));

    let user = Actor {
        kind: ActorKind::User,
        id: "alice".into(),
    };
    tandem::factory::workspace::set_run_target(&store, &ws, &run_id, "workspace", &user_caller)
        .unwrap();

    // Tamper run.json worktree_path to point outside workspace/worktrees
    let outside = temp_dir("outside-wt");
    let run_json = store.root.join("runs").join(&run_id).join("run.json");
    let mut run = get_run(&store, &run_id).unwrap().unwrap();
    run.worktree_path = Some(outside.to_string_lossy().to_string());
    fs::write(&run_json, serde_json::to_vec_pretty(&run).unwrap()).unwrap();
    let _ = tandem::factory::run::withdraw_run(&store, &run_id, &user).unwrap();
    // Re-write tampered worktree_path after withdraw
    let mut withdrawn = get_run(&store, &run_id).unwrap().unwrap();
    withdrawn.worktree_path = Some(outside.to_string_lossy().to_string());
    fs::write(&run_json, serde_json::to_vec_pretty(&withdrawn).unwrap()).unwrap();

    let err_cleanup = cleanup_worktree(&store, &ws, &run_id, &user, &user_caller).unwrap_err();
    assert!(matches!(err_cleanup, WorkspaceError::Factory(_)));

    let _ = fs::remove_dir_all(&ws);
    let _ = fs::remove_dir_all(&outside);
}
