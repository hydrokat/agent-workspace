use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::create_tests::{CREATE_TESTS_EVIDENCE_FILE, CreateTestsEvidence, is_test_file};
use super::scope::{ScopeCheckInput, list_deleted_files, verify_scope};
use super::{ContractFailure, StageContext, StageContract};
use crate::factory::events::reject_if_symlink;
use crate::factory::store::write_atomic;

pub const DEVELOP_EVIDENCE_FILE: &str = "develop-evidence.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevelopEvidence {
    pub files_changed: Vec<String>,
    pub modified_create_tests_files: Vec<String>,
    pub rationale: Option<String>,
}

pub struct DevelopStageContract;

impl StageContract for DevelopStageContract {
    fn name(&self) -> &'static str {
        "develop"
    }

    fn verify(&self, ctx: &StageContext<'_>) -> Result<(), ContractFailure> {
        let scope = verify_scope(&ScopeCheckInput {
            worktree_path: ctx.worktree_path,
            artifact_dir: ctx.artifact_dir,
            has_repo_write: ctx.has_repo_write,
        })?;

        if scope.changed_files.is_empty() {
            return Err(ContractFailure::new(
                "develop_non_empty_diff",
                "develop stage produced an empty diff",
            ));
        }

        let deleted = list_deleted_files(ctx.worktree_path)?;
        let create_tests_files = load_create_tests_files(ctx.artifact_dir)?;

        let deleted_tests: Vec<String> = create_tests_files
            .iter()
            .filter(|f| deleted.contains(*f) || !ctx.worktree_path.join(f).exists())
            .cloned()
            .collect();
        if !deleted_tests.is_empty() {
            return Err(ContractFailure::new(
                "develop_tests_preserved",
                format!("create-tests file(s) deleted: {}", deleted_tests.join(", ")),
            ));
        }

        let deleted_other_tests: Vec<String> = deleted
            .iter()
            .filter(|f| is_test_file(f))
            .cloned()
            .collect();
        if !deleted_other_tests.is_empty() {
            return Err(ContractFailure::new(
                "develop_tests_preserved",
                format!(
                    "test file(s) deleted during develop: {}",
                    deleted_other_tests.join(", ")
                ),
            ));
        }

        let modified_create_tests: Vec<String> = scope
            .changed_files
            .iter()
            .filter(|f| create_tests_files.contains(*f))
            .cloned()
            .collect();

        let rationale = ctx
            .result
            .rationale
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        if !modified_create_tests.is_empty() && rationale.is_none() {
            return Err(ContractFailure::new(
                "develop_tests_preserved",
                format!(
                    "create-tests file(s) modified without stated rationale: {}",
                    modified_create_tests.join(", ")
                ),
            ));
        }

        write_evidence(
            ctx.artifact_dir,
            &DevelopEvidence {
                files_changed: scope.changed_files,
                modified_create_tests_files: modified_create_tests,
                rationale,
            },
        )?;

        Ok(())
    }
}

fn load_create_tests_files(artifact_dir: &Path) -> Result<BTreeSet<String>, ContractFailure> {
    let path = artifact_dir.join(CREATE_TESTS_EVIDENCE_FILE);
    if !path.exists() {
        return Ok(BTreeSet::new());
    }
    reject_if_symlink(&path)
        .map_err(|e| ContractFailure::new("develop_tests_preserved", e.to_string()))?;
    let raw = fs::read_to_string(&path)
        .map_err(|e| ContractFailure::new("develop_tests_preserved", e.to_string()))?;
    let evidence: CreateTestsEvidence = serde_json::from_str(&raw)
        .map_err(|e| ContractFailure::new("develop_tests_preserved", e.to_string()))?;
    Ok(evidence.test_files.into_iter().collect())
}

fn write_evidence(artifact_dir: &Path, evidence: &DevelopEvidence) -> Result<(), ContractFailure> {
    let path = artifact_dir.join(DEVELOP_EVIDENCE_FILE);
    reject_if_symlink(&path)
        .map_err(|e| ContractFailure::new("develop_evidence", e.to_string()))?;
    let bytes = serde_json::to_vec_pretty(evidence)
        .map_err(|e| ContractFailure::new("develop_evidence", e.to_string()))?;
    write_atomic(&path, &bytes)
        .map_err(|e| ContractFailure::new("develop_evidence", e.to_string()))?;
    Ok(())
}
