pub mod parse;
pub mod scheduler;
pub mod schema;
pub mod validate;

use std::path::Path;

use parse::{ParseError, RawWorkflow, load_workflow_file, parse_workflow_str};

pub const DEFAULT_WORKFLOW_YAML: &str = include_str!("default-delivery.yaml");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowSource {
    Workspace,
    Embedded,
}

impl WorkflowSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Embedded => "embedded",
        }
    }
}

/// Loads `<workspace>/workflows/default-delivery.yaml` if present, or falls back to the embedded default.
pub fn load_default_workflow(workspace: &Path) -> Result<(RawWorkflow, WorkflowSource), ParseError> {
    let path = workspace.join("workflows").join("default-delivery.yaml");
    if path.exists() {
        Ok((load_workflow_file(&path)?, WorkflowSource::Workspace))
    } else {
        use sha2::{Digest, Sha256};
        let mut wf = parse_workflow_str(DEFAULT_WORKFLOW_YAML)?;
        let digest = Sha256::new().chain_update(DEFAULT_WORKFLOW_YAML.as_bytes()).finalize();
        wf.sha256 = digest.iter().map(|b| format!("{b:02x}")).collect();
        wf.raw_text = DEFAULT_WORKFLOW_YAML.to_string();
        Ok((wf, WorkflowSource::Embedded))
    }
}
