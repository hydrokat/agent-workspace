use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::factory::FactoryError;
use crate::factory::data_policy::{PolicyDenial, check_path};
use crate::factory::events::reject_if_symlink;
use crate::factory::model::FactorySettings;
use crate::factory::permissions::ToolGrant;

pub const MAX_CONTEXT_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextItem {
    pub kind: String,
    pub path: String,
    pub bytes: usize,
    pub sha256: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextManifest {
    pub items: Vec<ContextItem>,
    pub total_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_grant: Option<ToolGrant>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readable_scope: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ContextInputs {
    pub plan_snapshot_path: PathBuf,
    pub acceptance_criteria: Vec<String>,
    pub playbook_path: PathBuf,
    pub agent_def_path: PathBuf,
    pub dependency_summaries: Vec<(String, String)>,
    pub retry_rationale: Option<String>,
    pub tool_grant: Option<ToolGrant>,
}

#[derive(Debug, Clone)]
pub struct AssembledContext {
    pub manifest: ContextManifest,
    pub prompt: String,
}

#[derive(Debug, Clone, Default)]
pub struct ReviewContextExtras {
    pub changed_files: Vec<String>,
    pub diff: String,
    pub evidence: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("context size {total_bytes} bytes exceeds {max_bytes} byte cap")]
    CapExceeded {
        total_bytes: usize,
        max_bytes: usize,
    },
    #[error("{0}")]
    PolicyDenied(#[from] PolicyDenial),
    #[error(transparent)]
    Factory(#[from] FactoryError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::new().chain_update(bytes).finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn read_scoped_file(kind: &str, path: &Path) -> Result<(ContextItem, String), ContextError> {
    reject_if_symlink(path)?;
    let bytes = fs::read(path)?;
    let text = String::from_utf8(bytes.clone()).map_err(|_| {
        FactoryError::InvalidInput(format!(
            "context file is not valid UTF-8: {}",
            path.display()
        ))
    })?;
    let item = ContextItem {
        kind: kind.to_string(),
        path: path.to_string_lossy().to_string(),
        bytes: bytes.len(),
        sha256: sha256_hex(&bytes),
    };
    Ok((item, text))
}

pub fn assemble_context(inputs: &ContextInputs) -> Result<AssembledContext, ContextError> {
    let (plan_item, plan_text) = read_scoped_file("plan_snapshot", &inputs.plan_snapshot_path)?;
    let (playbook_item, playbook_text) = read_scoped_file("playbook", &inputs.playbook_path)?;
    let (agent_item, agent_text) = read_scoped_file("agent_def", &inputs.agent_def_path)?;

    let mut prompt = String::new();
    let _ = writeln!(prompt, "## Plan Snapshot\n{plan_text}");
    if !inputs.acceptance_criteria.is_empty() {
        let _ = writeln!(prompt, "\n## Acceptance Criteria");
        for ac in &inputs.acceptance_criteria {
            let _ = writeln!(prompt, "- {ac}");
        }
    }
    let _ = writeln!(prompt, "\n## Stage Playbook\n{playbook_text}");
    let _ = writeln!(prompt, "\n## Agent Role\n{agent_text}");

    if !inputs.dependency_summaries.is_empty() {
        let _ = writeln!(prompt, "\n## Dependency Summaries");
        for (stage, summary) in &inputs.dependency_summaries {
            let _ = writeln!(prompt, "- **{stage}**: {summary}");
        }
    }

    if let Some(ref rationale) = inputs.retry_rationale {
        let _ = writeln!(
            prompt,
            "\n## Previous Attempt Failure Rationale\n{rationale}"
        );
    }

    let items = vec![plan_item, playbook_item, agent_item];
    let total_bytes = prompt.len();
    if total_bytes > MAX_CONTEXT_BYTES {
        return Err(ContextError::CapExceeded {
            total_bytes,
            max_bytes: MAX_CONTEXT_BYTES,
        });
    }

    Ok(AssembledContext {
        manifest: ContextManifest {
            items,
            total_bytes,
            tool_grant: inputs.tool_grant.clone(),
            excluded_paths: Vec::new(),
            readable_scope: None,
        },
        prompt,
    })
}

pub fn assemble_context_with_policy(
    inputs: &ContextInputs,
    provider: &str,
    settings: &FactorySettings,
) -> Result<AssembledContext, ContextError> {
    check_path(
        &inputs.plan_snapshot_path.to_string_lossy(),
        provider,
        settings,
    )?;
    check_path(&inputs.playbook_path.to_string_lossy(), provider, settings)?;
    check_path(&inputs.agent_def_path.to_string_lossy(), provider, settings)?;
    assemble_context(inputs)
}

pub fn assemble_review_context_with_policy(
    inputs: &ContextInputs,
    extras: &ReviewContextExtras,
    provider: &str,
    settings: &FactorySettings,
) -> Result<AssembledContext, ContextError> {
    check_path(
        &inputs.plan_snapshot_path.to_string_lossy(),
        provider,
        settings,
    )?;
    check_path(&inputs.playbook_path.to_string_lossy(), provider, settings)?;
    check_path(&inputs.agent_def_path.to_string_lossy(), provider, settings)?;
    for changed in &extras.changed_files {
        check_path(changed, provider, settings)?;
    }
    assemble_review_context(inputs, extras)
}

pub fn assemble_review_context(
    inputs: &ContextInputs,
    extras: &ReviewContextExtras,
) -> Result<AssembledContext, ContextError> {
    let base_inputs = ContextInputs {
        plan_snapshot_path: inputs.plan_snapshot_path.clone(),
        acceptance_criteria: inputs.acceptance_criteria.clone(),
        playbook_path: inputs.playbook_path.clone(),
        agent_def_path: inputs.agent_def_path.clone(),
        dependency_summaries: Vec::new(),
        retry_rationale: None,
        tool_grant: inputs.tool_grant.clone(),
    };
    let mut assembled = assemble_context(&base_inputs)?;

    if !extras.changed_files.is_empty() {
        let _ = writeln!(assembled.prompt, "\n## Changed Files");
        for f in &extras.changed_files {
            let _ = writeln!(assembled.prompt, "- {f}");
        }
    }
    if !extras.evidence.trim().is_empty() {
        let _ = writeln!(assembled.prompt, "\n## Evidence\n{}", extras.evidence);
    }
    if !extras.diff.trim().is_empty() {
        let header = "\n## Diff\n```diff\n";
        let footer = "\n```\n";
        let budget = MAX_CONTEXT_BYTES
            .saturating_sub(assembled.prompt.len())
            .saturating_sub(header.len() + footer.len());
        let diff_slice = if extras.diff.len() > budget {
            let mut end = budget;
            while end > 0 && !extras.diff.is_char_boundary(end) {
                end -= 1;
            }
            &extras.diff[..end]
        } else {
            extras.diff.as_str()
        };
        let _ = write!(assembled.prompt, "{header}{diff_slice}{footer}");
    }

    assembled.manifest.total_bytes = assembled.prompt.len();
    if assembled.manifest.total_bytes > MAX_CONTEXT_BYTES {
        return Err(ContextError::CapExceeded {
            total_bytes: assembled.manifest.total_bytes,
            max_bytes: MAX_CONTEXT_BYTES,
        });
    }
    Ok(assembled)
}
