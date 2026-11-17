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
    Interrupted,
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
            super::broker::result::JobResultStatus::Succeeded => {
                if let Some(contract) = super::stages::contract_for_stage(&job.stage) {
                    let run_opt = super::run::get_run(&store, &job.run).ok().flatten();
                    let worktree_path = run_opt
                        .as_ref()
                        .and_then(|r| r.worktree_path.as_deref())
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| self.workspace.clone());
                    let artifact_dir = store.run_dir(&job.run).join("artifacts");
                    let plan_snapshot_path = run_opt
                        .as_ref()
                        .map(|r| {
                            store
                                .root
                                .join("ideas")
                                .join(&r.idea)
                                .join("artifacts")
                                .join("plan-snapshot.md")
                        })
                        .unwrap_or_else(|| store.run_dir(&job.run).join("plan-snapshot.md"));
                    let ctx = super::stages::StageContext {
                        run_id: &job.run,
                        stage: &job.stage,
                        artifact_dir: &artifact_dir,
                        target_repo: &worktree_path,
                        worktree_path: &worktree_path,
                        plan_snapshot_path: &plan_snapshot_path,
                        result: &parsed,
                    };
                    if let Err(failure) = contract.verify(&ctx) {
                        return StageOutcome::Failed {
                            reason: failure.to_string(),
                        };
                    }
                }
                StageOutcome::Succeeded
            }
            super::broker::result::JobResultStatus::Failed
            | super::broker::result::JobResultStatus::NeedsInput => StageOutcome::Failed {
                reason: parsed
                    .rationale
                    .unwrap_or_else(|| "no structured result".to_string()),
            },
        }
    }
}
