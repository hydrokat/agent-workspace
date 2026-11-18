use serde::{Deserialize, Serialize};

use super::evidence::EvidenceItem;
use super::model::{
    Actor, ActorKind, ApprovalGate, ArtifactRef, FeasibilityVerdict, Idea, IdeaState,
    OrchestratorRef, Plan, PrLink, Risk, Run, RunState, Stage, StageState, validate_owner,
    validate_text, validate_title,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Create {
        id: String,
        title: String,
        text: String,
        owner: String,
    },
    StartDiscovery,
    SelectDirection,
    MarkReady,
    MarkReadyWithSnapshot {
        snapshot: ArtifactRef,
        approved_by: String,
        via: String,
    },
    RevokeReady,
    CreateRun {
        run: String,
    },
    WithdrawRun {
        run: String,
    },
    CancelRun {
        run: String,
    },
    StartRun {
        run: String,
    },
    StagesComplete {
        run: String,
    },
    ChangesRequested {
        run: String,
    },
    Deliver {
        run: String,
        prs: Vec<PrLink>,
    },
    Cancel {
        reason: String,
    },
    BindOrchestrator {
        cli: String,
        model: String,
        conversation: Option<String>,
    },
    Describe {
        title: String,
        text: String,
    },
    StoreArtifact {
        kind: String,
        artifact: ArtifactRef,
        verdict: Option<FeasibilityVerdict>,
        risk: Option<Risk>,
        workflow: Option<String>,
        missing_sections: Vec<String>,
    },
    AddAssumption {
        text: String,
    },
}

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Self::Create { .. } => "Create",
            Self::StartDiscovery => "StartDiscovery",
            Self::SelectDirection => "SelectDirection",
            Self::MarkReady => "MarkReady",
            Self::MarkReadyWithSnapshot { .. } => "MarkReadyWithSnapshot",
            Self::RevokeReady => "RevokeReady",
            Self::CreateRun { .. } => "CreateRun",
            Self::WithdrawRun { .. } => "WithdrawRun",
            Self::CancelRun { .. } => "CancelRun",
            Self::StartRun { .. } => "StartRun",
            Self::StagesComplete { .. } => "StagesComplete",
            Self::ChangesRequested { .. } => "ChangesRequested",
            Self::Deliver { .. } => "Deliver",
            Self::Cancel { .. } => "Cancel",
            Self::BindOrchestrator { .. } => "BindOrchestrator",
            Self::Describe { .. } => "Describe",
            Self::StoreArtifact { .. } => "StoreArtifact",
            Self::AddAssumption { .. } => "AddAssumption",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    pub has_feasibility: bool,
    pub feasibility_blocks: bool,
    pub has_plan: bool,
    pub open_blocking_questions: usize,
    pub has_active_run: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum EventData {
    #[serde(rename = "idea.created")]
    IdeaCreated {
        id: String,
        title: String,
        text: String,
        owner: String,
    },
    #[serde(rename = "idea.discovery_started")]
    IdeaDiscoveryStarted {},
    #[serde(rename = "idea.direction_selected")]
    IdeaDirectionSelected {},
    #[serde(rename = "idea.ready")]
    IdeaReady {},
    #[serde(rename = "idea.plan_snapshot_frozen")]
    IdeaPlanSnapshotFrozen {
        plan_snapshot: ArtifactRef,
        approved_by: String,
        via: String,
    },
    #[serde(rename = "idea.ready_revoked")]
    IdeaReadyRevoked {},
    #[serde(rename = "run.created")]
    RunCreated {
        run: String,
        #[serde(default)]
        workflow: String,
        #[serde(default)]
        version: String,
        #[serde(default)]
        sha256: String,
        #[serde(default)]
        source: String,
        #[serde(default)]
        plan_sha256: String,
        #[serde(default)]
        deploy_requested: bool,
        #[serde(default)]
        stages: Vec<Stage>,
    },
    #[serde(rename = "run.withdrawn")]
    RunWithdrawn { run: String },
    #[serde(rename = "run.started")]
    RunStarted { run: String },
    #[serde(rename = "stage.started")]
    StageStarted {
        run: String,
        stage: String,
        attempt: u32,
    },
    #[serde(rename = "stage.succeeded")]
    StageSucceeded { run: String, stage: String },
    #[serde(rename = "stage.failed")]
    StageFailed {
        run: String,
        stage: String,
        reason: String,
    },
    #[serde(rename = "stage.skipped")]
    StageSkipped {
        run: String,
        stage: String,
        reason: String,
    },
    #[serde(rename = "stage.blocked")]
    StageBlocked {
        run: String,
        stage: String,
        reason: String,
    },
    #[serde(rename = "run.stages_complete")]
    RunStagesComplete { run: String },
    #[serde(rename = "run.changes_requested")]
    RunChangesRequested { run: String },
    #[serde(rename = "idea.delivered")]
    IdeaDelivered { run: String, prs: Vec<PrLink> },
    #[serde(rename = "idea.orchestrator_bound")]
    IdeaOrchestratorBound {
        cli: String,
        model: String,
        #[serde(default)]
        conversation: Option<String>,
    },
    #[serde(rename = "idea.described")]
    IdeaDescribed { title: String, text: String },
    #[serde(rename = "artifact.stored")]
    ArtifactStored {
        kind: String,
        artifact: ArtifactRef,
        #[serde(default)]
        verdict: Option<FeasibilityVerdict>,
        #[serde(default)]
        risk: Option<Risk>,
        #[serde(default)]
        workflow: Option<String>,
        #[serde(default)]
        missing_sections: Vec<String>,
    },
    #[serde(rename = "idea.assumption_added")]
    IdeaAssumptionAdded { text: String },
    #[serde(rename = "idea.cancelled")]
    IdeaCancelled { reason: String },
    #[serde(rename = "provider.consented")]
    ProviderConsented {
        provider: String,
        auth_type: String,
        verified_on: String,
        terms_record: String,
        consented_by: String,
    },
    #[serde(rename = "provider.revoked")]
    ProviderRevoked { provider: String },
    #[serde(rename = "workspace.prepared")]
    WorkspacePrepared {
        run: String,
        target: String,
        branch: String,
        base_commit: String,
        worktree_path: String,
    },
    #[serde(rename = "run.target_selected")]
    RunTargetSelected { run: String, target: String },
    #[serde(rename = "stage.model_upgraded")]
    StageModelUpgraded {
        run: String,
        stage: String,
        from: String,
        to: String,
    },
    #[serde(rename = "run.paused")]
    RunPaused {
        run: String,
        reason: String,
        #[serde(default)]
        commands: Vec<String>,
    },
    #[serde(rename = "run.resumed")]
    RunResumed { run: String },
    #[serde(rename = "stage.retry_requested")]
    StageRetryRequested {
        run: String,
        stage: String,
        hypothesis: String,
    },
    #[serde(rename = "run.cancelled")]
    RunCancelled { run: String },
    #[serde(rename = "stage.interrupted")]
    StageInterrupted { run: String, stage: String },
    #[serde(rename = "stage.reopened")]
    StageReopened {
        run: String,
        stage: String,
        reason: String,
    },
    #[serde(rename = "run.baseline")]
    RunBaseline {
        run: String,
        passed: bool,
        #[serde(default)]
        failed_checks: Vec<String>,
    },
    #[serde(rename = "evidence.added")]
    EvidenceAdded { run: String, item: EvidenceItem },
    #[serde(rename = "pr.created")]
    PrCreated { run: String, pr: PrLink },
    #[serde(rename = "pr.status_refreshed")]
    PrStatusRefreshed { run: String, pr: PrLink },
}

fn actor_label(actor: ActorKind) -> &'static str {
    match actor {
        ActorKind::User => "user",
        ActorKind::Orchestrator => "orchestrator",
        ActorKind::System => "system",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    #[error("idea not found")]
    NotFound,
    #[error("idea already exists")]
    AlreadyExists,
    #[error("cannot run {command} when idea is in state {}", from.label())]
    IllegalTransition {
        from: IdeaState,
        command: &'static str,
    },
    #[error("{} is not allowed to run {command}", actor_label(*.actor))]
    ActorNotAllowed {
        command: &'static str,
        actor: ActorKind,
    },
    #[error("gate {gate:?} not met: {reason}")]
    GateNotMet { gate: ApprovalGate, reason: String },
    #[error("{0}")]
    InvalidInput(String),
}

fn ensure_actor(
    cmd: &Command,
    actor: &Actor,
    allowed: &[ActorKind],
) -> Result<(), TransitionError> {
    if allowed.contains(&actor.kind) {
        Ok(())
    } else {
        Err(TransitionError::ActorNotAllowed {
            command: cmd.name(),
            actor: actor.kind,
        })
    }
}

fn ensure_non_empty_id(id: &str, label: &str) -> Result<String, TransitionError> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err(TransitionError::InvalidInput(format!(
            "{label} must not be empty"
        )));
    }
    Ok(trimmed.to_string())
}

pub fn decide(
    idea: Option<&Idea>,
    cmd: &Command,
    actor: &Actor,
    facts: &Facts,
) -> Result<Vec<EventData>, TransitionError> {
    let Some(idea) = idea else {
        return match cmd {
            Command::Create {
                id,
                title,
                text,
                owner,
            } => {
                ensure_actor(cmd, actor, &[ActorKind::User, ActorKind::Orchestrator])?;
                let id = ensure_non_empty_id(id, "idea id")?;
                let title =
                    validate_title(title).map_err(|err| TransitionError::InvalidInput(err.0))?;
                let text =
                    validate_text(text).map_err(|err| TransitionError::InvalidInput(err.0))?;
                let owner =
                    validate_owner(owner).map_err(|err| TransitionError::InvalidInput(err.0))?;
                Ok(vec![EventData::IdeaCreated {
                    id,
                    title,
                    text,
                    owner,
                }])
            }
            _ => Err(TransitionError::NotFound),
        };
    };

    match (idea.state, cmd) {
        (_, Command::Create { .. }) => Err(TransitionError::AlreadyExists),

        (IdeaState::Inbox, Command::StartDiscovery) => {
            ensure_actor(cmd, actor, &[ActorKind::User, ActorKind::Orchestrator])?;
            Ok(vec![EventData::IdeaDiscoveryStarted {}])
        }

        (IdeaState::Ideation, Command::SelectDirection) => {
            ensure_actor(cmd, actor, &[ActorKind::User, ActorKind::Orchestrator])?;
            if !facts.has_feasibility {
                return Err(TransitionError::GateNotMet {
                    gate: ApprovalGate::Feasibility,
                    reason: "a feasibility study is required before selecting a direction"
                        .to_string(),
                });
            }
            if facts.feasibility_blocks {
                return Err(TransitionError::GateNotMet {
                    gate: ApprovalGate::Feasibility,
                    reason: "feasibility study marked this idea as infeasible".to_string(),
                });
            }
            Ok(vec![EventData::IdeaDirectionSelected {}])
        }

        (IdeaState::Planning, Command::MarkReady) => {
            ensure_actor(cmd, actor, &[ActorKind::User])?;
            if !facts.has_plan {
                return Err(TransitionError::GateNotMet {
                    gate: ApprovalGate::ReadyForDevelopment,
                    reason: "a plan is required before marking an idea ready for development"
                        .to_string(),
                });
            }
            if facts.open_blocking_questions > 0 {
                return Err(TransitionError::GateNotMet {
                    gate: ApprovalGate::ReadyForDevelopment,
                    reason: format!(
                        "{} open blocking question(s) must be resolved first",
                        facts.open_blocking_questions
                    ),
                });
            }
            Ok(vec![EventData::IdeaReady {}])
        }

        (
            IdeaState::Planning,
            Command::MarkReadyWithSnapshot {
                snapshot,
                approved_by,
                via,
            },
        ) => {
            ensure_actor(cmd, actor, &[ActorKind::User])?;
            if !facts.has_plan {
                return Err(TransitionError::GateNotMet {
                    gate: ApprovalGate::ReadyForDevelopment,
                    reason:
                        "a complete plan is required before marking an idea ready for development"
                            .to_string(),
                });
            }
            if facts.open_blocking_questions > 0 {
                return Err(TransitionError::GateNotMet {
                    gate: ApprovalGate::ReadyForDevelopment,
                    reason: format!(
                        "{} open blocking question(s) must be resolved first",
                        facts.open_blocking_questions
                    ),
                });
            }
            Ok(vec![
                EventData::IdeaReady {},
                EventData::IdeaPlanSnapshotFrozen {
                    plan_snapshot: snapshot.clone(),
                    approved_by: approved_by.clone(),
                    via: via.clone(),
                },
            ])
        }

        (IdeaState::ReadyForDevelopment, Command::RevokeReady) => {
            ensure_actor(cmd, actor, &[ActorKind::User])?;
            if facts.has_active_run {
                return Err(TransitionError::InvalidInput(
                    "cannot revoke ready state while an active run exists".to_string(),
                ));
            }
            Ok(vec![EventData::IdeaReadyRevoked {}])
        }

        (IdeaState::ReadyForDevelopment, Command::CreateRun { run }) => {
            ensure_actor(cmd, actor, &[ActorKind::System])?;
            let run = ensure_non_empty_id(run, "run id")?;
            Ok(vec![EventData::RunCreated {
                run,
                workflow: String::new(),
                version: String::new(),
                sha256: String::new(),
                source: String::new(),
                plan_sha256: String::new(),
                deploy_requested: false,
                stages: Vec::new(),
            }])
        }

        (IdeaState::Queued, Command::WithdrawRun { run }) => {
            ensure_actor(cmd, actor, &[ActorKind::User])?;
            let run = ensure_non_empty_id(run, "run id")?;
            Ok(vec![EventData::RunWithdrawn { run }])
        }

        (
            IdeaState::Queued | IdeaState::InDevelopment | IdeaState::Review,
            Command::CancelRun { run },
        ) => {
            ensure_actor(cmd, actor, &[ActorKind::User])?;
            let run = ensure_non_empty_id(run, "run id")?;
            Ok(vec![EventData::RunCancelled { run }])
        }

        (IdeaState::Queued, Command::StartRun { run }) => {
            ensure_actor(cmd, actor, &[ActorKind::System])?;
            let run = ensure_non_empty_id(run, "run id")?;
            Ok(vec![EventData::RunStarted { run }])
        }

        (IdeaState::InDevelopment, Command::StagesComplete { run }) => {
            ensure_actor(cmd, actor, &[ActorKind::System])?;
            let run = ensure_non_empty_id(run, "run id")?;
            Ok(vec![EventData::RunStagesComplete { run }])
        }

        (IdeaState::Review, Command::ChangesRequested { run }) => {
            ensure_actor(cmd, actor, &[ActorKind::System])?;
            let run = ensure_non_empty_id(run, "run id")?;
            Ok(vec![EventData::RunChangesRequested { run }])
        }

        (IdeaState::Review, Command::Deliver { run, prs }) => {
            ensure_actor(cmd, actor, &[ActorKind::System])?;
            let run = ensure_non_empty_id(run, "run id")?;
            if prs.is_empty() {
                return Err(TransitionError::InvalidInput(
                    "at least one pull request link is required to deliver an idea".to_string(),
                ));
            }
            Ok(vec![EventData::IdeaDelivered {
                run,
                prs: prs.clone(),
            }])
        }

        (state, Command::Cancel { reason }) if !state.is_terminal() => {
            ensure_actor(cmd, actor, &[ActorKind::User])?;
            Ok(vec![EventData::IdeaCancelled {
                reason: reason.clone(),
            }])
        }

        (
            state,
            Command::BindOrchestrator {
                cli,
                model,
                conversation,
            },
        ) if !state.is_terminal() => {
            ensure_actor(cmd, actor, &[ActorKind::User, ActorKind::System])?;
            Ok(vec![EventData::IdeaOrchestratorBound {
                cli: cli.clone(),
                model: model.clone(),
                conversation: conversation.clone(),
            }])
        }

        (
            IdeaState::Inbox | IdeaState::Ideation | IdeaState::Planning,
            Command::Describe { title, text },
        ) => {
            ensure_actor(cmd, actor, &[ActorKind::User, ActorKind::Orchestrator])?;
            let title =
                validate_title(title).map_err(|err| TransitionError::InvalidInput(err.0))?;
            let text = validate_text(text).map_err(|err| TransitionError::InvalidInput(err.0))?;
            let mut out = Vec::new();
            if idea.state == IdeaState::Inbox {
                out.push(EventData::IdeaDiscoveryStarted {});
            }
            out.push(EventData::IdeaDescribed { title, text });
            Ok(out)
        }

        (
            state,
            Command::StoreArtifact {
                kind,
                artifact,
                verdict,
                risk,
                workflow,
                missing_sections,
            },
        ) => {
            ensure_actor(cmd, actor, &[ActorKind::Orchestrator, ActorKind::User])?;
            if kind == "feasibility" {
                if !matches!(
                    state,
                    IdeaState::Inbox | IdeaState::Ideation | IdeaState::Planning
                ) {
                    return Err(TransitionError::IllegalTransition {
                        from: state,
                        command: cmd.name(),
                    });
                }
            } else if kind == "plan" {
                if !matches!(state, IdeaState::Ideation | IdeaState::Planning) {
                    return Err(TransitionError::IllegalTransition {
                        from: state,
                        command: cmd.name(),
                    });
                }
            } else {
                return Err(TransitionError::InvalidInput(format!(
                    "invalid artifact kind: {kind}"
                )));
            }
            Ok(vec![EventData::ArtifactStored {
                kind: kind.clone(),
                artifact: artifact.clone(),
                verdict: *verdict,
                risk: *risk,
                workflow: workflow.clone(),
                missing_sections: missing_sections.clone(),
            }])
        }

        (
            IdeaState::Inbox | IdeaState::Ideation | IdeaState::Planning,
            Command::AddAssumption { text },
        ) => {
            ensure_actor(cmd, actor, &[ActorKind::Orchestrator, ActorKind::User])?;
            let trimmed = text.trim();
            if trimmed.is_empty() || trimmed.chars().count() > 500 {
                return Err(TransitionError::InvalidInput(
                    "assumption must be 1..=500 characters".to_string(),
                ));
            }
            if idea.assumptions.len() >= 30 {
                return Err(TransitionError::InvalidInput(
                    "at most 30 assumptions allowed per idea".to_string(),
                ));
            }
            Ok(vec![EventData::IdeaAssumptionAdded {
                text: trimmed.to_string(),
            }])
        }

        (from, cmd) => Err(TransitionError::IllegalTransition {
            from,
            command: cmd.name(),
        }),
    }
}

pub fn apply(idea: Option<Idea>, event: &EventData, ts: f64) -> Option<Idea> {
    match (idea, event) {
        (
            None,
            EventData::IdeaCreated {
                id,
                title,
                text,
                owner,
            },
        ) => Some(Idea {
            id: id.clone(),
            title: title.clone(),
            text: text.clone(),
            state: IdeaState::Inbox,
            owner: owner.clone(),
            created: ts,
            updated: ts,
            risk: None,
            orchestrator: None,
            feasibility: None,
            feasibility_verdict: None,
            plan: None,
            plan_snapshot: None,
            artifacts: Vec::new(),
            assumptions: Vec::new(),
            runs: Vec::new(),
            prs: Vec::new(),
            cancelled_from: None,
        }),
        (None, _) => None,
        (Some(mut idea), ev) => {
            match ev {
                EventData::IdeaCreated { .. } => {}
                EventData::IdeaDiscoveryStarted {} if idea.state == IdeaState::Inbox => {
                    idea.state = IdeaState::Ideation;
                    idea.updated = ts;
                }
                EventData::IdeaOrchestratorBound {
                    cli,
                    model,
                    conversation,
                } => {
                    idea.orchestrator = Some(OrchestratorRef {
                        cli: cli.clone(),
                        model: model.clone(),
                        conversation: conversation.clone(),
                    });
                    idea.updated = ts;
                }
                EventData::IdeaDescribed { title, text } => {
                    idea.title = title.clone();
                    idea.text = text.clone();
                    idea.updated = ts;
                }
                EventData::ArtifactStored {
                    kind,
                    artifact,
                    verdict,
                    risk,
                    workflow,
                    missing_sections,
                } => {
                    idea.artifacts.push(artifact.clone());
                    if kind == "feasibility" {
                        idea.feasibility = Some(artifact.clone());
                        if verdict.is_some() {
                            idea.feasibility_verdict = *verdict;
                        }
                        if risk.is_some() {
                            idea.risk = *risk;
                        }
                    } else if kind == "plan" {
                        idea.plan = Some(Plan {
                            artifact: artifact.clone(),
                            workflow: workflow.clone().unwrap_or_default(),
                            acceptance_criteria: Vec::new(),
                            missing_sections: missing_sections.clone(),
                        });
                    }
                    idea.updated = ts;
                }
                EventData::IdeaAssumptionAdded { text } => {
                    idea.assumptions.push(text.clone());
                    idea.updated = ts;
                }
                EventData::IdeaDirectionSelected {} if idea.state == IdeaState::Ideation => {
                    idea.state = IdeaState::Planning;
                    idea.updated = ts;
                }
                EventData::IdeaReady {} if idea.state == IdeaState::Planning => {
                    idea.state = IdeaState::ReadyForDevelopment;
                    idea.updated = ts;
                }
                EventData::IdeaPlanSnapshotFrozen { plan_snapshot, .. } => {
                    idea.plan_snapshot = Some(plan_snapshot.clone());
                    idea.updated = ts;
                }
                EventData::IdeaReadyRevoked {} if idea.state == IdeaState::ReadyForDevelopment => {
                    idea.state = IdeaState::Planning;
                    idea.updated = ts;
                }
                EventData::RunCreated { run, .. }
                    if idea.state == IdeaState::ReadyForDevelopment =>
                {
                    idea.state = IdeaState::Queued;
                    if !idea.runs.contains(run) {
                        idea.runs.push(run.clone());
                    }
                    idea.updated = ts;
                }
                EventData::RunWithdrawn { run } if idea.state == IdeaState::Queued => {
                    idea.state = IdeaState::ReadyForDevelopment;
                    idea.runs.retain(|r| r != run);
                    idea.updated = ts;
                }
                EventData::RunCancelled { .. }
                    if matches!(
                        idea.state,
                        IdeaState::Queued | IdeaState::InDevelopment | IdeaState::Review
                    ) =>
                {
                    idea.state = IdeaState::ReadyForDevelopment;
                    idea.updated = ts;
                }
                EventData::RunStarted { .. } if idea.state == IdeaState::Queued => {
                    idea.state = IdeaState::InDevelopment;
                    idea.updated = ts;
                }
                EventData::RunStagesComplete { .. } if idea.state == IdeaState::InDevelopment => {
                    idea.state = IdeaState::Review;
                    idea.updated = ts;
                }
                EventData::RunChangesRequested { .. } if idea.state == IdeaState::Review => {
                    idea.state = IdeaState::InDevelopment;
                    idea.updated = ts;
                }
                EventData::IdeaDelivered { prs, .. } if idea.state == IdeaState::Review => {
                    idea.state = IdeaState::Delivered;
                    idea.prs = prs.clone();
                    idea.updated = ts;
                }
                EventData::PrCreated { pr, .. } => {
                    if !idea
                        .prs
                        .iter()
                        .any(|e| e.url == pr.url || (e.repo == pr.repo && e.number == pr.number))
                    {
                        idea.prs.push(pr.clone());
                    }
                    idea.updated = ts;
                }
                EventData::PrStatusRefreshed { pr, .. } => {
                    if let Some(existing) = idea
                        .prs
                        .iter_mut()
                        .find(|e| e.url == pr.url || (e.repo == pr.repo && e.number == pr.number))
                    {
                        existing.state = pr.state.clone();
                        existing.checks_summary = pr.checks_summary.clone();
                    }
                    idea.updated = ts;
                }
                EventData::IdeaCancelled { .. } if !idea.state.is_terminal() => {
                    idea.cancelled_from = Some(idea.state);
                    idea.state = IdeaState::Cancelled;
                    idea.updated = ts;
                }
                _ => {}
            }
            Some(idea)
        }
    }
}

pub fn apply_run(run: Option<Run>, event: &EventData, idea_id: &str, ts: f64) -> Option<Run> {
    match (run, event) {
        (
            None,
            EventData::RunCreated {
                run,
                workflow,
                version,
                sha256,
                source,
                plan_sha256,
                deploy_requested,
                stages,
            },
        ) => Some(Run {
            id: run.clone(),
            idea: idea_id.to_string(),
            workflow: workflow.clone(),
            workflow_version: version.clone(),
            workflow_sha256: sha256.clone(),
            workflow_source: source.clone(),
            plan_sha256: plan_sha256.clone(),
            deploy_requested: *deploy_requested,
            target: None,
            branch: None,
            base_commit: None,
            worktree_path: None,
            pause_reason: None,
            action_commands: Vec::new(),
            baseline_passed: None,
            state: RunState::Queued,
            stages: stages.clone(),
            evidence: Vec::new(),
            prs: Vec::new(),
            budget: None,
            created: ts,
            updated: ts,
        }),
        (None, _) => None,
        (Some(mut run), ev) => {
            match ev {
                EventData::RunStarted { .. } if run.state == RunState::Queued => {
                    run.state = RunState::Running;
                    run.updated = ts;
                }
                EventData::RunWithdrawn { .. } if run.state == RunState::Queued => {
                    run.state = RunState::Withdrawn;
                    run.updated = ts;
                }
                EventData::RunStagesComplete { .. } if run.state == RunState::Running => {
                    run.state = RunState::Review;
                    run.updated = ts;
                }
                EventData::RunChangesRequested { .. } if run.state == RunState::Review => {
                    run.state = RunState::Running;
                    run.updated = ts;
                }
                EventData::IdeaDelivered { .. } if run.state == RunState::Review => {
                    run.state = RunState::Delivered;
                    run.updated = ts;
                }
                EventData::StageStarted { stage, attempt, .. } => {
                    if let Some(st) = run.stages.iter_mut().find(|s| s.name == *stage) {
                        st.state = StageState::Running;
                        st.attempts = *attempt;
                        st.started = Some(ts);
                        st.finished = None;
                        st.rationale = None;
                    }
                    run.updated = ts;
                }
                EventData::StageSucceeded { stage, .. } => {
                    if let Some(st) = run.stages.iter_mut().find(|s| s.name == *stage) {
                        st.state = StageState::Succeeded;
                        st.finished = Some(ts);
                        st.rationale = None;
                        st.retry_hypothesis = None;
                    }
                    run.updated = ts;
                }
                EventData::StageFailed { stage, reason, .. } => {
                    if let Some(st) = run.stages.iter_mut().find(|s| s.name == *stage) {
                        st.state = StageState::Failed;
                        st.finished = Some(ts);
                        st.rationale = Some(reason.clone());
                    }
                    run.updated = ts;
                }
                EventData::StageSkipped { stage, reason, .. } => {
                    if let Some(st) = run.stages.iter_mut().find(|s| s.name == *stage) {
                        st.state = StageState::Skipped;
                        st.finished = Some(ts);
                        st.rationale = Some(reason.clone());
                    }
                    run.updated = ts;
                }
                EventData::StageBlocked { stage, reason, .. } => {
                    if let Some(st) = run.stages.iter_mut().find(|s| s.name == *stage) {
                        st.state = StageState::Blocked;
                        st.rationale = Some(reason.clone());
                    }
                    run.updated = ts;
                }
                EventData::WorkspacePrepared {
                    target,
                    branch,
                    base_commit,
                    worktree_path,
                    ..
                } => {
                    run.target = Some(target.clone());
                    run.branch = Some(branch.clone());
                    run.base_commit = Some(base_commit.clone());
                    run.worktree_path = Some(worktree_path.clone());
                    run.updated = ts;
                }
                EventData::RunTargetSelected { target, .. } => {
                    run.target = Some(target.clone());
                    run.updated = ts;
                }
                EventData::RunPaused {
                    reason, commands, ..
                } => {
                    run.state = RunState::Paused;
                    run.pause_reason = Some(reason.clone());
                    run.action_commands = commands.clone();
                    run.updated = ts;
                }
                EventData::RunResumed { .. } => {
                    run.state = RunState::Running;
                    run.pause_reason = None;
                    run.action_commands.clear();
                    run.updated = ts;
                }
                EventData::StageRetryRequested {
                    stage, hypothesis, ..
                } => {
                    if let Some(st) = run.stages.iter_mut().find(|s| s.name == *stage) {
                        st.retry_hypothesis = Some(hypothesis.clone());
                    }
                    run.updated = ts;
                }
                EventData::RunCancelled { .. }
                    if matches!(
                        run.state,
                        RunState::Pending
                            | RunState::Queued
                            | RunState::Running
                            | RunState::Paused
                            | RunState::Review
                    ) =>
                {
                    run.state = RunState::Cancelled;
                    run.pause_reason = None;
                    run.action_commands.clear();
                    run.updated = ts;
                }
                EventData::StageInterrupted { stage, .. } => {
                    if let Some(st) = run.stages.iter_mut().find(|s| s.name == *stage) {
                        st.state = StageState::Pending;
                        st.interrupted_attempts += 1;
                        st.finished = Some(ts);
                        st.rationale = Some("interrupted".to_string());
                    }
                    run.updated = ts;
                }
                EventData::StageReopened { stage, reason, .. } => {
                    if let Some(st) = run.stages.iter_mut().find(|s| s.name == *stage) {
                        st.state = StageState::Pending;
                        st.finished = None;
                        st.rationale = Some(reason.clone());
                        st.retry_hypothesis = Some(reason.clone());
                    }
                    // Reset downstream test, review, and demo stages to Pending so the scheduler re-runs them after develop.
                    if stage == "develop" {
                        for downstream in ["test", "review", "demo"] {
                            if let Some(st) = run.stages.iter_mut().find(|s| {
                                s.name == downstream
                                    && matches!(
                                        s.state,
                                        StageState::Failed
                                            | StageState::Succeeded
                                            | StageState::Running
                                    )
                            }) {
                                st.state = StageState::Pending;
                                st.finished = None;
                            }
                        }
                    }
                    run.updated = ts;
                }
                EventData::RunBaseline { passed, .. } => {
                    run.baseline_passed = Some(*passed);
                    run.updated = ts;
                }
                EventData::EvidenceAdded { item, .. } => {
                    run.evidence.push(item.clone());
                    run.updated = ts;
                }
                EventData::PrCreated { pr, .. } => {
                    if !run
                        .prs
                        .iter()
                        .any(|e| e.url == pr.url || (e.repo == pr.repo && e.number == pr.number))
                    {
                        run.prs.push(pr.clone());
                    }
                    run.updated = ts;
                }
                EventData::PrStatusRefreshed { pr, .. } => {
                    if let Some(existing) = run
                        .prs
                        .iter_mut()
                        .find(|e| e.url == pr.url || (e.repo == pr.repo && e.number == pr.number))
                    {
                        existing.state = pr.state.clone();
                        existing.checks_summary = pr.checks_summary.clone();
                    }
                    run.updated = ts;
                }
                _ => {}
            }
            Some(run)
        }
    }
}
