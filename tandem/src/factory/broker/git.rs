use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowedGitSubcommand {
    RevParse,
    SymbolicRef,
    Status,
    Branch,
    Worktree,
    Diff,
    Fetch,
}

impl AllowedGitSubcommand {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RevParse => "rev-parse",
            Self::SymbolicRef => "symbolic-ref",
            Self::Status => "status",
            Self::Branch => "branch",
            Self::Worktree => "worktree",
            Self::Diff => "diff",
            Self::Fetch => "fetch",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "rev-parse" => Some(Self::RevParse),
            "symbolic-ref" => Some(Self::SymbolicRef),
            "status" => Some(Self::Status),
            "branch" => Some(Self::Branch),
            "worktree" => Some(Self::Worktree),
            "diff" => Some(Self::Diff),
            "fetch" => Some(Self::Fetch),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git timed out")]
    Timeout,
    #[error("git {subcommand} failed: {stderr}")]
    Failed { subcommand: String, stderr: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn run_git(
    subcommand: AllowedGitSubcommand,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
) -> Result<String, GitError> {
    let mut cmd = Command::new("git");
    cmd.arg(subcommand.as_str())
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    let start = Instant::now();
    loop {
        if let Some(_status) = child.try_wait()? {
            let output = child.wait_with_output()?;
            if output.status.success() {
                return Ok(String::from_utf8_lossy(&output.stdout).to_string());
            }
            return Err(GitError::Failed {
                subcommand: subcommand.as_str().to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(GitError::Timeout);
        }
        thread::sleep(Duration::from_millis(20));
    }
}
