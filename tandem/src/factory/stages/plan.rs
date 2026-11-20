use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{ContractFailure, StageContext, StageContract};
use crate::factory::broker::git::{AllowedGitSubcommand, run_git};
use crate::factory::events::reject_if_symlink;

const GIT_TIMEOUT: Duration = Duration::from_secs(15);
const ALLOWED_SLICE_TYPES: &[&str] = &["New Feature", "Enhancement", "Bug Fix", "Delivery"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlicesManifest {
    #[serde(default)]
    pub phase: Option<String>,
    pub slices: Vec<Slice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Slice {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub slice_type: String,
    pub paths: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    pub ui: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<crate::factory::routing::Complexity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_tier: Option<crate::factory::routing::ModelTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_rationale: Option<String>,
}

pub struct PlanStageContract;

impl StageContract for PlanStageContract {
    fn name(&self) -> &'static str {
        "plan"
    }

    fn verify(&self, ctx: &StageContext<'_>) -> Result<(), ContractFailure> {
        let manifest = load_and_validate_slices(ctx.artifact_dir)?;
        verify_acceptance_criteria_coverage(ctx.plan_snapshot_path, &manifest)?;
        verify_slice_paths_in_target(ctx.target_repo, &manifest)?;
        verify_result_spec_files(ctx.artifact_dir, &ctx.result.artifacts)?;
        verify_worktree_unchanged(ctx.worktree_path, &ctx.result.files_changed)?;
        Ok(())
    }
}

fn load_and_validate_slices(artifact_dir: &Path) -> Result<SlicesManifest, ContractFailure> {
    let slices_path = artifact_dir.join("slices.json");
    reject_if_symlink(&slices_path)
        .map_err(|e| ContractFailure::new("slices_schema", e.to_string()))?;
    let raw = fs::read_to_string(&slices_path).map_err(|e| {
        ContractFailure::new(
            "slices_schema",
            format!("failed to read {}: {e}", slices_path.display()),
        )
    })?;
    let manifest: SlicesManifest = serde_json::from_str(&raw)
        .map_err(|e| ContractFailure::new("slices_schema", format!("invalid slices.json: {e}")))?;

    if let Some(ref phase) = manifest.phase
        && phase.trim().is_empty()
    {
        return Err(ContractFailure::new(
            "slices_schema",
            "phase must not be empty when present",
        ));
    }
    if manifest.slices.is_empty() {
        return Err(ContractFailure::new(
            "slices_schema",
            "slices array must not be empty",
        ));
    }

    let mut ids = HashSet::new();
    for slice in &manifest.slices {
        let id = slice.id.trim();
        if id.is_empty() {
            return Err(ContractFailure::new(
                "slices_schema",
                "slice id must not be empty",
            ));
        }
        if !ids.insert(id.to_string()) {
            return Err(ContractFailure::new(
                "slices_schema",
                format!("duplicate slice id: {id}"),
            ));
        }
        if slice.title.trim().is_empty() {
            return Err(ContractFailure::new(
                "slices_schema",
                format!("slice {id} has an empty title"),
            ));
        }
        if !ALLOWED_SLICE_TYPES.contains(&slice.slice_type.as_str()) {
            return Err(ContractFailure::new(
                "slices_schema",
                format!(
                    "slice {id} has invalid type '{}' (expected one of: {})",
                    slice.slice_type,
                    ALLOWED_SLICE_TYPES.join(", ")
                ),
            ));
        }
        if slice.paths.iter().any(|p| p.trim().is_empty()) {
            return Err(ContractFailure::new(
                "slices_schema",
                format!("slice {id} contains an empty path"),
            ));
        }
    }

    for slice in &manifest.slices {
        for dep in &slice.dependencies {
            if dep == &slice.id {
                return Err(ContractFailure::new(
                    "slices_schema",
                    format!("slice {} cannot depend on itself", slice.id),
                ));
            }
            if !ids.contains(dep.as_str()) {
                return Err(ContractFailure::new(
                    "slices_schema",
                    format!("slice {} depends on unknown slice '{dep}'", slice.id),
                ));
            }
        }
    }

    Ok(manifest)
}

fn normalize_criterion(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

pub fn extract_acceptance_criteria(plan_markdown: &str) -> Vec<String> {
    let mut criteria = Vec::new();
    let mut in_section = false;
    let mut in_fence: Option<char> = None;

    for line in plan_markdown.lines() {
        let trimmed = line.trim();
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
            in_section = normalized == "acceptance criteria";
            continue;
        }
        if !in_section || trimmed.is_empty() {
            continue;
        }

        let item = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| {
                let digits = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
                if digits > 0 && trimmed[digits..].starts_with(". ") {
                    Some(&trimmed[digits + 2..])
                } else {
                    None
                }
            });

        if let Some(text) = item {
            let clean = text.trim();
            if !clean.is_empty() {
                criteria.push(clean.to_string());
            }
        }
    }

    criteria
}

fn verify_acceptance_criteria_coverage(
    plan_snapshot_path: &Path,
    manifest: &SlicesManifest,
) -> Result<(), ContractFailure> {
    reject_if_symlink(plan_snapshot_path)
        .map_err(|e| ContractFailure::new("acceptance_criteria_coverage", e.to_string()))?;
    let plan_text = fs::read_to_string(plan_snapshot_path).map_err(|e| {
        ContractFailure::new(
            "acceptance_criteria_coverage",
            format!("failed to read {}: {e}", plan_snapshot_path.display()),
        )
    })?;

    let required = extract_acceptance_criteria(&plan_text);
    if required.is_empty() {
        return Err(ContractFailure::new(
            "acceptance_criteria_coverage",
            "plan snapshot contains no acceptance criteria",
        ));
    }

    let covered: HashSet<String> = manifest
        .slices
        .iter()
        .flat_map(|s| s.acceptance_criteria.iter())
        .map(|c| normalize_criterion(c))
        .collect();

    for ac in required {
        if !covered.contains(&normalize_criterion(&ac)) {
            return Err(ContractFailure::new(
                "acceptance_criteria_coverage",
                format!("uncovered acceptance criterion: {ac}"),
            ));
        }
    }

    Ok(())
}

fn is_safe_relative_path(raw: &str) -> bool {
    let p = Path::new(raw);
    if p.is_absolute() {
        return false;
    }
    p.components().all(|c| {
        !matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    })
}

fn canonical_existing_ancestor(path: &Path) -> Option<PathBuf> {
    let mut current = Some(path);
    while let Some(candidate) = current {
        if candidate.exists() {
            return fs::canonicalize(candidate).ok();
        }
        current = candidate.parent();
    }
    None
}

fn verify_slice_paths_in_target(
    target_repo: &Path,
    manifest: &SlicesManifest,
) -> Result<(), ContractFailure> {
    let canon_target = fs::canonicalize(target_repo).map_err(|e| {
        ContractFailure::new(
            "slice_paths_in_target",
            format!(
                "failed to canonicalize target repo {}: {e}",
                target_repo.display()
            ),
        )
    })?;

    for slice in &manifest.slices {
        for raw_path in &slice.paths {
            if !is_safe_relative_path(raw_path) {
                return Err(ContractFailure::new(
                    "slice_paths_in_target",
                    format!(
                        "slice {} path '{}' must be relative to target repository",
                        slice.id, raw_path
                    ),
                ));
            }
            let joined = target_repo.join(raw_path);
            // Canonicalize the nearest existing ancestor to catch symlinks escaping target_repo.
            if let Some(ancestor) = canonical_existing_ancestor(&joined)
                && !ancestor.starts_with(&canon_target)
            {
                return Err(ContractFailure::new(
                    "slice_paths_in_target",
                    format!(
                        "slice {} path '{}' resolves outside target repository",
                        slice.id, raw_path
                    ),
                ));
            }
        }
    }

    Ok(())
}

fn verify_result_spec_files(
    artifact_dir: &Path,
    artifacts: &[String],
) -> Result<(), ContractFailure> {
    if artifacts.is_empty() {
        return Err(ContractFailure::new(
            "result_spec_files_exist",
            "result.artifacts must list generated spec files",
        ));
    }

    let mut has_spec_md = false;
    for rel in artifacts {
        if !is_safe_relative_path(rel) {
            return Err(ContractFailure::new(
                "result_spec_files_exist",
                format!("artifact path '{rel}' must be relative to artifact directory"),
            ));
        }
        let full = artifact_dir.join(rel);
        reject_if_symlink(&full)
            .map_err(|e| ContractFailure::new("result_spec_files_exist", e.to_string()))?;
        if !full.is_file() {
            return Err(ContractFailure::new(
                "result_spec_files_exist",
                format!("named artifact does not exist: {rel}"),
            ));
        }
        if rel.starts_with("specs/") && rel.ends_with(".md") {
            has_spec_md = true;
        }
    }

    if !has_spec_md {
        return Err(ContractFailure::new(
            "result_spec_files_exist",
            "result.artifacts must include at least one spec markdown file under specs/",
        ));
    }

    Ok(())
}

fn verify_worktree_unchanged(
    worktree_path: &Path,
    files_changed: &[String],
) -> Result<(), ContractFailure> {
    if !files_changed.is_empty() {
        return Err(ContractFailure::new(
            "worktree_unchanged",
            format!(
                "plan stage is read-only but result reported files_changed: {}",
                files_changed.join(", ")
            ),
        ));
    }

    let status = run_git(
        AllowedGitSubcommand::Status,
        &["--porcelain"],
        worktree_path,
        GIT_TIMEOUT,
    )
    .map_err(|e| ContractFailure::new("worktree_unchanged", e.to_string()))?;

    if !status.trim().is_empty() {
        return Err(ContractFailure::new(
            "worktree_unchanged",
            format!("worktree has uncommitted changes:\n{}", status.trim()),
        ));
    }

    Ok(())
}
