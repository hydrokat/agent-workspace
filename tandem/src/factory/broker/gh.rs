use std::io::ErrorKind;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use super::{ChildWaitOutcome, MAX_BROKER_OUTPUT_BYTES, wait_child_with_timeout_and_drain};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowedGhSubcommand {
    AuthStatus,
    RepoView,
    PrCreate,
    PrView,
    PrList,
}

impl AllowedGhSubcommand {
    pub fn args(self) -> &'static [&'static str] {
        match self {
            Self::AuthStatus => &["auth", "status"],
            Self::RepoView => &["repo", "view"],
            Self::PrCreate => &["pr", "create"],
            Self::PrView => &["pr", "view"],
            Self::PrList => &["pr", "list"],
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::AuthStatus => "auth status",
            Self::RepoView => "repo view",
            Self::PrCreate => "pr create",
            Self::PrView => "pr view",
            Self::PrList => "pr list",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "auth status" => Some(Self::AuthStatus),
            "repo view" => Some(Self::RepoView),
            "pr create" => Some(Self::PrCreate),
            "pr view" => Some(Self::PrView),
            "pr list" => Some(Self::PrList),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GhError {
    #[error("gh is not installed")]
    NotInstalled,
    #[error("gh timed out")]
    Timeout,
    #[error("gh {subcommand} failed: {stderr}")]
    Failed { subcommand: String, stderr: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn run_gh(
    subcommand: AllowedGhSubcommand,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
) -> Result<String, GhError> {
    run_gh_bin(Path::new("gh"), subcommand, args, cwd, timeout)
}

pub fn run_gh_bin(
    gh_binary: &Path,
    subcommand: AllowedGhSubcommand,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
) -> Result<String, GhError> {
    let mut cmd = Command::new(gh_binary);
    cmd.args(subcommand.args())
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) if err.kind() == ErrorKind::NotFound => return Err(GhError::NotInstalled),
        Err(err) => return Err(GhError::Io(err)),
    };

    match wait_child_with_timeout_and_drain(&mut child, timeout, MAX_BROKER_OUTPUT_BYTES)? {
        ChildWaitOutcome::Exited {
            status,
            stdout,
            stderr,
        } => {
            if status.success() {
                Ok(String::from_utf8_lossy(&stdout).to_string())
            } else {
                Err(GhError::Failed {
                    subcommand: subcommand.as_str().to_string(),
                    stderr: String::from_utf8_lossy(&stderr).trim().to_string(),
                })
            }
        }
        ChildWaitOutcome::TimedOut => Err(GhError::Timeout),
    }
}
