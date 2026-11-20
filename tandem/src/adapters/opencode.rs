use std::collections::BTreeMap;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use crate::conversations::valid_opencode_session_id;

use super::agy::RunnerFn;
use super::base::{
    Adapter, AdapterError, AgentSpec, InteractiveWorkerAdapter, Model, ToolFlag, WorkerCommandSpec,
    WorkerLaunchRequest, validate_opencode_model_id, validate_tool_name,
};

const CONFIG_ENV: &str = "OPENCODE_CONFIG_CONTENT";

fn default_runner(argv: &[&str], _timeout: Duration) -> Result<(i32, String), AdapterError> {
    let Some((program, args)) = argv.split_first() else {
        return Err(AdapterError::ModelList("empty command".to_string()));
    };
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| AdapterError::ModelList(e.to_string()))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok((output.status.code().unwrap_or(1), stdout))
}

pub fn parse_opencode_models(output: &str) -> Vec<Model> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| validate_opencode_model_id(line).is_ok())
        .map(|id| Model {
            id: id.to_string(),
            label: id.to_string(),
        })
        .collect()
}

pub struct OpencodeAdapter {
    runner: RunnerFn,
}

impl Default for OpencodeAdapter {
    fn default() -> Self {
        Self {
            runner: Arc::new(default_runner),
        }
    }
}

impl OpencodeAdapter {
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

impl Adapter for OpencodeAdapter {
    fn cli(&self) -> &'static str {
        "opencode"
    }

    fn build_argv(
        &self,
        spec: &AgentSpec,
        resume: bool,
        _log_file: Option<&str>,
    ) -> Result<Vec<String>, AdapterError> {
        validate_opencode_model_id(&spec.model)?;
        let mut argv = vec!["opencode".to_string()];

        if spec.skip_permissions {
            argv.push("--auto".to_string());
        }

        if resume && let Some(ref conv) = spec.conversation {
            let checked = valid_opencode_session_id(conv).ok_or_else(|| {
                AdapterError::InvalidInput("opencode session id has an invalid shape".to_string())
            })?;
            argv.push("--session".to_string());
            argv.push(checked);
        }

        Ok(argv)
    }

    fn list_models(&self) -> Result<Vec<Model>, AdapterError> {
        let (code, stdout) = (self.runner)(
            &["opencode", "models", "--standalone"],
            Duration::from_secs(10),
        )?;
        if code != 0 {
            return Err(AdapterError::ModelList(
                "opencode models failed".to_string(),
            ));
        }
        let models = parse_opencode_models(&stdout);
        if models.is_empty() {
            return Err(AdapterError::ModelList(
                "opencode models listed nothing".to_string(),
            ));
        }
        Ok(models)
    }

    fn env(&self, spec: &AgentSpec) -> Result<BTreeMap<String, String>, AdapterError> {
        let model = validate_opencode_model_id(&spec.model)?;
        let config = serde_json::json!({ "model": model }).to_string();
        Ok(BTreeMap::from([(CONFIG_ENV.to_string(), config)]))
    }
}

impl InteractiveWorkerAdapter for OpencodeAdapter {
    fn build_worker_argv(
        &self,
        request: &WorkerLaunchRequest,
    ) -> Result<WorkerCommandSpec, AdapterError> {
        let model = validate_opencode_model_id(&request.model)?;
        if request.read_only {
            return Err(AdapterError::InvalidInput(
                "opencode interactive mode does not support read-only enforcement".to_string(),
            ));
        }
        for tool in &request.tools {
            match tool {
                ToolFlag::Disallow(name) => {
                    return Err(AdapterError::InvalidInput(format!(
                        "opencode interactive mode cannot enforce disallowed tool filter: {name}"
                    )));
                }
                ToolFlag::Allow(name) => {
                    validate_tool_name(name)?;
                    if name.contains('(') {
                        return Err(AdapterError::InvalidInput(format!(
                            "opencode interactive mode does not support scoped tool filter: {name}"
                        )));
                    }
                }
            }
        }
        let prompt = format!(
            "Read and execute the task instructions in {}.",
            request.prompt_file.display()
        );
        let config = serde_json::json!({ "model": model }).to_string();
        Ok(WorkerCommandSpec {
            argv: vec![
                "opencode".to_string(),
                "--auto".to_string(),
                "--prompt".to_string(),
                prompt,
            ],
            env: BTreeMap::from([(CONFIG_ENV.to_string(), config)]),
        })
    }
}
