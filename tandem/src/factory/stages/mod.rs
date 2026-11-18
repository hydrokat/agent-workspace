use std::path::Path;
use std::time::Duration;

use crate::factory::broker::git::{AllowedGitSubcommand, run_git};
use crate::factory::broker::result::JobResult;
use crate::factory::checks::ConfirmedChecks;

pub mod create_tests;
pub mod demo;
pub mod deploy;
pub mod develop;
pub mod plan;
pub mod review;
pub mod scope;
pub mod test;

const GIT_COMMIT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct StageContext<'a> {
    pub run_id: &'a str,
    pub idea_id: &'a str,
    pub stage: &'a str,
    pub attempt: u32,
    pub has_repo_write: bool,
    pub artifact_dir: &'a Path,
    pub target_repo: &'a Path,
    pub worktree_path: &'a Path,
    pub plan_snapshot_path: &'a Path,
    pub confirmed_checks: Option<&'a ConfirmedChecks>,
    pub result: &'a JobResult,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{check}: {detail}")]
pub struct ContractFailure {
    pub check: &'static str,
    pub detail: String,
}

impl ContractFailure {
    pub fn new(check: &'static str, detail: impl Into<String>) -> Self {
        Self {
            check,
            detail: detail.into(),
        }
    }
}

pub trait StageContract {
    fn name(&self) -> &'static str;
    fn verify(&self, ctx: &StageContext<'_>) -> Result<(), ContractFailure>;
}

pub fn contract_for_stage(stage: &str) -> Option<Box<dyn StageContract>> {
    match stage {
        "plan" => Some(Box::new(plan::PlanStageContract)),
        "create-tests" => Some(Box::new(create_tests::CreateTestsStageContract)),
        "develop" => Some(Box::new(develop::DevelopStageContract)),
        "test" => Some(Box::new(test::TestStageContract)),
        "review" => Some(Box::new(review::ReviewStageContract)),
        "demo" => Some(Box::new(demo::DemoStageContract)),
        "deploy" => Some(Box::new(deploy::DeployStageContract)),
        _ => None,
    }
}

pub fn commit_stage_changes(
    worktree_path: &Path,
    idea_id: &str,
    stage: &str,
    files: &[String],
) -> Result<(), ContractFailure> {
    if files.is_empty() {
        return Ok(());
    }
    let mut add_args = Vec::with_capacity(files.len() + 1);
    add_args.push("--");
    for f in files {
        add_args.push(f.as_str());
    }
    run_git(
        AllowedGitSubcommand::Add,
        &add_args,
        worktree_path,
        GIT_COMMIT_TIMEOUT,
    )
    .map_err(|e| ContractFailure::new("stage_commit_add", e.to_string()))?;

    let prefix = if stage == "create-tests" {
        "test"
    } else {
        "feat"
    };
    let message = format!("{prefix}({idea_id}): {stage} stage changes");
    run_git(
        AllowedGitSubcommand::Commit,
        &["-m", &message],
        worktree_path,
        GIT_COMMIT_TIMEOUT,
    )
    .map_err(|e| ContractFailure::new("stage_commit", e.to_string()))?;

    Ok(())
}
