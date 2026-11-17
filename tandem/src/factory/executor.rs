use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageJob {
    pub run: String,
    pub stage: String,
    pub attempt: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum StageOutcome {
    Succeeded,
    Failed { reason: String },
}

pub trait Executor {
    fn run(&mut self, job: &StageJob) -> StageOutcome;
}
