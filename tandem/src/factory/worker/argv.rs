use crate::adapters::agy::AgyAdapter;
use crate::adapters::base::{
    AdapterError, InteractiveWorkerAdapter, WorkerCommandSpec, WorkerLaunchRequest,
};
use crate::adapters::claude::ClaudeAdapter;
use crate::adapters::opencode::OpencodeAdapter;

pub fn build_worker_command(
    cli: &str,
    request: &WorkerLaunchRequest,
) -> Result<WorkerCommandSpec, AdapterError> {
    match cli {
        "claude" => ClaudeAdapter::new().build_worker_argv(request),
        "agy" => AgyAdapter::new().build_worker_argv(request),
        "opencode" => OpencodeAdapter::new().build_worker_argv(request),
        other => Err(AdapterError::UnknownCli(other.to_string())),
    }
}
