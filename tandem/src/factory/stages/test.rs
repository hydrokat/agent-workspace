use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{ContractFailure, StageContext, StageContract};
use crate::factory::broker::checks::{CheckOutcome, CheckRunError, run_checks};
use crate::factory::checks::{CheckKind, ConfirmedChecks};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestStageReport {
    pub outcomes: Vec<CheckOutcome>,
    pub all_passed: bool,
    pub failures_summary: String,
}

pub fn run_test_stage_checks(
    confirmed: &ConfirmedChecks,
    worktree: &Path,
    artifact_dir: &Path,
    attempt: u32,
) -> Result<TestStageReport, CheckRunError> {
    let mut outcomes = run_checks(confirmed, CheckKind::Test, worktree, artifact_dir, attempt)?;
    outcomes.extend(run_checks(
        confirmed,
        CheckKind::Lint,
        worktree,
        artifact_dir,
        attempt,
    )?);

    let all_passed = outcomes.iter().all(CheckOutcome::passed);
    let failures_summary = format_failures_summary(&outcomes);

    Ok(TestStageReport {
        outcomes,
        all_passed,
        failures_summary,
    })
}

pub fn format_failures_summary(outcomes: &[CheckOutcome]) -> String {
    let mut lines = Vec::new();
    for outcome in outcomes {
        if outcome.passed() {
            continue;
        }
        let kind = match outcome.kind {
            CheckKind::Test => "test",
            CheckKind::Lint => "lint",
        };
        let status = if outcome.timed_out {
            "timed out".to_string()
        } else if outcome.log_cap_exceeded {
            "log cap exceeded".to_string()
        } else if let Some(ref err) = outcome.error {
            format!("spawn error: {err}")
        } else {
            format!(
                "exit {}",
                outcome
                    .exit_code
                    .map_or_else(|| "signal".to_string(), |c| c.to_string())
            )
        };
        lines.push(format!(
            "- [{kind}] `{}` ({status}) — log: {}",
            outcome.argv.join(" "),
            outcome.log_path.display()
        ));
        if let Some(ref summary) = outcome.summary {
            lines.push(summary.clone());
        }
    }
    lines.join("\n")
}

pub struct TestStageContract;

impl StageContract for TestStageContract {
    fn name(&self) -> &'static str {
        "test"
    }

    fn verify(&self, ctx: &StageContext<'_>) -> Result<(), ContractFailure> {
        let checks_dir = ctx.artifact_dir.join("checks");
        if !checks_dir.is_dir() {
            return Err(ContractFailure::new(
                "test_checks",
                "no check logs recorded for test stage",
            ));
        }
        Ok(())
    }
}
