use std::fs;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

use super::plan::SlicesManifest;
use super::scope::{ScopeCheckInput, verify_scope};
use super::{ContractFailure, StageContext, StageContract};
use crate::factory::events::reject_if_symlink;

pub const MAX_SCREENSHOTS: usize = 20;
pub const MAX_SCREENSHOT_BYTES: usize = 5 * 1024 * 1024;
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoManifest {
    pub screenshots: Vec<DemoScreenshotEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DemoScreenshotEntry {
    pub path: String,
    pub caption: String,
    #[serde(default)]
    pub criterion: Option<String>,
}

pub struct DemoStageContract;

impl StageContract for DemoStageContract {
    fn name(&self) -> &'static str {
        "demo"
    }

    fn verify(&self, ctx: &StageContext<'_>) -> Result<(), ContractFailure> {
        let manifest = load_and_validate_demo_manifest(ctx.artifact_dir)?;
        let required = required_ui_screenshot_count(ctx.artifact_dir);
        if manifest.screenshots.len() < required {
            return Err(ContractFailure::new(
                "demo_screenshot_count",
                format!(
                    "expected at least {required} screenshot(s) for UI acceptance criteria, got {}",
                    manifest.screenshots.len()
                ),
            ));
        }
        verify_scope(&ScopeCheckInput {
            worktree_path: ctx.worktree_path,
            artifact_dir: ctx.artifact_dir,
            has_repo_write: false,
        })?;
        Ok(())
    }
}

fn is_safe_relative_path(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        return false;
    }
    path.components().all(|c| {
        !matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    })
}

pub fn required_ui_screenshot_count(artifact_dir: &Path) -> usize {
    let slices_path = artifact_dir.join("slices.json");
    let Ok(raw) = fs::read_to_string(&slices_path) else {
        return 1;
    };
    let Ok(manifest) = serde_json::from_str::<SlicesManifest>(&raw) else {
        return 1;
    };
    let count: usize = manifest
        .slices
        .iter()
        .filter(|s| s.ui)
        .map(|s| s.acceptance_criteria.len().max(1))
        .sum();
    count.max(1)
}

pub fn load_and_validate_demo_manifest(
    artifact_dir: &Path,
) -> Result<DemoManifest, ContractFailure> {
    let manifest_path = artifact_dir.join("demo.json");
    reject_if_symlink(&manifest_path)
        .map_err(|e| ContractFailure::new("demo_manifest", e.to_string()))?;
    let raw = fs::read_to_string(&manifest_path).map_err(|e| {
        ContractFailure::new(
            "demo_manifest",
            format!("failed to read {}: {e}", manifest_path.display()),
        )
    })?;
    let manifest: DemoManifest = serde_json::from_str(&raw)
        .map_err(|e| ContractFailure::new("demo_manifest", format!("invalid demo.json: {e}")))?;

    if manifest.screenshots.is_empty() {
        return Err(ContractFailure::new(
            "demo_screenshot_count",
            "demo.json must include at least one screenshot",
        ));
    }
    if manifest.screenshots.len() > MAX_SCREENSHOTS {
        return Err(ContractFailure::new(
            "demo_screenshot_count",
            format!(
                "too many screenshots: {} (max {MAX_SCREENSHOTS})",
                manifest.screenshots.len()
            ),
        ));
    }

    for (idx, entry) in manifest.screenshots.iter().enumerate() {
        if entry.caption.trim().is_empty() {
            return Err(ContractFailure::new(
                "demo_caption",
                format!("screenshot #{idx} must have a non-empty caption"),
            ));
        }
        if !is_safe_relative_path(&entry.path) {
            return Err(ContractFailure::new(
                "demo_path",
                format!("screenshot #{idx} has unsafe path '{}'", entry.path),
            ));
        }
        let full = artifact_dir.join(entry.path.trim());
        reject_if_symlink(&full).map_err(|e| ContractFailure::new("demo_path", e.to_string()))?;
        let meta = fs::metadata(&full).map_err(|e| {
            ContractFailure::new(
                "demo_file",
                format!("screenshot #{idx} missing ({}): {e}", full.display()),
            )
        })?;
        if !meta.is_file() {
            return Err(ContractFailure::new(
                "demo_file",
                format!("screenshot #{idx} is not a regular file"),
            ));
        }
        if meta.len() as usize > MAX_SCREENSHOT_BYTES {
            return Err(ContractFailure::new(
                "demo_size",
                format!("screenshot #{idx} exceeds 5 MiB ({} bytes)", meta.len()),
            ));
        }
        let bytes = fs::read(&full).map_err(|e| {
            ContractFailure::new(
                "demo_file",
                format!("failed to read screenshot #{idx}: {e}"),
            )
        })?;
        if bytes.len() < PNG_SIGNATURE.len() || bytes[..8] != PNG_SIGNATURE {
            return Err(ContractFailure::new(
                "demo_format",
                format!("screenshot #{idx} is not a valid PNG"),
            ));
        }
    }

    Ok(manifest)
}
