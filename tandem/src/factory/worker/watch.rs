use std::time::{Duration, Instant};

use crate::factory::broker::result::{JobResult, parse_and_validate_result_strict};
use crate::tmux::client::TmuxClient;
use crate::tmux::tabs::parse_panes;

use super::{LaunchedWorker, WorkerError, WorkerWatchOutcome};

pub const DEFAULT_WATCH_INTERVAL: Duration = Duration::from_millis(200);

fn is_pane_alive(tmux: &TmuxClient, pane_id: &str) -> Result<bool, WorkerError> {
    let args = ["list-panes", "-a", "-F", crate::tmux::tabs::FORMAT];
    let res = tmux.run(&args)?;
    if res.code != 0 {
        return Ok(false);
    }
    let panes = parse_panes(&res.out);
    Ok(panes.iter().any(|p| p.id == pane_id && !p.dead))
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

    loop {
        if stop_requested() {
            let _ = tmux.kill_pane(&worker.pane_id);
            return Ok(WorkerWatchOutcome::Interrupted);
        }

        if let Some(result) = check_valid_result(worker) {
            return Ok(WorkerWatchOutcome::Completed(result));
        }

        if !is_pane_alive(tmux, &worker.pane_id)? {
            if let Some(result) = check_valid_result(worker) {
                return Ok(WorkerWatchOutcome::Completed(result));
            }
            let detail = if !worker.result_path.exists() {
                format!(
                    "worker pane {} exited without writing result.json",
                    worker.pane_id
                )
            } else {
                match parse_and_validate_result_strict(&worker.result_path, worker.attempt > 1) {
                    Ok(result) => return Ok(WorkerWatchOutcome::Completed(result)),
                    Err(err) => err.to_string(),
                }
            };
            return Ok(WorkerWatchOutcome::ExitedWithoutResult { detail });
        }

        if Instant::now() >= deadline {
            let _ = tmux.kill_pane(&worker.pane_id);
            return Ok(WorkerWatchOutcome::TimedOut {
                limit_secs: worker.time_limit_secs,
            });
        }

        std::thread::sleep(interval);
    }
}
