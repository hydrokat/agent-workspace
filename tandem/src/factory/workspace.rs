use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::coordination::filelock::exclusive_lock;
use crate::factory::FactoryError;
use crate::factory::approval::{ApprovalError, ensure_user_caller};
use crate::factory::broker::git::{AllowedGitSubcommand, GitError, run_git};
use crate::factory::data_policy::{
    ExcludedPathEntry, is_worktree_excluded, load_exclusions, normalize_rel_path, save_exclusions,
};
use crate::factory::events::{
    Event, append_locked, redact_event, reject_if_symlink, set_private_file_mode,
};
use crate::factory::ids::is_valid_run_id;
use crate::factory::machine::{self, EventData};
use crate::factory::model::{Actor, ActorKind, FactorySettings, Run, RunState};
use crate::factory::redact::RedactContext;
use crate::factory::run::{RunError, get_run};
use crate::factory::settings::load as load_settings;
use crate::factory::store::{FactoryStore, create_private_dir, current_time, write_atomic};
use crate::sources::codebases::list_codebases;

const GIT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetResolution {
    Resolved { name: String, path: PathBuf },
    Ambiguous { choices: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunWorkspace {
    pub target: String,
    pub target_path: PathBuf,
    pub worktree_path: PathBuf,
    pub artifact_dir: PathBuf,
    pub branch: String,
    pub base_commit: String,
    pub excluded_paths: Vec<ExcludedPathEntry>,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("choose a target repository: {}", .0.join(", "))]
    AmbiguousTarget(Vec<String>),
    #[error("unknown target repository: {0}")]
    UnknownTarget(String),
    #[error("cannot cleanup worktree with uncommitted changes in {0}")]
    DirtyWorktree(String),
    #[error("run {0} is in state {1:?} which does not allow cleanup")]
    InvalidRunState(String, RunState),
    #[error(transparent)]
    Approval(#[from] ApprovalError),
    #[error(transparent)]
    Run(#[from] RunError),
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Factory(#[from] FactoryError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn resolve_target(workspace: &Path) -> TargetResolution {
    let git_codebases: Vec<_> = list_codebases(workspace)
        .into_iter()
        .filter(|c| c.exists && c.is_git)
        .collect();

    match git_codebases.len() {
        0 => TargetResolution::Resolved {
            name: "workspace".to_string(),
            path: workspace.to_path_buf(),
        },
        1 => {
            let cb = &git_codebases[0];
            TargetResolution::Resolved {
                name: cb.name.clone(),
                path: cb.path.clone(),
            }
        }
        _ => TargetResolution::Ambiguous {
            choices: git_codebases.into_iter().map(|c| c.name).collect(),
        },
    }
}

pub fn target_path_for_name(
    workspace: &Path,
    target_name: &str,
) -> Result<PathBuf, WorkspaceError> {
    if target_name == "workspace" {
        return Ok(workspace.to_path_buf());
    }
    list_codebases(workspace)
        .into_iter()
        .find(|c| c.exists && c.is_git && c.name == target_name)
        .map(|c| c.path)
        .ok_or_else(|| WorkspaceError::UnknownTarget(target_name.to_string()))
}

pub fn slugify_branch_part(title: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= 40 {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "work".to_string()
    } else {
        out
    }
}

fn is_protected_branch(name: &str) -> bool {
    matches!(name, "main" | "master" | "trunk")
}

fn find_primary_branch(repo: &Path) -> Result<String, WorkspaceError> {
    for candidate in ["main", "trunk", "master"] {
        if run_git(
            AllowedGitSubcommand::RevParse,
            &["--verify", candidate],
            repo,
            GIT_TIMEOUT,
        )
        .is_ok()
        {
            return Ok(candidate.to_string());
        }
    }
    let head = run_git(
        AllowedGitSubcommand::SymbolicRef,
        &["--short", "HEAD"],
        repo,
        GIT_TIMEOUT,
    )?;
    Ok(head.trim().to_string())
}

fn branch_exists(repo: &Path, branch: &str) -> bool {
    run_git(
        AllowedGitSubcommand::RevParse,
        &["--verify", &format!("refs/heads/{branch}")],
        repo,
        GIT_TIMEOUT,
    )
    .is_ok()
}

pub fn set_run_target(
    store: &FactoryStore,
    workspace: &Path,
    run_id: &str,
    target_name: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Run, WorkspaceError> {
    ensure_user_caller(env)?;
    if !is_valid_run_id(run_id) {
        return Err(FactoryError::InvalidInput(format!("invalid run id: {run_id}")).into());
    }
    let _ = target_path_for_name(workspace, target_name)?;

    store.ensure()?;
    let lock_path = store.lock_path();
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    set_private_file_mode(&lock_path)?;
    let _guard = exclusive_lock(&lock_file)?;

    let run = get_run(store, run_id)?.ok_or_else(|| RunError::NotFound(format!("run {run_id}")))?;
    let ev_data = EventData::RunTargetSelected {
        run: run_id.to_string(),
        target: target_name.to_string(),
    };
    let actor = Actor {
        kind: ActorKind::User,
        id: "user".to_string(),
    };
    let ev = Event::from_data(
        current_time(),
        actor,
        run.idea.clone(),
        Some(run_id.to_string()),
        &ev_data,
    )?;
    let appended = append_locked(
        &store.events_path(),
        &[redact_event(ev, &RedactContext::from_env())],
    )?;
    let updated = machine::apply_run(Some(run), &ev_data, &appended[0].idea, appended[0].ts)
        .ok_or_else(|| FactoryError::InvalidInput("failed to apply target".to_string()))?;
    let bytes = serde_json::to_vec_pretty(&updated).map_err(FactoryError::from)?;
    write_atomic(&store.run_json_path(run_id), &bytes)?;
    Ok(updated)
}

pub fn prepare_worktree(
    store: &FactoryStore,
    workspace: &Path,
    run_id: &str,
) -> Result<RunWorkspace, WorkspaceError> {
    if !is_valid_run_id(run_id) {
        return Err(FactoryError::InvalidInput(format!("invalid run id: {run_id}")).into());
    }
    store.ensure()?;
    let run = get_run(store, run_id)?.ok_or_else(|| RunError::NotFound(format!("run {run_id}")))?;

    let artifact_dir = store.run_dir(run_id);
    create_private_dir(&artifact_dir)?;

    let (target, target_path) = if let Some(ref explicit) = run.target {
        (explicit.clone(), target_path_for_name(workspace, explicit)?)
    } else {
        match resolve_target(workspace) {
            TargetResolution::Resolved { name, path } => (name, path),
            TargetResolution::Ambiguous { choices } => {
                return Err(WorkspaceError::AmbiguousTarget(choices));
            }
        }
    };

    let wt_root = workspace.join("worktrees");
    fs::create_dir_all(&wt_root)?;
    reject_if_symlink(&wt_root)?;
    let wt_path = wt_root.join(format!("factory-{run_id}"));

    if let (Some(branch), Some(base_commit)) = (&run.branch, &run.base_commit)
        && wt_path.exists()
    {
        return Ok(RunWorkspace {
            target,
            target_path,
            worktree_path: wt_path,
            artifact_dir,
            branch: branch.clone(),
            base_commit: base_commit.clone(),
            excluded_paths: load_exclusions(store, run_id),
        });
    }

    let settings = load_settings(store)?;

    if let Ok(remotes) = run_git(
        AllowedGitSubcommand::Branch,
        &["-r"],
        &target_path,
        GIT_TIMEOUT,
    ) && !remotes.trim().is_empty()
    {
        let _ = run_git(AllowedGitSubcommand::Fetch, &[], &target_path, GIT_TIMEOUT);
    }

    let primary = find_primary_branch(&target_path)?;
    let base_commit = run_git(
        AllowedGitSubcommand::RevParse,
        &[&primary],
        &target_path,
        GIT_TIMEOUT,
    )?
    .trim()
    .to_string();

    let idea_title = store
        .get_idea(&run.idea)?
        .map(|i| i.title)
        .unwrap_or_else(|| "work".to_string());
    let base_branch = format!("feat/{}-{}", run.idea, slugify_branch_part(&idea_title));
    let mut branch = base_branch.clone();
    let mut suffix = 2u32;
    while branch_exists(&target_path, &branch) || is_protected_branch(&branch) {
        branch = format!("{base_branch}-{suffix}");
        suffix += 1;
    }

    let wt_str = wt_path.to_string_lossy().to_string();
    run_git(
        AllowedGitSubcommand::Worktree,
        &["add", "-b", &branch, &wt_str, &primary],
        &target_path,
        GIT_TIMEOUT,
    )?;

    let excluded_paths = apply_sparse_checkout_exclusions(&wt_path, &settings)?;
    save_exclusions(store, run_id, &excluded_paths)?;

    let ev_data = EventData::WorkspacePrepared {
        run: run_id.to_string(),
        target: target.clone(),
        branch: branch.clone(),
        base_commit: base_commit.clone(),
        worktree_path: wt_str,
    };
    let actor = Actor {
        kind: ActorKind::System,
        id: "engine".to_string(),
    };
    let ev = Event::from_data(
        current_time(),
        actor,
        run.idea.clone(),
        Some(run_id.to_string()),
        &ev_data,
    )?;
    let appended = append_locked(
        &store.events_path(),
        &[redact_event(ev, &RedactContext::from_env())],
    )?;
    let updated = machine::apply_run(Some(run), &ev_data, &appended[0].idea, appended[0].ts)
        .ok_or_else(|| FactoryError::InvalidInput("failed to apply workspace".to_string()))?;
    let bytes = serde_json::to_vec_pretty(&updated).map_err(FactoryError::from)?;
    write_atomic(&store.run_json_path(run_id), &bytes)?;

    Ok(RunWorkspace {
        target,
        target_path,
        worktree_path: wt_path,
        artifact_dir,
        branch,
        base_commit,
        excluded_paths,
    })
}

fn collect_worktree_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<String>,
) -> Result<(), WorkspaceError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let path = entry.path();
        if path.is_symlink() {
            continue;
        }
        if path.is_dir() {
            collect_worktree_files(root, &path, out)?;
        } else if path.is_file()
            && let Ok(rel) = path.strip_prefix(root)
        {
            out.push(normalize_rel_path(&rel.to_string_lossy()));
        }
    }
    Ok(())
}

fn apply_sparse_checkout_exclusions(
    wt_path: &Path,
    settings: &FactorySettings,
) -> Result<Vec<ExcludedPathEntry>, WorkspaceError> {
    let mut files = Vec::new();
    collect_worktree_files(wt_path, wt_path, &mut files)?;
    files.sort();

    let mut excluded = Vec::new();
    for rel in files {
        if let Some(rule) = is_worktree_excluded(&rel, settings) {
            excluded.push(ExcludedPathEntry { path: rel, rule });
        }
    }

    if !excluded.is_empty() {
        run_git(
            AllowedGitSubcommand::SparseCheckout,
            &["init", "--no-cone"],
            wt_path,
            GIT_TIMEOUT,
        )?;
        let mut patterns: Vec<String> = Vec::with_capacity(excluded.len() + 3);
        patterns.push("set".to_string());
        patterns.push("--no-cone".to_string());
        patterns.push("/*".to_string());
        for item in &excluded {
            patterns.push(format!("!/{}", item.path));
        }
        let args: Vec<&str> = patterns.iter().map(String::as_str).collect();
        run_git(
            AllowedGitSubcommand::SparseCheckout,
            &args,
            wt_path,
            GIT_TIMEOUT,
        )?;
    }

    Ok(excluded)
}

pub fn cleanup_worktree(
    store: &FactoryStore,
    workspace: &Path,
    run_id: &str,
    _actor: &Actor,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<(), WorkspaceError> {
    ensure_user_caller(env)?;
    if !is_valid_run_id(run_id) {
        return Err(FactoryError::InvalidInput(format!("invalid run id: {run_id}")).into());
    }
    let run = get_run(store, run_id)?.ok_or_else(|| RunError::NotFound(format!("run {run_id}")))?;
    if !matches!(
        run.state,
        RunState::Delivered | RunState::Cancelled | RunState::Withdrawn
    ) {
        return Err(WorkspaceError::InvalidRunState(
            run_id.to_string(),
            run.state,
        ));
    }

    let wt_path = run
        .worktree_path
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            workspace
                .join("worktrees")
                .join(format!("factory-{run_id}"))
        });
    reject_if_symlink(&wt_path)?;
    if !wt_path.exists() {
        return Ok(());
    }
    let wt_root = workspace.join("worktrees");
    reject_if_symlink(&wt_root)?;
    if !wt_root.exists() {
        return Err(FactoryError::InvalidInput(format!(
            "worktree path {} escapes {}",
            wt_path.display(),
            wt_root.display()
        ))
        .into());
    }
    let canon_root = wt_root.canonicalize()?;
    let canon_wt = wt_path.canonicalize()?;
    if !canon_wt.starts_with(&canon_root) || canon_wt == canon_root {
        return Err(FactoryError::InvalidInput(format!(
            "worktree path {} escapes {}",
            wt_path.display(),
            wt_root.display()
        ))
        .into());
    }

    let status = run_git(
        AllowedGitSubcommand::Status,
        &["--porcelain"],
        &wt_path,
        GIT_TIMEOUT,
    )?;
    if !status.trim().is_empty() {
        return Err(WorkspaceError::DirtyWorktree(wt_path.display().to_string()));
    }

    let target_name = run.target.as_deref().unwrap_or("workspace");
    let target_path = target_path_for_name(workspace, target_name)?;
    let wt_str = wt_path.to_string_lossy().to_string();
    run_git(
        AllowedGitSubcommand::Worktree,
        &["remove", &wt_str],
        &target_path,
        GIT_TIMEOUT,
    )?;
    Ok(())
}
