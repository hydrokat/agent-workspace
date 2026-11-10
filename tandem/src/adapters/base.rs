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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
            name: None,
        }
    }
}

pub fn is_valid_custom_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 32 && name.chars().all(|c| c.is_ascii_alphanumeric())
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

fn valid_opencode_part(part: &str) -> bool {
    let bytes = part.as_bytes();
    !bytes.is_empty()
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

pub fn validate_opencode_model_id(model: &str) -> Result<String, AdapterError> {
    if model.len() > 100 {
        return Err(AdapterError::InvalidModel);
    }
    let (base, variant) = match model.split_once('#') {
        Some((base, variant)) => (base, Some(variant)),
        None => (model, None),
    };
    let Some((provider, name)) = base.split_once('/') else {
        return Err(AdapterError::InvalidModel);
    };
    let variant_ok = variant.is_none_or(valid_opencode_part);
    if valid_opencode_part(provider) && valid_opencode_part(name) && variant_ok {
        Ok(model.to_string())
    } else {
        Err(AdapterError::InvalidModel)
    }
}

pub fn validate_model_for_cli(cli: &str, model: &str) -> Result<String, AdapterError> {
    match cli {
        "opencode" => validate_opencode_model_id(model),
        _ => validate_model_id(model),
    }
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
    fn env(&self, _spec: &AgentSpec) -> Result<BTreeMap<String, String>, AdapterError> {
        Ok(BTreeMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_custom_name() {
        assert!(is_valid_custom_name("AlphaNumeric123"));
        assert!(is_valid_custom_name("a"));
        assert!(is_valid_custom_name("12345678901234567890123456789012")); // 32 chars
        assert!(!is_valid_custom_name(""));
        assert!(!is_valid_custom_name("has space"));
        assert!(!is_valid_custom_name("with-dash"));
        assert!(!is_valid_custom_name("with_underscore"));
        assert!(!is_valid_custom_name("with.dot"));
        assert!(!is_valid_custom_name("123456789012345678901234567890123")); // 33 chars
    }
}
