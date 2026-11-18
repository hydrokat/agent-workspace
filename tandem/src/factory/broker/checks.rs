use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::factory::checks::{CheckKind, ConfirmedChecks};

pub const MAX_CHECK_LOG_BYTES: usize = 16 * 1024 * 1024;
const SUMMARY_TAIL_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckOutcome {
    pub kind: CheckKind,
    pub argv: Vec<String>,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub log_path: PathBuf,
    pub timed_out: bool,
    pub log_cap_exceeded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl CheckOutcome {
    pub fn passed(&self) -> bool {
        self.exit_code == Some(0)
            && !self.timed_out
            && !self.log_cap_exceeded
            && self.error.is_none()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CheckRunError {
    #[error("invalid worktree path {}: {reason}", path.display())]
    InvalidWorktree { path: PathBuf, reason: String },
    #[error("refusing symlinked check artifact path {}", path.display())]
    SymlinkPath { path: PathBuf },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

struct LogState {
    file: File,
    bytes_written: usize,
    exceeded: Arc<AtomicBool>,
}

pub fn run_checks(
    config: &ConfirmedChecks,
    kind: CheckKind,
    worktree: &Path,
    artifact_dir: &Path,
    attempt: u32,
) -> Result<Vec<CheckOutcome>, CheckRunError> {
    validate_worktree_dir(worktree)?;

    let checks_dir = artifact_dir.join("checks");
    ensure_real_dir(&checks_dir)?;

    let commands = match kind {
        CheckKind::Test => config.test(),
        CheckKind::Lint => config.lint(),
    };
    let timeout = Duration::from_secs(config.timeout_secs());
    let env = build_scrubbed_env();
    let kind_label = match kind {
        CheckKind::Test => "test",
        CheckKind::Lint => "lint",
    };

    let mut outcomes = Vec::with_capacity(commands.len());
    for (idx, argv) in commands.iter().enumerate() {
        let log_path = checks_dir.join(format!("{kind_label}-{}-attempt{attempt}.log", idx + 1));
        let outcome = run_single_check(kind, argv, worktree, &log_path, timeout, &env)?;
        outcomes.push(outcome);
    }

    Ok(outcomes)
}

fn validate_worktree_dir(worktree: &Path) -> Result<(), CheckRunError> {
    let meta = fs::symlink_metadata(worktree).map_err(|e| CheckRunError::InvalidWorktree {
        path: worktree.to_path_buf(),
        reason: e.to_string(),
    })?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(CheckRunError::InvalidWorktree {
            path: worktree.to_path_buf(),
            reason: "worktree must be a real directory".to_string(),
        });
    }
    Ok(())
}

fn ensure_real_dir(dir: &Path) -> Result<(), CheckRunError> {
    if let Ok(meta) = fs::symlink_metadata(dir) {
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(CheckRunError::SymlinkPath {
                path: dir.to_path_buf(),
            });
        }
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    Ok(())
}

fn open_log_file(log_path: &Path) -> Result<File, CheckRunError> {
    if let Ok(meta) = fs::symlink_metadata(log_path)
        && meta.file_type().is_symlink()
    {
        return Err(CheckRunError::SymlinkPath {
            path: log_path.to_path_buf(),
        });
    }
    Ok(OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)?)
}

fn build_scrubbed_env() -> BTreeMap<String, String> {
    crate::factory::checks::host_check_env()
}

fn find_run_tests_wrapper(worktree: &Path) -> Option<PathBuf> {
    let candidate = worktree.join(".agents/skills/test-failures/scripts/run_tests.py");
    let meta = fs::symlink_metadata(&candidate).ok()?;
    if meta.is_file() && !meta.file_type().is_symlink() {
        Some(candidate)
    } else {
        None
    }
}

fn run_single_check(
    kind: CheckKind,
    argv: &[String],
    worktree: &Path,
    log_path: &Path,
    timeout: Duration,
    env: &BTreeMap<String, String>,
) -> Result<CheckOutcome, CheckRunError> {
    let wrapper = if kind == CheckKind::Test && !argv.iter().any(|a| a.contains("run_tests.py")) {
        find_run_tests_wrapper(worktree)
    } else {
        None
    };

    let (exec_program, exec_args, used_wrapper) = if let Some(ref wrapper_path) = wrapper {
        let mut args = vec![
            wrapper_path.to_string_lossy().to_string(),
            "--cwd".to_string(),
            worktree.to_string_lossy().to_string(),
            "--timeout".to_string(),
            timeout.as_secs().max(1).to_string(),
            "--".to_string(),
        ];
        args.extend(argv.iter().cloned());
        ("python3".to_string(), args, true)
    } else {
        (argv[0].clone(), argv[1..].to_vec(), false)
    };

    let log_file = open_log_file(log_path)?;
    let log_exceeded = Arc::new(AtomicBool::new(false));
    let log_state = Arc::new(Mutex::new(LogState {
        file: log_file,
        bytes_written: 0,
        exceeded: Arc::clone(&log_exceeded),
    }));

    let start = Instant::now();
    let spawn_res = Command::new(&exec_program)
        .args(&exec_args)
        .current_dir(worktree)
        .env_clear()
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match spawn_res {
        Ok(c) => c,
        Err(err) => {
            if let Ok(mut state) = log_state.lock() {
                let _ = writeln!(state.file, "failed to spawn {exec_program}: {err}");
            }
            return Ok(CheckOutcome {
                kind,
                argv: argv.to_vec(),
                exit_code: None,
                duration_ms: start.elapsed().as_millis() as u64,
                log_path: log_path.to_path_buf(),
                timed_out: false,
                log_cap_exceeded: false,
                summary: Some(format!("failed to spawn {exec_program}: {err}")),
                error: Some(err.to_string()),
            });
        }
    };

    let stdout_thread = child
        .stdout
        .take()
        .map(|pipe| spawn_log_pump(pipe, Arc::clone(&log_state)));
    let stderr_thread = child
        .stderr
        .take()
        .map(|pipe| spawn_log_pump(pipe, log_state));

    let mut timed_out = false;
    let mut exit_code = None;

    loop {
        if log_exceeded.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        if let Some(status) = child.try_wait()? {
            exit_code = status.code();
            break;
        }
        if start.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        thread::sleep(Duration::from_millis(15));
    }

    join_io_thread(stdout_thread);
    join_io_thread(stderr_thread);

    let cap_hit = log_exceeded.load(Ordering::Acquire);
    let passed = exit_code == Some(0) && !timed_out && !cap_hit;
    let summary = if used_wrapper || !passed {
        read_summary_tail(log_path)
    } else {
        None
    };

    Ok(CheckOutcome {
        kind,
        argv: argv.to_vec(),
        exit_code,
        duration_ms: start.elapsed().as_millis() as u64,
        log_path: log_path.to_path_buf(),
        timed_out,
        log_cap_exceeded: cap_hit,
        summary,
        error: None,
    })
}

fn join_io_thread(handle: Option<JoinHandle<()>>) {
    if let Some(h) = handle {
        let _ = h.join();
    }
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
                    let remaining = MAX_CHECK_LOG_BYTES.saturating_sub(state.bytes_written);
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

fn read_summary_tail(log_path: &Path) -> Option<String> {
    let bytes = fs::read(log_path).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let slice = if bytes.len() > SUMMARY_TAIL_BYTES {
        &bytes[bytes.len() - SUMMARY_TAIL_BYTES..]
    } else {
        &bytes[..]
    };
    let text = String::from_utf8_lossy(slice).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

pub(crate) fn next_phase_via_specs_py(workspace: &Path) -> Option<String> {
    let script = workspace.join(".agents/skills/spec-planner/scripts/specs.py");
    if !script.is_file() || crate::factory::events::reject_if_symlink(&script).is_err() {
        return None;
    }
    let output = Command::new("python3")
        .arg(&script)
        .arg("--root")
        .arg(workspace)
        .arg("next-phase")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    #[derive(Deserialize)]
    struct NextPhaseOut {
        next: String,
    }
    let parsed: NextPhaseOut = serde_json::from_slice(&output.stdout).ok()?;
    let trimmed = parsed.next.trim();
    if trimmed.len() == 3 && trimmed.bytes().all(|b| b.is_ascii_digit()) {
        Some(trimmed.to_string())
    } else {
        None
    }
}
