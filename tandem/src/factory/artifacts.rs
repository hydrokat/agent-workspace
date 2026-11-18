use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use super::FactoryError;
use super::ids::is_valid_idea_id;
use super::model::ArtifactRef;
use super::redact::{RedactContext, Redacted, redact};
use super::store::FactoryStore;

pub const MAX_ARTIFACT_BYTES: usize = 256 * 1024;
pub const REQUIRED_PLAN_SECTIONS: &[&str] = &[
    "Scope",
    "Acceptance criteria",
    "Risks",
    "Estimates",
    "Workflow recommendation",
];

fn validate_kind(kind: &str) -> Result<(), FactoryError> {
    if matches!(kind, "feasibility" | "plan") {
        Ok(())
    } else {
        Err(FactoryError::InvalidInput(format!(
            "invalid artifact kind: {kind} (expected feasibility or plan)"
        )))
    }
}

fn validate_markdown_bytes(bytes: &[u8]) -> Result<&str, FactoryError> {
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(FactoryError::InvalidInput(format!(
            "artifact size {} exceeds {MAX_ARTIFACT_BYTES} byte limit",
            bytes.len()
        )));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| FactoryError::InvalidInput("artifact must be valid UTF-8".to_string()))?;
    if let Some(ch) = text
        .chars()
        .find(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(FactoryError::InvalidInput(format!(
            "artifact contains disallowed control character (U+{:04X})",
            ch as u32
        )));
    }
    Ok(text)
}

pub fn put(
    store: &FactoryStore,
    idea_id: &str,
    kind: &str,
    source_path: &Path,
) -> Result<ArtifactRef, FactoryError> {
    if !is_valid_idea_id(idea_id) {
        return Err(FactoryError::InvalidInput(format!(
            "invalid idea id: {idea_id}"
        )));
    }
    validate_kind(kind)?;

    let meta = fs::symlink_metadata(source_path)?;
    if meta.file_type().is_symlink() {
        return Err(FactoryError::Symlink(source_path.to_path_buf()));
    }
    if !meta.file_type().is_file() {
        return Err(FactoryError::InvalidInput(format!(
            "artifact source is not a regular file: {}",
            source_path.display()
        )));
    }
    if meta.len() as usize > MAX_ARTIFACT_BYTES {
        return Err(FactoryError::InvalidInput(format!(
            "artifact size {} exceeds {MAX_ARTIFACT_BYTES} byte limit",
            meta.len()
        )));
    }

    let bytes = fs::read(source_path)?;
    let text = validate_markdown_bytes(&bytes)?;
    let redacted = redact(text, &RedactContext::from_env());
    put_redacted(store, idea_id, kind, &redacted)
}

pub fn put_redacted(
    store: &FactoryStore,
    idea_id: &str,
    kind: &str,
    content: &Redacted,
) -> Result<ArtifactRef, FactoryError> {
    if !is_valid_idea_id(idea_id) {
        return Err(FactoryError::InvalidInput(format!(
            "invalid idea id: {idea_id}"
        )));
    }
    validate_kind(kind)?;
    let bytes = content.as_str().as_bytes();
    let _ = validate_markdown_bytes(bytes)?;

    let artifacts_dir = store.root.join("ideas").join(idea_id).join("artifacts");
    fs::create_dir_all(&artifacts_dir)?;

    let prefix = format!("{kind}-v");
    let mut max_ver = 0u32;
    for entry in fs::read_dir(&artifacts_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(rest) = name.strip_prefix(&prefix)
            && let Some(num_str) = rest.strip_suffix(".md")
            && let Ok(v) = num_str.parse::<u32>()
        {
            max_ver = max_ver.max(v);
        }
    }
    let next_ver = max_ver + 1;
    let filename = format!("{kind}-v{next_ver}.md");
    let dest = artifacts_dir.join(&filename);
    let tmp = artifacts_dir.join(format!(".{filename}.tmp"));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, &dest)?;

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let sha256: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);

    Ok(ArtifactRef {
        kind: kind.to_string(),
        version: next_ver,
        path: format!("ideas/{idea_id}/artifacts/{filename}"),
        sha256,
        bytes: bytes.len() as u64,
        created,
    })
}

pub fn format_for_display(content: &Redacted) -> &str {
    content.as_str()
}

pub fn read(
    store: &FactoryStore,
    idea_id: &str,
    kind: &str,
    version: Option<u32>,
) -> Result<String, FactoryError> {
    if !is_valid_idea_id(idea_id) {
        return Err(FactoryError::InvalidInput(format!(
            "invalid idea id: {idea_id}"
        )));
    }
    validate_kind(kind)?;
    let artifacts_dir = store.root.join("ideas").join(idea_id).join("artifacts");
    let target_ver = match version {
        Some(v) => v,
        None => {
            let prefix = format!("{kind}-v");
            let mut max_ver = 0u32;
            if artifacts_dir.exists() {
                for entry in fs::read_dir(&artifacts_dir)? {
                    let entry = entry?;
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if let Some(rest) = name.strip_prefix(&prefix)
                        && let Some(num_str) = rest.strip_suffix(".md")
                        && let Ok(v) = num_str.parse::<u32>()
                    {
                        max_ver = max_ver.max(v);
                    }
                }
            }
            if max_ver == 0 {
                return Err(FactoryError::InvalidInput(format!(
                    "no {kind} artifact stored for {idea_id}"
                )));
            }
            max_ver
        }
    };
    let path = artifacts_dir.join(format!("{kind}-v{target_ver}.md"));
    let meta = fs::symlink_metadata(&path)?;
    if meta.file_type().is_symlink() {
        return Err(FactoryError::Symlink(path));
    }
    Ok(fs::read_to_string(path)?)
}

pub fn missing_plan_sections(text: &str) -> Vec<String> {
    let mut found = std::collections::HashSet::new();
    let mut in_fence: Option<char> = None;

    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(fence_ch) = in_fence {
            let fence = if fence_ch == '`' { "```" } else { "~~~" };
            if trimmed.starts_with(fence) {
                in_fence = None;
            }
            continue;
        }
        if trimmed.starts_with("```") {
            in_fence = Some('`');
            continue;
        }
        if trimmed.starts_with("~~~") {
            in_fence = Some('~');
            continue;
        }
        if let Some(heading) = trimmed.strip_prefix("## ") {
            let normalized = heading
                .trim()
                .trim_end_matches('#')
                .trim()
                .to_ascii_lowercase();
            found.insert(normalized);
        }
    }

    REQUIRED_PLAN_SECTIONS
        .iter()
        .filter(|sec| !found.contains(&sec.to_ascii_lowercase()))
        .map(|sec| (*sec).to_string())
        .collect()
}
