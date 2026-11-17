use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::gate::GatePass;
use crate::state::CLIS;

pub const MAX_LOG_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_STOP_GRACE: Duration = Duration::from_secs(10);

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

#[derive(Debug, Clone)]
pub struct ProviderCommand {
    pub program: String,
    pub args: Vec<String>,
    pub workspace_root: PathBuf,
    pub cwd: PathBuf,
    pub log_path: PathBuf,
    pub stdin_prompt: Option<String>,
    pub factory_env: BTreeMap<String, String>,
    pub parent_env: BTreeMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    #[error("unknown provider CLI program: {program}")]
    UnknownProgram { program: String },
    #[error("program {actual} does not match gate pass provider {expected}")]
    ProgramMismatch { expected: String, actual: String },
    #[error("invalid worktree path {}: {reason}", path.display())]
    InvalidWorktree { path: PathBuf, reason: String },
    #[error("provider CLI {program} not found in PATH")]
    ProgramNotFound { program: String },
    #[error("job log exceeded {limit} byte cap")]
    LogCapExceeded { limit: usize },
    #[error("job timed out after {timeout:?}")]
    TimedOut { timeout: Duration },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

struct LogState {
    file: File,
    bytes_written: usize,
    exceeded: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    Exited(ExitStatus),
    Stopped(ExitStatus),
}

#[derive(Debug)]
pub struct RunningJob {
    child: Child,
    stdin: Option<ChildStdin>,
    stdin_thread: Option<JoinHandle<()>>,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_thread: Option<JoinHandle<()>>,
    log_exceeded: Arc<AtomicBool>,
}

impl RunningJob {
    pub fn wait(&mut self, timeout: Duration) -> Result<ExitStatus, SpawnError> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.log_exceeded.load(Ordering::Acquire) {
                self.kill_and_reap();
                self.join_io_threads();
                return Err(SpawnError::LogCapExceeded {
                    limit: MAX_LOG_BYTES,
                });
            }

            if let Some(status) = self.child.try_wait()? {
                self.join_io_threads();
                if self.log_exceeded.load(Ordering::Acquire) {
                    return Err(SpawnError::LogCapExceeded {
                        limit: MAX_LOG_BYTES,
                    });
                }
                return Ok(status);
            }

            let now = Instant::now();
            if now >= deadline {
                return Err(SpawnError::TimedOut { timeout });
            }

            let sleep_for = (deadline - now).min(Duration::from_millis(15));
            thread::sleep(sleep_for);
        }
    }

    pub fn wait_with_stop(
        &mut self,
        timeout: Duration,
        should_stop: &dyn Fn() -> bool,
        grace: Duration,
    ) -> Result<WaitOutcome, SpawnError> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.log_exceeded.load(Ordering::Acquire) {
                self.kill_and_reap();
                self.join_io_threads();
                return Err(SpawnError::LogCapExceeded {
                    limit: MAX_LOG_BYTES,
                });
            }

            if let Some(status) = self.child.try_wait()? {
                self.join_io_threads();
                if self.log_exceeded.load(Ordering::Acquire) {
                    return Err(SpawnError::LogCapExceeded {
                        limit: MAX_LOG_BYTES,
                    });
                }
                return Ok(WaitOutcome::Exited(status));
            }

            if should_stop() {
                let status = self.stop(grace)?;
                return Ok(WaitOutcome::Stopped(status));
            }

            let now = Instant::now();
            if now >= deadline {
                return Err(SpawnError::TimedOut { timeout });
            }

            let sleep_for = (deadline - now).min(Duration::from_millis(15));
            thread::sleep(sleep_for);
        }
    }

    pub fn stop(&mut self, grace: Duration) -> Result<ExitStatus, SpawnError> {
        drop(self.stdin.take());

        let deadline = Instant::now() + grace;
        loop {
            if let Some(status) = self.child.try_wait()? {
                self.join_io_threads();
                if self.log_exceeded.load(Ordering::Acquire) {
                    return Err(SpawnError::LogCapExceeded {
                        limit: MAX_LOG_BYTES,
                    });
                }
                return Ok(status);
            }

            let now = Instant::now();
            if now >= deadline {
                break;
            }

            let sleep_for = (deadline - now).min(Duration::from_millis(15));
            thread::sleep(sleep_for);
        }

        let _ = self.child.kill();
        let status = self.child.wait()?;
        self.join_io_threads();
        if self.log_exceeded.load(Ordering::Acquire) {
            return Err(SpawnError::LogCapExceeded {
                limit: MAX_LOG_BYTES,
            });
        }
        Ok(status)
    }

    fn kill_and_reap(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn join_io_threads(&mut self) {
        if let Some(handle) = self.stdin_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stdout_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for RunningJob {
    fn drop(&mut self) {
        drop(self.stdin.take());
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        self.join_io_threads();
    }
}

pub fn spawn_provider(pass: GatePass, command: ProviderCommand) -> Result<RunningJob, SpawnError> {
    if !CLIS.contains(&command.program.as_str()) {
        return Err(SpawnError::UnknownProgram {
            program: command.program,
        });
    }
    if command.program != pass.provider() {
        return Err(SpawnError::ProgramMismatch {
            expected: pass.provider().to_string(),
            actual: command.program,
        });
    }

    let canon_cwd = validate_worktree(&command.workspace_root, &command.cwd)?;
    let env = build_env(&command.program, &command.parent_env, &command.factory_env);

    let path_var = env.get("PATH").map(String::as_str).unwrap_or("");
    let resolved_program =
        find_in_path(&command.program, path_var).ok_or_else(|| SpawnError::ProgramNotFound {
            program: command.program.clone(),
        })?;

    let log_file = open_log_file(&command.log_path)?;
    let log_exceeded = Arc::new(AtomicBool::new(false));
    let log_state = Arc::new(Mutex::new(LogState {
        file: log_file,
        bytes_written: 0,
        exceeded: Arc::clone(&log_exceeded),
    }));

    let mut child = Command::new(&resolved_program)
        .args(&command.args)
        .current_dir(&canon_cwd)
        .env_clear()
        .envs(&env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let mut stdin = child.stdin.take();
    let stdin_thread = if let Some(prompt) = command.stdin_prompt {
        stdin.take().map(|mut pipe| {
            thread::spawn(move || {
                let _ = pipe.write_all(prompt.as_bytes());
                let _ = pipe.flush();
            })
        })
    } else {
        None
    };

    let stdout_thread = child
        .stdout
        .take()
        .map(|pipe| spawn_log_pump(pipe, Arc::clone(&log_state)));
    let stderr_thread = child
        .stderr
        .take()
        .map(|pipe| spawn_log_pump(pipe, log_state));

    Ok(RunningJob {
        child,
        stdin,
        stdin_thread,
        stdout_thread,
        stderr_thread,
        log_exceeded,
    })
}

fn validate_worktree(workspace_root: &Path, cwd: &Path) -> Result<PathBuf, SpawnError> {
    let worktrees_dir = workspace_root.join("worktrees");
    let wt_meta =
        fs::symlink_metadata(&worktrees_dir).map_err(|e| SpawnError::InvalidWorktree {
            path: worktrees_dir.clone(),
            reason: e.to_string(),
        })?;
    if wt_meta.file_type().is_symlink() || !wt_meta.is_dir() {
        return Err(SpawnError::InvalidWorktree {
            path: worktrees_dir,
            reason: "worktrees root must be a real directory".to_string(),
        });
    }

    let cwd_meta = fs::symlink_metadata(cwd).map_err(|e| SpawnError::InvalidWorktree {
        path: cwd.to_path_buf(),
        reason: e.to_string(),
    })?;
    if cwd_meta.file_type().is_symlink() || !cwd_meta.is_dir() {
        return Err(SpawnError::InvalidWorktree {
            path: cwd.to_path_buf(),
            reason: "working directory must be a real directory".to_string(),
        });
    }

    let canon_worktrees =
        worktrees_dir
            .canonicalize()
            .map_err(|e| SpawnError::InvalidWorktree {
                path: worktrees_dir,
                reason: e.to_string(),
            })?;
    let canon_cwd = cwd
        .canonicalize()
        .map_err(|e| SpawnError::InvalidWorktree {
            path: cwd.to_path_buf(),
            reason: e.to_string(),
        })?;

    if !canon_cwd.starts_with(&canon_worktrees) || canon_cwd == canon_worktrees {
        return Err(SpawnError::InvalidWorktree {
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

fn build_env(
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

fn find_in_path(program: &str, path_var: &str) -> Option<PathBuf> {
    for dir in std::env::split_paths(path_var) {
        let candidate = dir.join(program);
        if let Ok(meta) = fs::metadata(&candidate)
            && meta.is_file()
            && meta.permissions().mode() & 0o111 != 0
        {
            return Some(candidate);
        }
    }
    None
}

fn open_log_file(log_path: &Path) -> Result<File, SpawnError> {
    if let Some(parent) = log_path.parent() {
        if let Ok(meta) = fs::symlink_metadata(parent)
            && meta.file_type().is_symlink()
        {
            return Err(SpawnError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("refusing symlinked log directory {}", parent.display()),
            )));
        }
        fs::create_dir_all(parent)?;
    }
    if let Ok(meta) = fs::symlink_metadata(log_path)
        && meta.file_type().is_symlink()
    {
        return Err(SpawnError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing symlinked log file {}", log_path.display()),
        )));
    }

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)?;
    Ok(file)
}

fn spawn_log_pump<R: Read + Send + 'static>(
    mut reader: R,
    log_state: Arc<Mutex<LogState>>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let Ok(mut state) = log_state.lock() else {
                        break;
                    };
                    if state.exceeded.load(Ordering::Acquire) {
                        break;
                    }
                    let remaining = MAX_LOG_BYTES.saturating_sub(state.bytes_written);
                    if n <= remaining {
                        if state.file.write_all(&buf[..n]).is_err() {
                            break;
                        }
                        state.bytes_written += n;
                    } else {
                        if remaining > 0 {
                            let _ = state.file.write_all(&buf[..remaining]);
                            state.bytes_written += remaining;
                        }
                        state.exceeded.store(true, Ordering::Release);
                        break;
                    }
                }
            }
        }
    })
}
