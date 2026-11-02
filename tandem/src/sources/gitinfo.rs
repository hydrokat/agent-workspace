use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Worktree {
    pub path: String,
    pub branch: String,
    #[serde(default)]
    pub main: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitInfo {
    pub current: String,
    pub branches: Vec<String>,
    pub worktrees: Vec<Worktree>,
}

fn run_git_binary(
    binary: &str,
    directory: &Path,
    timeout: Duration,
    args: &[&str],
) -> Option<String> {
    let mut child = Command::new(binary)
        .args(args)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    let child_lock = Arc::new(Mutex::new(child));
    let child_for_thread = Arc::clone(&child_lock);

    std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        let status = child_for_thread
            .lock()
            .map(|mut c| c.wait())
            .unwrap_or(Err(std::io::Error::other("poisoned")));
        let _ = tx.send((status, out));
    });

    match rx.recv_timeout(timeout) {
        Ok((Ok(status), out)) if status.success() => String::from_utf8(out).ok(),
        _ => {
            if let Ok(mut c) = child_lock.lock() {
                let _ = c.kill();
            }
            None
        }
    }
}

pub fn git_info(directory: &Path, timeout: Duration) -> Option<GitInfo> {
    git_info_with_git(directory, timeout, "git")
}

pub fn git_info_with_git(directory: &Path, timeout: Duration, git_binary: &str) -> Option<GitInfo> {
    if !directory.is_dir() {
        return None;
    }

    let current = run_git_binary(
        git_binary,
        directory,
        timeout,
        &["branch", "--show-current"],
    )?;
    let branches_raw = run_git_binary(
        git_binary,
        directory,
        timeout,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )?;
    let trees_raw = run_git_binary(
        git_binary,
        directory,
        timeout,
        &["worktree", "list", "--porcelain"],
    )?;

    let mut worktrees = Vec::new();

    for line in trees_raw.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            let is_main = worktrees.is_empty();
            worktrees.push(Worktree {
                path: path.to_string(),
                branch: String::new(),
                main: is_main,
            });
        } else if let Some(branch_ref) = line.strip_prefix("branch ")
            && let Some(last) = worktrees.last_mut()
        {
            let branch_name = branch_ref.strip_prefix("refs/heads/").unwrap_or(branch_ref);
            last.branch = branch_name.to_string();
        }
    }

    let branches: Vec<String> = branches_raw
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    Some(GitInfo {
        current: current.trim().to_string(),
        branches,
        worktrees,
    })
}
