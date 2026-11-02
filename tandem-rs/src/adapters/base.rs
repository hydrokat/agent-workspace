use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error(
        "model id must be letters, digits, '.', '_', ':' or '-' and start with a letter or digit"
    )]
    InvalidModel,
    #[error("{0}")]
    InvalidInput(String),
    #[error("could not list models: {0}")]
    ModelList(String),
    #[error("unknown cli: {0}")]
    UnknownCli(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub label: String,
}

fn default_index() -> usize {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSpec {
    pub cli: String,
    pub role: String,
    pub model: String,
    #[serde(default = "default_index")]
    pub index: usize,
    #[serde(default)]
    pub skip_permissions: bool,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub conversation: Option<String>,
}

impl AgentSpec {
    pub fn new(cli: impl Into<String>, role: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            cli: cli.into(),
            role: role.into(),
            model: model.into(),
            index: 1,
            skip_permissions: false,
            cwd: None,
            conversation: None,
        }
    }
}

pub fn validate_model_id(model: &str) -> Result<String, AdapterError> {
    if model.is_empty() || model.len() > 100 {
        return Err(AdapterError::InvalidModel);
    }
    let bytes = model.as_bytes();
    if !bytes[0].is_ascii_alphanumeric() {
        return Err(AdapterError::InvalidModel);
    }
    for &b in &bytes[1..] {
        if !b.is_ascii_alphanumeric() && b != b'.' && b != b'_' && b != b':' && b != b'-' {
            return Err(AdapterError::InvalidModel);
        }
    }
    Ok(model.to_string())
}

pub fn session_id(spec: &AgentSpec) -> String {
    format!("{}-{}", spec.role, spec.index)
}

pub fn session_name(spec: &AgentSpec) -> String {
    format!("tandem-{}", session_id(spec))
}

pub fn session_env(spec: &AgentSpec, coord_dir: &str) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert("TANDEM_SESSION_ID".to_string(), session_id(spec));
    env.insert("TANDEM_ROLE".to_string(), spec.role.clone());
    env.insert("TANDEM_CLI".to_string(), spec.cli.clone());
    env.insert("TANDEM_COORD_DIR".to_string(), coord_dir.to_string());
    env
}

pub trait Adapter: Send + Sync {
    fn cli(&self) -> &'static str;
    fn build_argv(
        &self,
        spec: &AgentSpec,
        resume: bool,
        log_file: Option<&str>,
    ) -> Result<Vec<String>, AdapterError>;
    fn list_models(&self) -> Result<Vec<Model>, AdapterError>;
}
