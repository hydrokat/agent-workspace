pub mod browser;
pub mod checks;
pub mod context;
pub mod gh;
pub mod git;
pub mod job;
pub mod result;
pub mod terms;

use std::io::{self, Read};
use std::process::{Child, ExitStatus};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub(crate) const MAX_BROKER_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

pub(crate) fn current_process_id() -> u32 {
    std::process::id()
}

pub(crate) enum ChildWaitOutcome {
    Exited {
        status: ExitStatus,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    TimedOut,
}

fn spawn_bounded_reader<R: Read + Send + 'static>(
    mut reader: R,
    max_bytes: usize,
) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut out = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let remaining = max_bytes.saturating_sub(out.len());
                    if remaining > 0 {
                        let take = n.min(remaining);
                        out.extend_from_slice(&buf[..take]);
                    }
                    // Keep draining past the cap so the child never blocks on a full pipe.
                }
            }
        }
        out
    })
}

pub(crate) fn wait_child_with_timeout_and_drain(
    child: &mut Child,
    timeout: Duration,
    max_bytes: usize,
) -> io::Result<ChildWaitOutcome> {
    let stdout_handle = child
        .stdout
        .take()
        .map(|pipe| spawn_bounded_reader(pipe, max_bytes));
    let stderr_handle = child
        .stderr
        .take()
        .map(|pipe| spawn_bounded_reader(pipe, max_bytes));

    let deadline = Instant::now() + timeout;
    let mut exit_status = None;
    loop {
        if let Some(status) = child.try_wait()? {
            exit_status = Some(status);
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }

    let stdout = stdout_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    let stderr = stderr_handle
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    match exit_status {
        Some(status) => Ok(ChildWaitOutcome::Exited {
            status,
            stdout,
            stderr,
        }),
        None => Ok(ChildWaitOutcome::TimedOut),
    }
}
