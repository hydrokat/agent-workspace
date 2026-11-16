use serde::{Deserialize, Serialize};

use super::model::{
    Actor, ActorKind, ApprovalGate, ArtifactRef, FeasibilityVerdict, Idea, IdeaState,
    OrchestratorRef, Plan, PrLink, Risk, validate_owner, validate_text, validate_title,
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
    RevokeReady,
    CreateRun {
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
            Self::RevokeReady => "RevokeReady",
            Self::CreateRun { .. } => "CreateRun",
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
    #[serde(rename = "idea.ready_revoked")]
    IdeaReadyRevoked {},
    #[serde(rename = "run.created")]
    RunCreated { run: String },
    #[serde(rename = "run.started")]
    RunStarted { run: String },
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

        (IdeaState::ReadyForDevelopment, Command::RevokeReady) => {
            ensure_actor(cmd, actor, &[ActorKind::User])?;
            if facts.has_active_run {
                return Err(TransitionError::InvalidInput(
                    "cannot revoke ready state while a run is active".to_string(),
                ));
            }
            Ok(vec![EventData::IdeaReadyRevoked {}])
        }

        (IdeaState::ReadyForDevelopment, Command::CreateRun { run }) => {
            ensure_actor(cmd, actor, &[ActorKind::System])?;
            let run = ensure_non_empty_id(run, "run id")?;
            Ok(vec![EventData::RunCreated { run }])
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
                EventData::IdeaReadyRevoked {} if idea.state == IdeaState::ReadyForDevelopment => {
                    idea.state = IdeaState::Planning;
                    idea.updated = ts;
                }
                EventData::RunCreated { run } if idea.state == IdeaState::ReadyForDevelopment => {
                    idea.state = IdeaState::Queued;
                    if !idea.runs.contains(run) {
                        idea.runs.push(run.clone());
                    }
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
