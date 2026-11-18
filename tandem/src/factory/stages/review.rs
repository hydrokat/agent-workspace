use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::scope::{ScopeCheckInput, verify_scope};
use super::{ContractFailure, StageContext, StageContract};
use crate::factory::broker::git::{AllowedGitSubcommand, run_git};
use crate::factory::events::reject_if_symlink;
use crate::factory::store::write_atomic;

const GIT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Blocker,
    Major,
    Minor,
    Nit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Approve,
    ChangesRequested,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub severity: FindingSeverity,
    pub file: String,
    pub line: u32,
    pub title: String,
    pub detail: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingsReport {
    pub verdict: ReviewVerdict,
    #[serde(default)]
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRoundRecord {
    pub round: u32,
    pub stated_verdict: ReviewVerdict,
    pub derived_verdict: ReviewVerdict,
    pub verdict_overridden: bool,
    pub findings: Vec<Finding>,
    #[serde(default)]
    pub fixed_from_previous: Vec<String>,
    #[serde(default)]
    pub remaining_from_previous: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRoundsReport {
    pub rounds: Vec<ReviewRoundRecord>,
}

pub fn derive_verdict(findings: &[Finding]) -> ReviewVerdict {
    if findings.iter().any(|f| {
        matches!(
            f.severity,
            FindingSeverity::Blocker | FindingSeverity::Major
        )
    }) {
        ReviewVerdict::ChangesRequested
    } else {
        ReviewVerdict::Approve
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

fn diff_files(worktree_path: &Path, base_commit: Option<&str>) -> BTreeSet<String> {
    let range = base_commit
        .filter(|b| !b.trim().is_empty())
        .map(|b| format!("{b}..HEAD"));
    let args: Vec<&str> = match range.as_deref() {
        Some(r) => vec!["--name-only", r],
        None => vec!["--name-only", "HEAD"],
    };
    let Ok(out) = run_git(
        AllowedGitSubcommand::Diff,
        &args,
        worktree_path,
        GIT_TIMEOUT,
    ) else {
        return BTreeSet::new();
    };
    out.lines()
        .map(|l| {
            l.trim()
                .replace('\\', "/")
                .trim_start_matches("./")
                .to_string()
        })
        .filter(|l| !l.is_empty())
        .collect()
}

pub fn load_findings_report(artifact_dir: &Path) -> Result<FindingsReport, ContractFailure> {
    let findings_path = artifact_dir.join("findings.json");
    reject_if_symlink(&findings_path)
        .map_err(|e| ContractFailure::new("review_findings", e.to_string()))?;
    let raw = fs::read_to_string(&findings_path).map_err(|e| {
        ContractFailure::new(
            "review_findings",
            format!("failed to read {}: {e}", findings_path.display()),
        )
    })?;
    serde_json::from_str::<FindingsReport>(&raw)
        .map_err(|e| ContractFailure::new("review_findings", format!("invalid findings.json: {e}")))
}

pub fn validate_findings_report(
    artifact_dir: &Path,
    target_repo: &Path,
    worktree_path: &Path,
    base_commit: Option<&str>,
) -> Result<FindingsReport, ContractFailure> {
    let report = load_findings_report(artifact_dir)?;
    let changed = diff_files(worktree_path, base_commit);

    for finding in &report.findings {
        if finding.id.trim().is_empty()
            || finding.title.trim().is_empty()
            || finding.detail.trim().is_empty()
        {
            return Err(ContractFailure::new(
                "review_finding_fields",
                format!("finding '{}' is missing required fields", finding.id),
            ));
        }
        let norm_file = finding
            .file
            .trim()
            .replace('\\', "/")
            .trim_start_matches("./")
            .to_string();
        if !is_safe_relative_path(&norm_file) {
            return Err(ContractFailure::new(
                "review_finding_file",
                format!(
                    "finding '{}' has invalid file path '{}'",
                    finding.id, finding.file
                ),
            ));
        }
        let in_diff = changed.contains(&norm_file);
        let in_repo =
            target_repo.join(&norm_file).exists() || worktree_path.join(&norm_file).exists();
        if !in_diff && !in_repo {
            return Err(ContractFailure::new(
                "review_finding_file",
                format!(
                    "finding '{}' references file '{}' outside the diff and target repository",
                    finding.id, finding.file
                ),
            ));
        }
    }

    Ok(report)
}

pub fn record_review_round(
    artifact_dir: &Path,
    round: u32,
    report: &FindingsReport,
) -> Result<ReviewRoundRecord, ContractFailure> {
    let derived_verdict = derive_verdict(&report.findings);
    let verdict_overridden = report.verdict != derived_verdict;

    let rounds_path = artifact_dir.join("review-rounds.json");
    let mut rounds_report = if rounds_path.exists() {
        reject_if_symlink(&rounds_path)
            .map_err(|e| ContractFailure::new("review_rounds", e.to_string()))?;
        let raw = fs::read_to_string(&rounds_path)
            .map_err(|e| ContractFailure::new("review_rounds", e.to_string()))?;
        serde_json::from_str::<ReviewRoundsReport>(&raw)
            .map_err(|e| ContractFailure::new("review_rounds", e.to_string()))?
    } else {
        ReviewRoundsReport::default()
    };

    let current_ids: BTreeSet<String> = report.findings.iter().map(|f| f.id.clone()).collect();
    let (fixed_from_previous, remaining_from_previous) =
        if let Some(prev) = rounds_report.rounds.last() {
            let mut fixed = Vec::new();
            let mut remaining = Vec::new();
            for prev_finding in &prev.findings {
                if current_ids.contains(&prev_finding.id) {
                    remaining.push(prev_finding.id.clone());
                } else {
                    fixed.push(prev_finding.id.clone());
                }
            }
            (fixed, remaining)
        } else {
            (Vec::new(), Vec::new())
        };

    let record = ReviewRoundRecord {
        round,
        stated_verdict: report.verdict,
        derived_verdict,
        verdict_overridden,
        findings: report.findings.clone(),
        fixed_from_previous,
        remaining_from_previous,
    };

    let round_path = artifact_dir.join(format!("findings-round-{round}.json"));
    let round_bytes = serde_json::to_vec_pretty(&record)
        .map_err(|e| ContractFailure::new("review_rounds", e.to_string()))?;
    write_atomic(&round_path, &round_bytes)
        .map_err(|e| ContractFailure::new("review_rounds", e.to_string()))?;

    rounds_report.rounds.push(record.clone());
    let rounds_bytes = serde_json::to_vec_pretty(&rounds_report)
        .map_err(|e| ContractFailure::new("review_rounds", e.to_string()))?;
    write_atomic(&rounds_path, &rounds_bytes)
        .map_err(|e| ContractFailure::new("review_rounds", e.to_string()))?;

    Ok(record)
}

pub fn format_blocking_findings_summary(findings: &[Finding]) -> String {
    findings
        .iter()
        .filter(|f| {
            matches!(
                f.severity,
                FindingSeverity::Blocker | FindingSeverity::Major
            )
        })
        .map(|f| {
            let sev = match f.severity {
                FindingSeverity::Blocker => "blocker",
                FindingSeverity::Major => "major",
                FindingSeverity::Minor => "minor",
                FindingSeverity::Nit => "nit",
            };
            format!(
                "- [{sev}] {} ({}:{}): {} — {} (suggestion: {})",
                f.id, f.file, f.line, f.title, f.detail, f.suggestion
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub struct ReviewStageContract;

impl StageContract for ReviewStageContract {
    fn name(&self) -> &'static str {
        "review"
    }

    fn verify(&self, ctx: &StageContext<'_>) -> Result<(), ContractFailure> {
        verify_scope(&ScopeCheckInput {
            worktree_path: ctx.worktree_path,
            artifact_dir: ctx.artifact_dir,
            has_repo_write: false,
        })?;
        validate_findings_report(ctx.artifact_dir, ctx.target_repo, ctx.worktree_path, None)?;
        Ok(())
    }
}
