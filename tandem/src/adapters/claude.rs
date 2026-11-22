use serde::Serialize;

use crate::conversations::valid_id;
use crate::selfexec::self_command;

use super::base::{
    Adapter, AdapterError, AgentSpec, InteractiveWorkerAdapter, Model, ToolFlag, WorkerCommandSpec,
    WorkerLaunchRequest, session_name, validate_model_id, validate_tool_name,
};

pub const CLAUDE_ALIASES: [&str; 3] = ["opus", "sonnet", "fable"];

#[derive(Serialize)]
struct StatusLineInner<'a> {
    r#type: &'static str,
    command: &'a str,
}

#[derive(Serialize)]
struct StatusLineConfig<'a> {
    #[serde(rename = "statusLine")]
    status_line: StatusLineInner<'a>,
}

pub fn statusline_settings(self_cmd: Option<&[&str]>) -> String {
    let cmd = self_command(self_cmd);
    let command = format!("{cmd} quota-feed");
    let config = StatusLineConfig {
        status_line: StatusLineInner {
            r#type: "command",
            command: &command,
        },
    };
    serde_json::to_string(&config).unwrap_or_default()
}

#[derive(Default)]
pub struct ClaudeAdapter {
    self_cmd: Option<Vec<String>>,
}

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_self_cmd(self_cmd: Vec<String>) -> Self {
        Self {
            self_cmd: Some(self_cmd),
        }
    }
}

impl Adapter for ClaudeAdapter {
    fn cli(&self) -> &'static str {
        "claude"
    }

    fn build_argv(
        &self,
        spec: &AgentSpec,
        resume: bool,
        _log_file: Option<&str>,
    ) -> Result<Vec<String>, AdapterError> {
        if spec.skip_permissions {
            return Err(AdapterError::InvalidInput(
                "claude is always launched with --permission-mode auto".to_string(),
            ));
        }

        let valid_model = validate_model_id(&spec.model)?;
        let self_cmd_refs: Option<Vec<&str>> = self
            .self_cmd
            .as_ref()
            .map(|cmds| cmds.iter().map(|s| s.as_str()).collect());
        let settings = statusline_settings(self_cmd_refs.as_deref());

        let mut argv = vec![
            "claude".to_string(),
            "--permission-mode".to_string(),
            "auto".to_string(),
            "--model".to_string(),
            valid_model,
            "-n".to_string(),
            session_name(spec),
            "--settings".to_string(),
            settings,
        ];

        if let Some(ref conv) = spec.conversation {
            let checked = valid_id(conv).ok_or_else(|| {
                AdapterError::InvalidInput("conversation id must be a UUID".to_string())
            })?;
            argv.push(if resume {
                "--resume".to_string()
            } else {
                "--session-id".to_string()
            });
            argv.push(checked);
        }

        Ok(argv)
    }

    fn list_models(&self) -> Result<Vec<Model>, AdapterError> {
        Ok(CLAUDE_ALIASES
            .iter()
            .map(|&alias| Model {
                id: alias.to_string(),
                label: format!("{alias} (latest)"),
            })
            .collect())
    }
}

impl InteractiveWorkerAdapter for ClaudeAdapter {
    fn build_worker_argv(
        &self,
        request: &WorkerLaunchRequest,
    ) -> Result<WorkerCommandSpec, AdapterError> {
        let valid_model = validate_model_id(&request.model)?;
        let mut allowed = Vec::new();
        let mut disallowed = Vec::new();

        for tool in &request.tools {
            match tool {
                ToolFlag::Allow(name) => allowed.push(validate_tool_name(name)?),
                ToolFlag::Disallow(name) => disallowed.push(validate_tool_name(name)?),
            }
        }

        let self_cmd_refs: Option<Vec<&str>> = self
            .self_cmd
            .as_ref()
            .map(|cmds| cmds.iter().map(|s| s.as_str()).collect());
        let settings = statusline_settings(self_cmd_refs.as_deref());
        let mode = if request.read_only {
            "plan"
        } else {
            "acceptEdits"
        };
        let prompt = format!(
            "Read and execute the task instructions in {}.",
            request.prompt_file.display()
        );

        let mut argv = vec![
            "claude".to_string(),
            "--permission-mode".to_string(),
            mode.to_string(),
            "--model".to_string(),
            valid_model,
            "--settings".to_string(),
            settings,
        ];
        if !allowed.is_empty() {
            argv.push("--allowedTools".to_string());
            argv.push(allowed.join(","));
        }
        if !disallowed.is_empty() {
            argv.push("--disallowedTools".to_string());
            argv.push(disallowed.join(","));
        }
        argv.push(prompt);

        Ok(WorkerCommandSpec {
            argv,
            env: std::collections::BTreeMap::new(),
        })
    }
}
