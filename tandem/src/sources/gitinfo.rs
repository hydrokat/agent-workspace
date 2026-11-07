use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

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

#[derive(Clone)]
struct CacheEntry {
    signature: Vec<(PathBuf, Option<SystemTime>)>,
    info: GitInfo,
}

static CACHE: Mutex<Option<HashMap<(PathBuf, String), CacheEntry>>> = Mutex::new(None);

pub fn clear_git_info_cache() {
    if let Ok(mut lock) = CACHE.lock()
        && let Some(map) = lock.as_mut()
    {
        map.clear();
    }
}

fn find_git_dir(directory: &Path) -> Option<(PathBuf, PathBuf)> {
    for ancestor in directory.ancestors() {
        let git_path = ancestor.join(".git");
        if git_path.is_file() {
            let content = std::fs::read_to_string(&git_path).ok()?;
            let target = content.strip_prefix("gitdir:")?.trim();
            let resolved = if Path::new(target).is_absolute() {
                PathBuf::from(target)
            } else {
                ancestor.join(target)
            };
            return Some((ancestor.to_path_buf(), resolved));
        } else if git_path.is_dir() {
            return Some((ancestor.to_path_buf(), git_path));
        }
    }
    None
}

fn git_signature(directory: &Path) -> Option<Vec<(PathBuf, Option<SystemTime>)>> {
    let (repo_root, git_dir) = find_git_dir(directory)?;

    let check_files = [
        git_dir.join("HEAD"),
        git_dir.join("index"),
        git_dir.join("refs").join("heads"),
        git_dir.join("packed-refs"),
        git_dir.join("worktrees"),
    ];

    let mut sig = Vec::with_capacity(check_files.len() + 1);
    let dot_git_mtime = std::fs::metadata(repo_root.join(".git"))
        .ok()
        .and_then(|m| m.modified().ok());
    sig.push((repo_root.join(".git"), dot_git_mtime));
    for f in &check_files {
        let mtime = std::fs::metadata(f).ok().and_then(|m| m.modified().ok());
        sig.push((f.clone(), mtime));
    }
    Some(sig)
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

    let sig = git_signature(directory);
    let key_dir = directory
        .canonicalize()
        .unwrap_or_else(|_| directory.to_path_buf());
    let key = (key_dir, git_binary.to_string());

    if let Some(ref current_sig) = sig
        && let Ok(mut lock) = CACHE.lock()
    {
        let map = lock.get_or_insert_with(HashMap::new);
        if let Some(entry) = map.get(&key)
            && &entry.signature == current_sig
        {
            return Some(entry.info.clone());
        }
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

    let info = GitInfo {
        current: current.trim().to_string(),
        branches,
        worktrees,
    };

    if let Some(current_sig) = sig
        && let Ok(mut lock) = CACHE.lock()
    {
        let map = lock.get_or_insert_with(HashMap::new);
        map.insert(
            key,
            CacheEntry {
                signature: current_sig,
                info: info.clone(),
            },
        );
    }

    Some(info)
}
