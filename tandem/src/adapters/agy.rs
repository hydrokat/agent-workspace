use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use crate::conversations::valid_id;

use super::base::{Adapter, AdapterError, AgentSpec, Model, validate_model_id};

pub type RunnerFn =
    Arc<dyn Fn(&[&str], Duration) -> Result<(i32, String), AdapterError> + Send + Sync>;

fn default_runner(argv: &[&str], _timeout: Duration) -> Result<(i32, String), AdapterError> {
    if argv.is_empty() {
        return Err(AdapterError::ModelList("empty command".to_string()));
    }
    let output = Command::new(argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| AdapterError::ModelList(e.to_string()))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let code = output.status.code().unwrap_or(1);
    Ok((code, stdout))
}

pub fn parse_models(output: &str) -> Vec<Model> {
    let mut models = Vec::new();
    for line in output.lines() {
        if let Some((raw_id, raw_label)) = line.split_once('\t')
            && validate_model_id(raw_id).is_ok()
        {
            let cleaned_label: String = raw_label
                .chars()
                .filter(|&c| {
                    !matches!(
                        c,
                        '\x00'..='\x08' | '\x0b'..='\x1f' | '\x7f'..='\u{9f}'
                    )
                })
                .collect();
            let trimmed = cleaned_label.trim();
            if !trimmed.is_empty() {
                models.push(Model {
                    id: raw_id.to_string(),
                    label: trimmed.to_string(),
                });
            }
        }
    }
    models
}

pub struct AgyAdapter {
    runner: RunnerFn,
}

impl Default for AgyAdapter {
    fn default() -> Self {
        Self {
            runner: Arc::new(default_runner),
        }
    }
}

impl AgyAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_runner<F>(runner: F) -> Self
    where
        F: Fn(&[&str], Duration) -> Result<(i32, String), AdapterError> + Send + Sync + 'static,
    {
        Self {
            runner: Arc::new(runner),
        }
    }
}

impl Adapter for AgyAdapter {
    fn cli(&self) -> &'static str {
        "agy"
    }

    fn build_argv(
        &self,
        spec: &AgentSpec,
        resume: bool,
        log_file: Option<&str>,
    ) -> Result<Vec<String>, AdapterError> {
        let valid_model = validate_model_id(&spec.model)?;
        let mut argv = vec!["agy".to_string(), "--model".to_string(), valid_model];

        if spec.skip_permissions {
            argv.push("--dangerously-skip-permissions".to_string());
        }

        if let Some(log) = log_file {
            if !log.starts_with('/') {
                return Err(AdapterError::InvalidInput(
                    "log file must be an absolute path".to_string(),
                ));
            }
            argv.push("--log-file".to_string());
            argv.push(log.to_string());
        }

        if resume && let Some(ref conv) = spec.conversation {
            let checked = valid_id(conv).ok_or_else(|| {
                AdapterError::InvalidInput("conversation id must be a UUID".to_string())
            })?;
            argv.push("--conversation".to_string());
            argv.push(checked);
        }

        Ok(argv)
    }

    fn list_models(&self) -> Result<Vec<Model>, AdapterError> {
        let (code, stdout) = (self.runner)(&["agy", "models"], Duration::from_secs(10))?;
        if code != 0 {
            return Err(AdapterError::ModelList("agy models failed".to_string()));
        }
        Ok(parse_models(&stdout))
    }
}
