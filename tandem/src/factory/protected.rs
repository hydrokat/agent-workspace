use std::fmt;

use serde::{Deserialize, Serialize};

use super::FactoryError;
use super::approval::{ApprovalError, Caller, require_user};
use super::checks::require_confirmed_deploy;
use super::events::{Event, append_locked, read_all, redact_events, set_private_file_mode};
use super::ids::is_valid_run_id;
use super::machine::EventData;
use super::model::{Actor, ActorKind, Approval, ApprovalGate, Run};
use super::redact::RedactContext;
use super::run::get_run;
use super::store::{FactoryStore, current_time};
use super::workflow::schema::{StageDef, StageRequirement, WorkflowTool};
use crate::coordination::filelock::exclusive_lock;

pub const APPROVAL_TTL_SECS: f64 = 3600.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtectedAction {
    ProductionDeploy,
    SecretsAccess,
    ProductionAccess,
}

impl ProtectedAction {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "production_deploy" => Some(Self::ProductionDeploy),
            "secrets_access" => Some(Self::SecretsAccess),
            "production_access" => Some(Self::ProductionAccess),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ProductionDeploy => "production_deploy",
            Self::SecretsAccess => "secrets_access",
            Self::ProductionAccess => "production_access",
        }
    }
}

impl fmt::Display for ProtectedAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProtectedApproval {
    pub seq: u64,
    pub gate: u8,
    pub action: ProtectedAction,
    pub run: String,
    pub stage: String,
    pub workflow_sha256: String,
    pub actor: Actor,
    pub surface: String,
    pub granted_at: f64,
    pub expires_at: f64,
    pub used: bool,
}

impl ProtectedApproval {
    pub fn matches(&self, run_id: &str, stage: &str, workflow_sha256: &str, now: f64) -> bool {
        !self.used
            && self.run == run_id
            && self.stage == stage
            && self.workflow_sha256 == workflow_sha256
            && now < self.expires_at
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ApproveOutcome {
    pub approval: ProtectedApproval,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deploy_command: Option<Vec<String>>,
    pub expires_in_minutes: u64,
}

pub fn action_for_stage(
    stage_def: &StageDef,
    policy_actions: &[ProtectedAction],
) -> ProtectedAction {
    if stage_def.tools.contains(&WorkflowTool::Deployment) || stage_def.name == "deploy" {
        return ProtectedAction::ProductionDeploy;
    }
    policy_actions
        .first()
        .copied()
        .unwrap_or(ProtectedAction::ProductionDeploy)
}

#[allow(clippy::too_many_arguments)]
pub fn approve(
    store: &FactoryStore,
    run_id: &str,
    action: ProtectedAction,
    confirm: &str,
    stage: Option<&str>,
    approved_by: &str,
    caller: &Caller<'_>,
) -> Result<ProtectedApproval, ApprovalError> {
    approve_with_details(store, run_id, action, confirm, stage, approved_by, caller)
        .map(|o| o.approval)
}

#[allow(clippy::too_many_arguments)]
pub fn approve_with_details(
    store: &FactoryStore,
    run_id: &str,
    action: ProtectedAction,
    confirm: &str,
    stage: Option<&str>,
    approved_by: &str,
    caller: &Caller<'_>,
) -> Result<ApproveOutcome, ApprovalError> {
    let surface = require_user(caller)?;
    let via = surface.as_str();

    if !is_valid_run_id(run_id) {
        return Err(ApprovalError::InvalidInput(format!(
            "invalid run id: {run_id}"
        )));
    }
    if confirm != run_id {
        return Err(ApprovalError::InvalidInput(format!(
            "confirmation mismatch: pass --confirm {run_id}"
        )));
    }
    if approved_by.trim().is_empty() {
        return Err(ApprovalError::InvalidInput(
            "approved_by must not be empty".to_string(),
        ));
    }

    let run = get_run(store, run_id)
        .map_err(|e| ApprovalError::InvalidInput(e.to_string()))?
        .ok_or_else(|| ApprovalError::InvalidInput(format!("run not found: {run_id}")))?;

    let target_name = run.target.as_deref().unwrap_or("workspace");
    let settings = store.settings()?;
    let deploy_command = if action == ProtectedAction::ProductionDeploy {
        let confirmed = require_confirmed_deploy(&settings, target_name).map_err(|_| {
            ApprovalError::Gate(format!(
                "Refused: no deploy command is confirmed for {target_name}."
            ))
        })?;
        Some(confirmed.command().to_vec())
    } else {
        None
    };

    let resolved_stage = match stage {
        Some(s) => {
            if !run.stages.iter().any(|st| st.name == s) {
                return Err(ApprovalError::InvalidInput(format!(
                    "stage '{s}' does not exist in run {run_id}"
                )));
            }
            s.to_string()
        }
        None => {
            if action == ProtectedAction::ProductionDeploy {
                run.stages
                    .iter()
                    .find(|st| st.name == "deploy")
                    .map(|st| st.name.clone())
                    .ok_or_else(|| {
                        ApprovalError::InvalidInput(format!("run {run_id} has no deploy stage"))
                    })?
            } else {
                let wf_path = store.run_dir(run_id).join("workflow.yaml");
                let wf_stage = super::workflow::parse::load_workflow_file(&wf_path)
                    .ok()
                    .and_then(|raw| super::workflow::validate::validate(&raw, &settings).ok())
                    .and_then(|wf| {
                        wf.stages
                            .into_iter()
                            .find(|s| s.requires.contains(&StageRequirement::HumanApproval))
                            .map(|s| s.name)
                    });
                wf_stage.unwrap_or_else(|| {
                    run.stages
                        .first()
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| "deploy".to_string())
                })
            }
        }
    };

    let actor = Actor {
        kind: ActorKind::User,
        id: approved_by.to_string(),
    };
    let granted_at = current_time();
    let expires_at = granted_at + APPROVAL_TTL_SECS;

    store.ensure()?;
    let lock_path = store.lock_path();
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(FactoryError::from)?;
    set_private_file_mode(&lock_path).map_err(ApprovalError::from)?;
    let _guard = exclusive_lock(&lock_file).map_err(FactoryError::from)?;

    let ev_data = EventData::ApprovalGranted {
        gate: 3,
        action,
        run: run.id.clone(),
        stage: resolved_stage.clone(),
        workflow_sha256: run.workflow_sha256.clone(),
        actor: actor.clone(),
        surface: via.to_string(),
        expires_at,
    };
    let ev = Event::from_data(
        granted_at,
        actor.clone(),
        run.idea.clone(),
        Some(run.id.clone()),
        &ev_data,
    )?;
    let redacted = redact_events(&[ev], &RedactContext::from_env());
    let appended = append_locked(&store.events_path(), &redacted)?;
    let ev = appended
        .into_iter()
        .last()
        .ok_or_else(|| ApprovalError::InvalidInput("approval event was not recorded".into()))?;

    Ok(ApproveOutcome {
        approval: ProtectedApproval {
            seq: ev.seq,
            gate: 3,
            action,
            run: run.id,
            stage: resolved_stage,
            workflow_sha256: run.workflow_sha256,
            actor,
            surface: via.to_string(),
            granted_at: ev.ts,
            expires_at,
            used: false,
        },
        deploy_command,
        expires_in_minutes: (APPROVAL_TTL_SECS / 60.0) as u64,
    })
}

pub fn load_run_approvals(
    store: &FactoryStore,
    run_id: &str,
) -> Result<Vec<ProtectedApproval>, FactoryError> {
    let log = read_all(&store.events_path())?;
    let mut approvals = Vec::new();
    for ev in &log.events {
        let Ok(data) = ev.to_event_data() else {
            continue;
        };
        match data {
            EventData::ApprovalGranted {
                gate,
                action,
                run,
                stage,
                workflow_sha256,
                actor,
                surface,
                expires_at,
            } if run == run_id => {
                approvals.push(ProtectedApproval {
                    seq: ev.seq,
                    gate,
                    action,
                    run,
                    stage,
                    workflow_sha256,
                    actor,
                    surface,
                    granted_at: ev.ts,
                    expires_at,
                    used: false,
                });
            }
            EventData::ApprovalConsumed {
                run,
                stage,
                action,
                approval_seq,
            } if run == run_id => {
                if let Some(found) = approvals.iter_mut().find(|a| a.seq == approval_seq) {
                    found.used = true;
                } else if let Some(found) = approvals
                    .iter_mut()
                    .rev()
                    .find(|a| !a.used && a.stage == stage && a.action == action)
                {
                    found.used = true;
                }
            }
            _ => {}
        }
    }
    Ok(approvals)
}

pub fn find_matching_approval<'a>(
    approvals: &'a [ProtectedApproval],
    run_id: &str,
    stage: &str,
    workflow_sha256: &str,
    now: f64,
) -> Option<&'a ProtectedApproval> {
    approvals
        .iter()
        .rev()
        .find(|a| a.matches(run_id, stage, workflow_sha256, now))
}

pub fn to_scheduler_approvals(
    approvals: &[ProtectedApproval],
    run: &Run,
    now: f64,
) -> Vec<Approval> {
    approvals
        .iter()
        .filter(|a| a.matches(&run.id, &a.stage, &run.workflow_sha256, now))
        .map(|a| Approval {
            gate: ApprovalGate::ProtectedAction,
            actor: a.actor.clone(),
            via: a.surface.clone(),
            at: a.granted_at,
            subject: Some(a.stage.clone()),
        })
        .collect()
}
