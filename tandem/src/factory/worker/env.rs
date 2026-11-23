use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const COMMON_ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "HOME",
    "LANG",
    "LC_ALL",
    "TERM",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "XDG_CACHE_HOME",
];

#[derive(Debug, thiserror::Error)]
#[error("invalid worktree path {}: {reason}", path.display())]
pub struct WorktreeError {
    pub path: PathBuf,
    pub reason: String,
}

pub fn validate_worktree(workspace_root: &Path, cwd: &Path) -> Result<PathBuf, WorktreeError> {
    let worktrees_dir = workspace_root.join("worktrees");
    let wt_meta = fs::symlink_metadata(&worktrees_dir).map_err(|e| WorktreeError {
        path: worktrees_dir.clone(),
        reason: e.to_string(),
    })?;
    if wt_meta.file_type().is_symlink() || !wt_meta.is_dir() {
        return Err(WorktreeError {
            path: worktrees_dir,
            reason: "worktrees root must be a real directory".to_string(),
        });
    }

    let cwd_meta = fs::symlink_metadata(cwd).map_err(|e| WorktreeError {
        path: cwd.to_path_buf(),
        reason: e.to_string(),
    })?;
    if cwd_meta.file_type().is_symlink() || !cwd_meta.is_dir() {
        return Err(WorktreeError {
            path: cwd.to_path_buf(),
            reason: "working directory must be a real directory".to_string(),
        });
    }

    let canon_worktrees = worktrees_dir.canonicalize().map_err(|e| WorktreeError {
        path: worktrees_dir,
        reason: e.to_string(),
    })?;
    let canon_cwd = cwd.canonicalize().map_err(|e| WorktreeError {
        path: cwd.to_path_buf(),
        reason: e.to_string(),
    })?;

    if !canon_cwd.starts_with(&canon_worktrees) || canon_cwd == canon_worktrees {
        return Err(WorktreeError {
            path: cwd.to_path_buf(),
            reason: format!(
                "must be strictly inside {}",
                canon_worktrees.to_string_lossy()
            ),
        });
    }

    Ok(canon_cwd)
}

fn provider_config_vars(provider: &str) -> &'static [&'static str] {
    match provider {
        "claude" => &["CLAUDE_CONFIG_DIR"],
        "agy" => &["AGY_CONFIG_DIR"],
        "opencode" => &[
            "OPENCODE_CONFIG",
            "OPENCODE_CONFIG_DIR",
            "OPENCODE_CONFIG_CONTENT",
        ],
        _ => &[],
    }
}

fn is_allowed_factory_var(key: &str) -> bool {
    if !key.starts_with("TANDEM_") {
        return false;
    }
    let upper = key.to_ascii_uppercase();
    !["TOKEN", "KEY", "SECRET", "PASSWORD", "AUTH"]
        .iter()
        .any(|forbidden| upper.contains(forbidden))
}

pub fn build_env(
    provider: &str,
    parent_env: &BTreeMap<String, String>,
    factory_env: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    let provider_vars = provider_config_vars(provider);

    for &key in COMMON_ENV_ALLOWLIST.iter().chain(provider_vars.iter()) {
        if let Some(val) = parent_env.get(key) {
            env.insert(key.to_string(), val.clone());
        }
    }

    for (key, val) in factory_env {
        if is_allowed_factory_var(key) {
            env.insert(key.clone(), val.clone());
        }
    }

    env
}
