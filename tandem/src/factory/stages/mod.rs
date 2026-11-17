use std::path::Path;

use crate::factory::broker::result::JobResult;

pub mod plan;

#[derive(Debug, Clone)]
pub struct StageContext<'a> {
    pub run_id: &'a str,
    pub stage: &'a str,
    pub artifact_dir: &'a Path,
    pub target_repo: &'a Path,
    pub worktree_path: &'a Path,
    pub plan_snapshot_path: &'a Path,
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
        _ => None,
    }
}
