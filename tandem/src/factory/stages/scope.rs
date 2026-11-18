use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::Duration;

use super::ContractFailure;
use super::plan::{Slice, SlicesManifest};
use crate::factory::broker::git::{AllowedGitSubcommand, run_git};
use crate::factory::events::reject_if_symlink;

const GIT_TIMEOUT: Duration = Duration::from_secs(15);

const PROTECTED_PREFIXES: &[&str] = &[
    ".agent-workspace/",
    "workflows/",
    ".github/workflows/",
    ".git/hooks/",
    ".githooks/",
];

const PROTECTED_EXACT: &[&str] = &[
    ".agent-workspace",
    "workflows",
    ".github/workflows",
    ".git/hooks",
    ".githooks",
    "checks.json",
];

const ALLOWED_LOCKFILES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
    "uv.lock",
    "poetry.lock",
    "Gemfile.lock",
    "go.sum",
    "composer.lock",
    "flake.lock",
];

#[derive(Debug, Clone)]
pub struct ScopeCheckInput<'a> {
    pub worktree_path: &'a Path,
    pub artifact_dir: &'a Path,
    pub has_repo_write: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeCheckResult {
    pub changed_files: Vec<String>,
}

fn normalize_rel_path(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches('"');
    let replaced = trimmed.replace('\\', "/");
    replaced.trim_start_matches("./").to_string()
}

pub fn is_protected_path(path: &str) -> bool {
    let norm = normalize_rel_path(path);
    if PROTECTED_EXACT.contains(&norm.as_str()) {
        return true;
    }
    if PROTECTED_PREFIXES.iter().any(|p| norm.starts_with(p)) {
        return true;
    }
    norm.ends_with("/checks.json")
}

pub fn is_allowed_lockfile(path: &str) -> bool {
    let norm = normalize_rel_path(path);
    Path::new(&norm)
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| ALLOWED_LOCKFILES.contains(&name))
}

pub fn path_in_slices(path: &str, slices: &[Slice]) -> bool {
    let norm_path = normalize_rel_path(path);
    for slice in slices {
        for raw in &slice.paths {
            let norm_slice = normalize_rel_path(raw);
            let dir_prefix = norm_slice.trim_end_matches('/');
            if dir_prefix.is_empty() {
                continue;
            }
            if norm_path == dir_prefix || norm_path.starts_with(&format!("{dir_prefix}/")) {
                return true;
            }
        }
    }
    false
}

pub fn list_changed_files(worktree_path: &Path) -> Result<Vec<String>, ContractFailure> {
    let status = run_git(
        AllowedGitSubcommand::Status,
        &["--porcelain", "-uall"],
        worktree_path,
        GIT_TIMEOUT,
    )
    .map_err(|e| ContractFailure::new("scope_git_status", e.to_string()))?;

    let mut files = BTreeSet::new();
    for line in status.lines() {
        if line.len() <= 3 {
            continue;
        }
        let body = &line[3..];
        if let Some((old, new)) = body.split_once(" -> ") {
            let old_norm = normalize_rel_path(old);
            let new_norm = normalize_rel_path(new);
            if !old_norm.is_empty() {
                files.insert(old_norm);
            }
            if !new_norm.is_empty() {
                files.insert(new_norm);
            }
        } else {
            let norm = normalize_rel_path(body);
            if !norm.is_empty() {
                files.insert(norm);
            }
        }
    }

    let diff_out = run_git(
        AllowedGitSubcommand::Diff,
        &["--name-only", "HEAD"],
        worktree_path,
        GIT_TIMEOUT,
    )
    .or_else(|_| {
        run_git(
            AllowedGitSubcommand::Diff,
            &["--name-only"],
            worktree_path,
            GIT_TIMEOUT,
        )
    })
    .map_err(|e| ContractFailure::new("scope_git_diff", e.to_string()))?;

    for line in diff_out.lines() {
        let norm = normalize_rel_path(line);
        if !norm.is_empty() {
            files.insert(norm);
        }
    }

    Ok(files.into_iter().collect())
}

pub fn list_deleted_files(worktree_path: &Path) -> Result<BTreeSet<String>, ContractFailure> {
    let status = run_git(
        AllowedGitSubcommand::Status,
        &["--porcelain", "-uall"],
        worktree_path,
        GIT_TIMEOUT,
    )
    .map_err(|e| ContractFailure::new("scope_git_status", e.to_string()))?;

    let mut deleted = BTreeSet::new();
    for line in status.lines() {
        if line.len() <= 3 {
            continue;
        }
        let xy = &line[..2];
        if xy.contains('D') {
            let norm = normalize_rel_path(&line[3..]);
            if !norm.is_empty() {
                deleted.insert(norm);
            }
        }
    }
    Ok(deleted)
}

fn load_slices(artifact_dir: &Path) -> Result<SlicesManifest, ContractFailure> {
    let slices_path = artifact_dir.join("slices.json");
    reject_if_symlink(&slices_path)
        .map_err(|e| ContractFailure::new("scope_slices", e.to_string()))?;
    let raw = fs::read_to_string(&slices_path).map_err(|e| {
        ContractFailure::new(
            "scope_slices",
            format!("failed to read {}: {e}", slices_path.display()),
        )
    })?;
    serde_json::from_str(&raw)
        .map_err(|e| ContractFailure::new("scope_slices", format!("invalid slices.json: {e}")))
}

pub fn verify_scope(input: &ScopeCheckInput<'_>) -> Result<ScopeCheckResult, ContractFailure> {
    let changed_files = list_changed_files(input.worktree_path)?;

    if !input.has_repo_write {
        if !changed_files.is_empty() {
            return Err(ContractFailure::new(
                "scope_readonly",
                format!(
                    "read-only stage modified files: {}",
                    changed_files.join(", ")
                ),
            ));
        }
        return Ok(ScopeCheckResult { changed_files });
    }

    let protected: Vec<&str> = changed_files
        .iter()
        .map(String::as_str)
        .filter(|p| is_protected_path(p))
        .collect();
    if !protected.is_empty() {
        return Err(ContractFailure::new(
            "scope_protected_paths",
            format!("protected paths modified: {}", protected.join(", ")),
        ));
    }

    let manifest = load_slices(input.artifact_dir)?;
    let out_of_scope: Vec<&str> = changed_files
        .iter()
        .map(String::as_str)
        .filter(|p| !is_allowed_lockfile(p) && !path_in_slices(p, &manifest.slices))
        .collect();
    if !out_of_scope.is_empty() {
        return Err(ContractFailure::new(
            "scope_out_of_slice",
            format!("out-of-scope files modified: {}", out_of_scope.join(", ")),
        ));
    }

    Ok(ScopeCheckResult { changed_files })
}
