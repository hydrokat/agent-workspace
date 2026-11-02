use serde::Serialize;

use crate::conversations::valid_id;
use crate::selfexec::self_command;

use super::base::{Adapter, AdapterError, AgentSpec, Model, session_name, validate_model_id};

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
