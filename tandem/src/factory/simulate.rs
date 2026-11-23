use std::collections::HashMap;

use serde::Serialize;

use super::broker::terms::AuthType;
use super::executor::{Executor, ReadyCli, StageJob, StageOutcome};
use super::model::{
    Actor, ActorKind, Approval, ApprovalGate, FactorySettings, RunState, Stage, StageState,
};
use super::workflow::scheduler::{RunFacts, SchedulerLimits, TriState, next};
use super::workflow::schema::Workflow;

#[derive(Debug, Clone, Default)]
pub struct SimulationScript {
    pub ui_changed: bool,
    pub deploy_requested: bool,
    pub approve_deploy: bool,
    pub fail_counts: HashMap<String, u32>,
    pub max_retries: Option<u32>,
}

pub struct FakeExecutor {
    fail_counts: HashMap<String, u32>,
}

impl FakeExecutor {
    pub fn new(fail_counts: HashMap<String, u32>) -> Self {
        Self { fail_counts }
    }
}

impl Executor for FakeExecutor {
    fn ready_clis(&self) -> Vec<ReadyCli> {
        crate::factory::model::KNOWN_PROVIDERS
            .iter()
            .map(|cli| ReadyCli::new(cli, Some(AuthType::ApiKey)))
            .collect()
    }

    fn run(&mut self, job: &StageJob) -> StageOutcome {
        let limit = self.fail_counts.get(&job.stage).copied().unwrap_or(0);
        if job.attempt <= limit {
            StageOutcome::Failed {
                reason: format!("attempt {} of {}", job.attempt, limit),
            }
        } else {
            StageOutcome::Succeeded
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SimulationStep {
    pub step: usize,
    pub stage: String,
    pub action: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SimulationOutcome {
    Finished { succeeded: usize, skipped: usize },
    Blocked { stage: String, reason: String },
    Stopped { stage: String, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Trace {
    pub workflow: String,
    pub version: u32,
    pub source: String,
    pub steps: Vec<SimulationStep>,
    pub outcome: SimulationOutcome,
    pub summary: String,
}

pub fn simulate(
    workflow: &Workflow,
    source: &str,
    settings: &FactorySettings,
    script: &SimulationScript,
) -> Trace {
    let mut effective_wf = workflow.clone();
    if let Some(retries) = script.max_retries {
        for s in &mut effective_wf.stages {
            s.budget.max_retries = Some(retries);
        }
    }

    let mut stages: Vec<Stage> = effective_wf
        .stages
        .iter()
        .map(|s| Stage {
            name: s.name.clone(),
            agent: s.agent.as_str().to_string(),
            state: StageState::Pending,
            attempts: 0,
            interrupted_attempts: 0,
            started: None,
            finished: None,
            artifacts: Vec::new(),
            rationale: None,
            retry_hypothesis: None,
        })
        .collect();

    let mut approvals = Vec::new();
    if script.approve_deploy {
        approvals.push(Approval {
            gate: ApprovalGate::ProtectedAction,
            actor: Actor {
                kind: ActorKind::User,
                id: "user".to_string(),
            },
            via: "cli".to_string(),
            at: 0.0,
            subject: Some("deploy".to_string()),
        });
    }

    let facts = RunFacts {
        ui_changed: if script.ui_changed {
            TriState::Yes
        } else {
            TriState::No
        },
        deploy_requested: script.deploy_requested,
        approvals,
    };

    let mut executor = FakeExecutor::new(script.fail_counts.clone());
    let mut steps = Vec::new();
    let mut step_num = 0usize;
    let max_iterations = effective_wf.stages.len() * 16 + 16;

    for _ in 0..max_iterations {
        let limits = SchedulerLimits::from_workflow_and_settings(&effective_wf, settings, &stages);
        let decision = next(&effective_wf, &stages, &facts, &limits);

        for skipped in &decision.skip {
            if let Some(st) = stages.iter_mut().find(|s| s.name == skipped.name) {
                st.state = StageState::Skipped;
            }
            step_num += 1;
            steps.push(SimulationStep {
                step: step_num,
                stage: skipped.name.clone(),
                action: "skipped".to_string(),
                reason: skipped.reason.clone(),
            });
        }

        if let Some(failed) = decision.failed.first() {
            if failed.retries_exhausted {
                let reason = format!(
                    "stage \"{}\" failed and its retries are used up",
                    failed.name
                );
                let summary = format!("stopped: {reason}");
                return Trace {
                    workflow: effective_wf.name,
                    version: effective_wf.version,
                    source: source.to_string(),
                    steps,
                    outcome: SimulationOutcome::Stopped {
                        stage: failed.name.clone(),
                        reason,
                    },
                    summary,
                };
            }
            if let Some(st) = stages.iter_mut().find(|s| s.name == failed.name) {
                st.state = StageState::Pending;
            }
            continue;
        }

        if decision.finished == Some(RunState::Succeeded) {
            let succeeded = stages
                .iter()
                .filter(|s| s.state == StageState::Succeeded)
                .count();
            let skipped = stages
                .iter()
                .filter(|s| s.state == StageState::Skipped)
                .count();
            let summary = format!("finished: {succeeded} stages succeeded, {skipped} skipped");
            return Trace {
                workflow: effective_wf.name,
                version: effective_wf.version,
                source: source.to_string(),
                steps,
                outcome: SimulationOutcome::Finished { succeeded, skipped },
                summary,
            };
        }

        if decision.start.is_empty() {
            if !decision.skip.is_empty() {
                continue;
            }
            if let Some(blocked) = decision.blocked.first() {
                let summary = format!("blocked: {} ({})", blocked.name, blocked.reason);
                return Trace {
                    workflow: effective_wf.name,
                    version: effective_wf.version,
                    source: source.to_string(),
                    steps,
                    outcome: SimulationOutcome::Blocked {
                        stage: blocked.name.clone(),
                        reason: blocked.reason.clone(),
                    },
                    summary,
                };
            }
            break;
        }

        for stage_name in &decision.start {
            let dep_reason = effective_wf
                .stages
                .iter()
                .find(|s| s.name == *stage_name)
                .map(|s| {
                    if s.depends_on.is_empty() {
                        "root stage".to_string()
                    } else {
                        format!("depends on {}", s.depends_on.join(", "))
                    }
                })
                .unwrap_or_default();

            step_num += 1;
            steps.push(SimulationStep {
                step: step_num,
                stage: stage_name.clone(),
                action: "start".to_string(),
                reason: dep_reason,
            });

            let attempt = {
                let st = stages
                    .iter_mut()
                    .find(|s| s.name == *stage_name)
                    .expect("stage exists");
                st.attempts += 1;
                st.state = StageState::Running;
                st.attempts
            };

            let outcome = executor.run(&StageJob {
                run: "sim".to_string(),
                stage: stage_name.clone(),
                attempt,
            });

            let st = stages
                .iter_mut()
                .find(|s| s.name == *stage_name)
                .expect("stage exists");

            match outcome {
                StageOutcome::Succeeded => {
                    st.state = StageState::Succeeded;
                    step_num += 1;
                    steps.push(SimulationStep {
                        step: step_num,
                        stage: stage_name.clone(),
                        action: "succeeded".to_string(),
                        reason: String::new(),
                    });
                }
                StageOutcome::Failed { reason } => {
                    st.state = StageState::Failed;
                    step_num += 1;
                    steps.push(SimulationStep {
                        step: step_num,
                        stage: stage_name.clone(),
                        action: "failed".to_string(),
                        reason,
                    });
                }
                StageOutcome::Interrupted => {
                    st.state = StageState::Pending;
                    st.interrupted_attempts += 1;
                    step_num += 1;
                    steps.push(SimulationStep {
                        step: step_num,
                        stage: stage_name.clone(),
                        action: "interrupted".to_string(),
                        reason: String::new(),
                    });
                }
                StageOutcome::Blocked { reason, .. } => {
                    st.state = StageState::Blocked;
                    step_num += 1;
                    steps.push(SimulationStep {
                        step: step_num,
                        stage: stage_name.clone(),
                        action: "blocked".to_string(),
                        reason,
                    });
                }
            }
        }
    }

    let summary = "stopped: simulation reached iteration limit".to_string();
    Trace {
        workflow: effective_wf.name,
        version: effective_wf.version,
        source: source.to_string(),
        steps,
        outcome: SimulationOutcome::Stopped {
            stage: String::new(),
            reason: "simulation reached iteration limit".to_string(),
        },
        summary,
    }
}

pub fn format_trace_text(trace: &Trace) -> String {
    let mut lines = Vec::with_capacity(trace.steps.len() + 2);
    lines.push(format!(
        "workflow {} v{} ({})",
        trace.workflow, trace.version, trace.source
    ));
    for s in &trace.steps {
        if s.reason.is_empty() {
            lines.push(format!("{:>2}  {:<14} {}", s.step, s.stage, s.action));
        } else {
            lines.push(format!(
                "{:>2}  {:<14} {:<10} {}",
                s.step, s.stage, s.action, s.reason
            ));
        }
    }
    lines.push(trace.summary.clone());
    lines.join("\n")
}
