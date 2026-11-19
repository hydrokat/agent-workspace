use std::collections::HashMap;

use super::schema::{StageCondition, StageDef, StageRequirement, ToolClass, Workflow};
use crate::factory::model::{Approval, ApprovalGate, FactorySettings, RunState, Stage, StageState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TriState {
    Yes,
    No,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RunFacts {
    pub ui_changed: TriState,
    pub deploy_requested: bool,
    pub approvals: Vec<Approval>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerLimits {
    pub effective_max_parallel: usize,
    pub running_count: usize,
    pub workspace_max_parallel: usize,
    pub workspace_running_count: usize,
}

impl SchedulerLimits {
    pub fn from_workflow_and_settings(
        workflow: &Workflow,
        settings: &FactorySettings,
        stages: &[Stage],
    ) -> Self {
        let effective_policy = crate::factory::settings::effective(&workflow.policy, settings);
        let effective_max_parallel = effective_policy.max_parallel_agents as usize;
        let running_count = stages
            .iter()
            .filter(|s| s.state == StageState::Running)
            .count();
        Self {
            effective_max_parallel,
            running_count,
            workspace_max_parallel: settings.max_parallel_agents,
            workspace_running_count: running_count,
        }
    }

    pub fn with_workspace_running(mut self, workspace_running_count: usize) -> Self {
        self.workspace_running_count = workspace_running_count;
        self
    }

    pub fn available_slots(&self) -> usize {
        let run_slots = self
            .effective_max_parallel
            .saturating_sub(self.running_count);
        let workspace_slots = self
            .workspace_max_parallel
            .saturating_sub(self.workspace_running_count);
        run_slots.min(workspace_slots)
    }
}

pub fn stage_has_writer_tool(stage_def: &StageDef) -> bool {
    stage_def
        .tools
        .iter()
        .any(|tool| matches!(tool.class(), ToolClass::Write | ToolClass::Execute))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedStage {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedStage {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedStage {
    pub name: String,
    pub attempts: u32,
    pub max_retries: u32,
    pub retries_exhausted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Decision {
    pub start: Vec<String>,
    pub skip: Vec<SkippedStage>,
    pub blocked: Vec<BlockedStage>,
    pub waiting: Vec<String>,
    pub failed: Vec<FailedStage>,
    pub finished: Option<RunState>,
}

enum ConditionEval {
    Met,
    False(String),
    Unknown,
}

fn dependencies_satisfied(stage_def: &StageDef, state_by_name: &HashMap<&str, StageState>) -> bool {
    stage_def.depends_on.iter().all(|dep| {
        matches!(
            state_by_name.get(dep.as_str()).copied(),
            Some(StageState::Succeeded | StageState::Skipped)
        )
    })
}

fn evaluate_condition(condition: Option<StageCondition>, facts: &RunFacts) -> ConditionEval {
    match condition {
        None => ConditionEval::Met,
        Some(StageCondition::UiChanged) => match facts.ui_changed {
            TriState::Yes => ConditionEval::Met,
            TriState::No => ConditionEval::False("condition ui_changed is false".to_string()),
            TriState::Unknown => ConditionEval::Unknown,
        },
        Some(StageCondition::DeployRequested) => {
            if facts.deploy_requested {
                ConditionEval::Met
            } else {
                ConditionEval::False("condition deploy_requested is false".to_string())
            }
        }
    }
}

fn has_required_approvals(stage_def: &StageDef, approvals: &[Approval]) -> bool {
    if !stage_def
        .requires
        .contains(&StageRequirement::HumanApproval)
    {
        return true;
    }
    approvals.iter().any(|approval| {
        approval.gate == ApprovalGate::ProtectedAction
            && approval
                .subject
                .as_deref()
                .is_none_or(|subject| subject == stage_def.name)
    })
}

pub fn next(
    workflow: &Workflow,
    stages: &[Stage],
    facts: &RunFacts,
    limits: &SchedulerLimits,
) -> Decision {
    let stage_by_name: HashMap<&str, &Stage> =
        stages.iter().map(|s| (s.name.as_str(), s)).collect();
    let state_by_name: HashMap<&str, StageState> =
        stages.iter().map(|s| (s.name.as_str(), s.state)).collect();

    let slots = limits.available_slots();
    let mut writer_busy = workflow.stages.iter().any(|stage_def| {
        state_by_name.get(stage_def.name.as_str()).copied() == Some(StageState::Running)
            && stage_has_writer_tool(stage_def)
    });
    let mut decision = Decision::default();

    for stage_def in &workflow.stages {
        let state = state_by_name
            .get(stage_def.name.as_str())
            .copied()
            .unwrap_or(StageState::Pending);
        let attempts = stage_by_name
            .get(stage_def.name.as_str())
            .map_or(0, |s| s.attempts);
        let interrupted_attempts = stage_by_name
            .get(stage_def.name.as_str())
            .map_or(0, |s| s.interrupted_attempts);

        match state {
            StageState::Succeeded | StageState::Skipped | StageState::Running => {}
            StageState::Failed => {
                let max_retries = stage_def.budget.max_retries.unwrap_or(0);
                let retries_used = attempts.saturating_sub(1 + interrupted_attempts);
                decision.failed.push(FailedStage {
                    name: stage_def.name.clone(),
                    attempts,
                    max_retries,
                    retries_exhausted: retries_used >= max_retries,
                });
            }
            StageState::Pending | StageState::Blocked => {
                if !dependencies_satisfied(stage_def, &state_by_name) {
                    continue;
                }

                match evaluate_condition(stage_def.when, facts) {
                    ConditionEval::False(reason) => {
                        decision.skip.push(SkippedStage {
                            name: stage_def.name.clone(),
                            reason,
                        });
                    }
                    ConditionEval::Unknown => {
                        decision.waiting.push(stage_def.name.clone());
                    }
                    ConditionEval::Met => {
                        if !has_required_approvals(stage_def, &facts.approvals) {
                            let action = crate::factory::protected::action_for_stage(
                                stage_def,
                                &workflow.policy.require_human_approval,
                            );
                            decision.blocked.push(BlockedStage {
                                name: stage_def.name.clone(),
                                reason: format!(
                                    "needs your approval for {action} (tandem factory run approve <run> --action {action} --confirm <run>)"
                                ),
                            });
                        } else if decision.start.len() < slots {
                            let is_writer = stage_has_writer_tool(stage_def);
                            if is_writer && writer_busy {
                                continue;
                            }
                            if is_writer {
                                writer_busy = true;
                            }
                            decision.start.push(stage_def.name.clone());
                        }
                    }
                }
            }
        }
    }

    let all_done = workflow.stages.iter().all(|s| {
        matches!(
            state_by_name.get(s.name.as_str()).copied(),
            Some(StageState::Succeeded | StageState::Skipped)
        )
    });

    if all_done {
        decision.finished = Some(RunState::Succeeded);
    } else if decision.failed.iter().any(|f| f.retries_exhausted)
        && limits.running_count == 0
        && decision.start.is_empty()
    {
        decision.finished = Some(RunState::Failed);
    }

    decision
}

pub fn next_stages(
    workflow: &Workflow,
    stages: &[Stage],
    facts: &RunFacts,
    limits: &SchedulerLimits,
) -> Decision {
    next(workflow, stages, facts, limits)
}
