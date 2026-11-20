use std::fs;
use std::time::{Duration, Instant};

use crate::factory::broker::result::{JobResult, parse_and_validate_result_strict};
use crate::factory::events::reject_if_symlink;
use crate::tmux::client::TmuxClient;
use crate::tmux::tabs::parse_panes;

use super::{LaunchedWorker, WorkerError, WorkerWatchOutcome};

pub const DEFAULT_WATCH_INTERVAL: Duration = Duration::from_millis(200);
const MAX_CONSECUTIVE_POLL_ERRORS: u8 = 3;

fn is_pane_alive(tmux: &TmuxClient, pane_id: &str) -> Result<bool, WorkerError> {
    let args = ["list-panes", "-a", "-F", crate::tmux::tabs::FORMAT];
    let res = tmux.run(&args)?;
    if res.code != 0 {
        return Err(WorkerError::Invalid(format!(
            "tmux list-panes exited {}: {}",
            res.code,
            res.out.trim()
        )));
    }
    let panes = parse_panes(&res.out);
    Ok(panes.iter().any(|p| p.id == pane_id && !p.dead))
}

fn read_relaunched_pane_id(worker: &LaunchedWorker, current: &str) -> Option<String> {
    let pane_path = worker.job_dir.join("pane_id");
    if reject_if_symlink(&pane_path).is_err() || !pane_path.is_file() {
        return None;
    }
    let raw = fs::read_to_string(pane_path).ok()?;
    let trimmed = raw.trim();
    if trimmed.starts_with('%')
        && trimmed.len() > 1
        && trimmed[1..].bytes().all(|b| b.is_ascii_digit())
        && trimmed != current
    {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn check_valid_result(worker: &LaunchedWorker) -> Option<JobResult> {
    if !worker.result_path.is_file() {
        return None;
    }
    parse_and_validate_result_strict(&worker.result_path, worker.attempt > 1).ok()
}

pub fn watch_with_interval(
    tmux: &TmuxClient,
    worker: &LaunchedWorker,
    stop_requested: &dyn Fn() -> bool,
    interval: Duration,
) -> Result<WorkerWatchOutcome, WorkerError> {
    let deadline = Instant::now() + Duration::from_secs(worker.time_limit_secs);
    let mut active_pane_id = worker.pane_id.clone();
    let mut poll_errors: u8 = 0;

    loop {
        if stop_requested() {
            let _ = tmux.kill_pane(&active_pane_id);
            return Ok(WorkerWatchOutcome::Interrupted);
        }

        if let Some(result) = check_valid_result(worker) {
            return Ok(WorkerWatchOutcome::Completed(result));
        }

        if let Some(next_pane) = read_relaunched_pane_id(worker, &active_pane_id) {
            active_pane_id = next_pane;
            poll_errors = 0;
        }

        match is_pane_alive(tmux, &active_pane_id) {
            Ok(true) => {
                poll_errors = 0;
            }
            Ok(false) => {
                if let Some(next_pane) = read_relaunched_pane_id(worker, &active_pane_id) {
                    active_pane_id = next_pane;
                    poll_errors = 0;
                    continue;
                }
                if let Some(result) = check_valid_result(worker) {
                    return Ok(WorkerWatchOutcome::Completed(result));
                }
                let detail = if !worker.result_path.exists() {
                    format!(
                        "worker pane {} exited without writing result.json",
                        active_pane_id
                    )
                } else {
                    match parse_and_validate_result_strict(&worker.result_path, worker.attempt > 1)
                    {
                        Ok(result) => return Ok(WorkerWatchOutcome::Completed(result)),
                        Err(err) => err.to_string(),
                    }
                };
                return Ok(WorkerWatchOutcome::ExitedWithoutResult { detail });
            }
            Err(err) => {
                poll_errors = poll_errors.saturating_add(1);
                if poll_errors >= MAX_CONSECUTIVE_POLL_ERRORS {
                    return Err(err);
                }
            }
        }

        if Instant::now() >= deadline {
            let _ = tmux.kill_pane(&active_pane_id);
            return Ok(WorkerWatchOutcome::TimedOut {
                limit_secs: worker.time_limit_secs,
            });
        }

        std::thread::sleep(interval);
    }
}
