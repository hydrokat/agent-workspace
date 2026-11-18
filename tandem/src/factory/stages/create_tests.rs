use std::path::Path;

use serde::{Deserialize, Serialize};

use super::scope::{ScopeCheckInput, verify_scope};
use super::{ContractFailure, StageContext, StageContract};
use crate::factory::broker::checks::{CheckOutcome, run_checks};
use crate::factory::checks::CheckKind;
use crate::factory::events::reject_if_symlink;
use crate::factory::store::write_atomic;

pub const CREATE_TESTS_EVIDENCE_FILE: &str = "create-tests-evidence.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateTestsEvidence {
    pub test_files: Vec<String>,
    pub no_behaviour_change: bool,
    pub rationale: Option<String>,
}

pub fn is_test_file(path: &str) -> bool {
    let norm = path.replace('\\', "/");
    let lower = norm.to_ascii_lowercase();
    let segments: Vec<&str> = lower.split('/').collect();
    if segments
        .iter()
        .any(|seg| matches!(*seg, "test" | "tests" | "__tests__" | "e2e"))
    {
        return true;
    }

    let file_name = Path::new(&lower)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    let stem = Path::new(&lower)
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    file_name.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with("_spec")
        || file_name.contains(".test.")
        || file_name.contains(".spec.")
        || file_name.ends_with("test.java")
        || file_name.ends_with("tests.cs")
        || file_name.ends_with("test.php")
}

pub struct CreateTestsStageContract;

impl StageContract for CreateTestsStageContract {
    fn name(&self) -> &'static str {
        "create-tests"
    }

    fn verify(&self, ctx: &StageContext<'_>) -> Result<(), ContractFailure> {
        let scope = verify_scope(&ScopeCheckInput {
            worktree_path: ctx.worktree_path,
            artifact_dir: ctx.artifact_dir,
            has_repo_write: ctx.has_repo_write,
        })?;

        let test_files: Vec<String> = scope
            .changed_files
            .into_iter()
            .filter(|p| is_test_file(p))
            .collect();

        if test_files.is_empty() && !ctx.result.no_behaviour_change {
            return Err(ContractFailure::new(
                "create_tests_files",
                "create-tests must add or change at least one test file",
            ));
        }

        if ctx.result.no_behaviour_change {
            let rationale = ctx
                .result
                .rationale
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let Some(rationale_text) = rationale else {
                return Err(ContractFailure::new(
                    "create_tests_rationale",
                    "no_behaviour_change requires a non-empty rationale",
                ));
            };
            write_evidence(
                ctx.artifact_dir,
                &CreateTestsEvidence {
                    test_files,
                    no_behaviour_change: true,
                    rationale: Some(rationale_text),
                },
            )?;
            return Ok(());
        }

        let Some(confirmed) = ctx.confirmed_checks else {
            return Err(ContractFailure::new(
                "create_tests_checks",
                "confirmed test checks are required to verify tests fail before implementation",
            ));
        };
        if confirmed.test().is_empty() {
            return Err(ContractFailure::new(
                "create_tests_checks",
                "confirmed test commands must not be empty",
            ));
        }

        let outcomes = run_checks(
            confirmed,
            CheckKind::Test,
            ctx.worktree_path,
            ctx.artifact_dir,
            ctx.attempt,
        )
        .map_err(|e| ContractFailure::new("create_tests_checks", e.to_string()))?;

        if outcomes.iter().all(CheckOutcome::passed) {
            return Err(ContractFailure::new(
                "create_tests_must_fail",
                "test commands passed before implementation; tests must fail where baseline passed unless no_behaviour_change is stated with rationale",
            ));
        }

        write_evidence(
            ctx.artifact_dir,
            &CreateTestsEvidence {
                test_files,
                no_behaviour_change: false,
                rationale: ctx.result.rationale.clone(),
            },
        )?;

        Ok(())
    }
}

fn write_evidence(
    artifact_dir: &Path,
    evidence: &CreateTestsEvidence,
) -> Result<(), ContractFailure> {
    let path = artifact_dir.join(CREATE_TESTS_EVIDENCE_FILE);
    reject_if_symlink(&path)
        .map_err(|e| ContractFailure::new("create_tests_evidence", e.to_string()))?;
    let bytes = serde_json::to_vec_pretty(evidence)
        .map_err(|e| ContractFailure::new("create_tests_evidence", e.to_string()))?;
    write_atomic(&path, &bytes)
        .map_err(|e| ContractFailure::new("create_tests_evidence", e.to_string()))?;
    Ok(())
}
