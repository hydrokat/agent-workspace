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

pub struct BrokerExecutor {
    pub workspace: std::path::PathBuf,
}

impl Executor for BrokerExecutor {
    fn run(&mut self, job: &StageJob) -> StageOutcome {
        let store = super::store::FactoryStore::new(&self.workspace);
        let log_path = store
            .root
            .join("runs")
            .join(&job.run)
            .join("jobs")
            .join(format!("{}.{}.{}", job.run, job.stage, job.attempt))
            .join("result.json");
        let parsed = super::broker::result::parse_and_validate_result(&log_path, job.attempt > 1);
        match parsed.status {
            super::broker::result::JobResultStatus::Succeeded => StageOutcome::Succeeded,
            super::broker::result::JobResultStatus::Failed
            | super::broker::result::JobResultStatus::NeedsInput => StageOutcome::Failed {
                reason: parsed
                    .rationale
                    .unwrap_or_else(|| "no structured result".to_string()),
            },
        }
    }
}
