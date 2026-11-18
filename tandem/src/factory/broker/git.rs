use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use super::{ChildWaitOutcome, MAX_BROKER_OUTPUT_BYTES, wait_child_with_timeout_and_drain};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowedGitSubcommand {
    RevParse,
    RevList,
    SymbolicRef,
    Status,
    Branch,
    Worktree,
    Diff,
    Fetch,
    Remote,
    Add,
    Commit,
    Push,
}

impl AllowedGitSubcommand {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RevParse => "rev-parse",
            Self::RevList => "rev-list",
            Self::SymbolicRef => "symbolic-ref",
            Self::Status => "status",
            Self::Branch => "branch",
            Self::Worktree => "worktree",
            Self::Diff => "diff",
            Self::Fetch => "fetch",
            Self::Remote => "remote",
            Self::Add => "add",
            Self::Commit => "commit",
            Self::Push => "push",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "rev-parse" => Some(Self::RevParse),
            "rev-list" => Some(Self::RevList),
            "symbolic-ref" => Some(Self::SymbolicRef),
            "status" => Some(Self::Status),
            "branch" => Some(Self::Branch),
            "worktree" => Some(Self::Worktree),
            "diff" => Some(Self::Diff),
            "fetch" => Some(Self::Fetch),
            "remote" => Some(Self::Remote),
            "add" => Some(Self::Add),
            "commit" => Some(Self::Commit),
            "push" => Some(Self::Push),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git timed out")]
    Timeout,
    #[error("git {subcommand} refused: {reason}")]
    Refused { subcommand: String, reason: String },
    #[error("git {subcommand} failed: {stderr}")]
    Failed { subcommand: String, stderr: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn is_protected_branch_ref(branch: &str) -> bool {
    let trimmed = branch
        .trim()
        .strip_prefix("refs/heads/")
        .unwrap_or(branch.trim());
    matches!(trimmed, "main" | "master" | "trunk")
}

fn validate_push_args(args: &[&str]) -> Result<(), GitError> {
    if args.len() != 3 || args[0] != "-u" || args[1] != "origin" {
        return Err(GitError::Refused {
            subcommand: "push".to_string(),
            reason: "push only permits `-u origin <branch>`".to_string(),
        });
    }
    let branch = args[2];
    if branch.is_empty()
        || branch.starts_with('-')
        || branch.starts_with('+')
        || branch.contains(':')
        || branch.chars().any(char::is_whitespace)
    {
        return Err(GitError::Refused {
            subcommand: "push".to_string(),
            reason: format!("invalid push branch '{branch}'"),
        });
    }
    if is_protected_branch_ref(branch) {
        return Err(GitError::Refused {
            subcommand: "push".to_string(),
            reason: format!("refusing to push protected branch '{branch}'"),
        });
    }
    Ok(())
}

pub fn push_branch(cwd: &Path, branch: &str, timeout: Duration) -> Result<String, GitError> {
    run_git(
        AllowedGitSubcommand::Push,
        &["-u", "origin", branch],
        cwd,
        timeout,
    )
}

pub fn run_git(
    subcommand: AllowedGitSubcommand,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
) -> Result<String, GitError> {
    if subcommand == AllowedGitSubcommand::Push {
        validate_push_args(args)?;
    }
    let mut cmd = Command::new("git");
    cmd.arg(subcommand.as_str())
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    match wait_child_with_timeout_and_drain(&mut child, timeout, MAX_BROKER_OUTPUT_BYTES)? {
        ChildWaitOutcome::Exited {
            status,
            stdout,
            stderr,
        } => {
            if status.success() {
                Ok(String::from_utf8_lossy(&stdout).to_string())
            } else {
                Err(GitError::Failed {
                    subcommand: subcommand.as_str().to_string(),
                    stderr: String::from_utf8_lossy(&stderr).trim().to_string(),
                })
            }
        }
        ChildWaitOutcome::TimedOut => Err(GitError::Timeout),
    }
}
