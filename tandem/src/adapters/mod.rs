pub mod agy;
pub mod base;
pub mod claude;

pub use agy::{AgyAdapter, RunnerFn, parse_models};
pub use base::{
    Adapter, AdapterError, AgentSpec, Model, session_env, session_id, session_name,
    validate_model_id,
};
pub use claude::{CLAUDE_ALIASES, ClaudeAdapter, statusline_settings};

pub fn adapter_for(cli: &str) -> Result<Box<dyn Adapter>, AdapterError> {
    match cli {
        "claude" => Ok(Box::new(ClaudeAdapter::default())),
        "agy" => Ok(Box::new(AgyAdapter::default())),
        _ => Err(AdapterError::UnknownCli(cli.to_string())),
    }
}
