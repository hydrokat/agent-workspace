use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::factory::broker::checks::{CheckRunError, run_deploy_command};
use crate::factory::checks::{ConfirmedDeploy, require_confirmed_deploy};
use crate::factory::model::{FactorySettings, Run};
use crate::factory::protected::{ProtectedApproval, find_matching_approval};
use crate::factory::redact::{RedactContext, redact};
use crate::factory::stages::{ContractFailure, StageContext, StageContract};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeployBlockReason {
    DeployNotRequested,
    MissingPullRequest,
    NoDeployCommandConfirmed(String),
    MissingApproval,
}

impl DeployBlockReason {
    pub fn message(&self) -> String {
        match self {
            Self::DeployNotRequested => "deploy was not requested at mark-ready".to_string(),
            Self::MissingPullRequest => "a pull request is required before deploy".to_string(),
            Self::NoDeployCommandConfirmed(target) => {
                format!("no deploy command is confirmed for {target}")
            }
            Self::MissingApproval => "needs your approval for production_deploy".to_string(),
        }
    }

    pub fn action_commands(&self, run_id: &str, target: &str) -> Vec<String> {
        match self {
            Self::DeployNotRequested => vec![],
            Self::MissingPullRequest => vec![format!("tandem factory run prs {run_id} --refresh")],
            Self::NoDeployCommandConfirmed(_) => vec![format!(
                "tandem factory checks set --target {target} --deploy '[\"<cmd>\"]' --confirm {target}"
            )],
            Self::MissingApproval => vec![format!(
                "tandem factory run approve {run_id} --action production_deploy --confirm {run_id}"
            )],
        }
    }
}

pub fn check_deploy_preconditions(
    run: &Run,
    settings: &FactorySettings,
    approvals: &[ProtectedApproval],
    now: f64,
) -> Result<ConfirmedDeploy, DeployBlockReason> {
    if !run.deploy_requested {
        return Err(DeployBlockReason::DeployNotRequested);
    }
    if run.prs.is_empty() {
        return Err(DeployBlockReason::MissingPullRequest);
    }
    let target_name = run.target.as_deref().unwrap_or("workspace");
    let confirmed = require_confirmed_deploy(settings, target_name)
        .map_err(|_| DeployBlockReason::NoDeployCommandConfirmed(target_name.to_string()))?;
    if find_matching_approval(approvals, &run.id, "deploy", &run.workflow_sha256, now).is_none() {
        return Err(DeployBlockReason::MissingApproval);
    }
    Ok(confirmed)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeployReport {
    pub command: Vec<String>,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub timed_out: bool,
    pub log_cap_exceeded: bool,
    pub passed: bool,
    pub summary: String,
}

pub fn run_deploy_stage(
    confirmed: &ConfirmedDeploy,
    worktree: &Path,
    artifact_dir: &Path,
    attempt: u32,
) -> Result<DeployReport, CheckRunError> {
    let outcome = run_deploy_command(confirmed, worktree, artifact_dir, attempt)?;
    let passed = outcome.passed();
    let raw_summary = outcome
        .summary
        .clone()
        .or_else(|| {
            std::fs::read_to_string(&outcome.log_path)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| {
            format!(
                "deploy command {} exited with {:?}",
                outcome.argv.join(" "),
                outcome.exit_code
            )
        });
    let ctx = RedactContext::from_env();
    let summary = redact(&raw_summary, &ctx).into_string();

    Ok(DeployReport {
        command: outcome.argv,
        exit_code: outcome.exit_code,
        duration_ms: outcome.duration_ms,
        timed_out: outcome.timed_out,
        log_cap_exceeded: outcome.log_cap_exceeded,
        passed,
        summary,
    })
}

pub struct DeployStageContract;

impl StageContract for DeployStageContract {
    fn name(&self) -> &'static str {
        "deploy"
    }

    fn verify(&self, ctx: &StageContext<'_>) -> Result<(), ContractFailure> {
        if ctx.has_repo_write {
            return Err(ContractFailure::new(
                "deploy_read_only",
                "deploy stage must not have repo_write permission",
            ));
        }
        Ok(())
    }
}
