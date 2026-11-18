use std::collections::BTreeMap;
use std::fs::OpenOptions;

use serde::{Deserialize, Serialize};

use super::FactoryError;
use super::approval::{ApprovalError, ensure_user_caller};
use super::events::{Event, append_locked, redact_event};
use super::machine::EventData;
use super::model::{Actor, ActorKind, FactorySettings};
use super::redact::RedactContext;
use super::settings::{load as load_settings, save_unlocked, validate as validate_settings};
use super::store::{FactoryStore, current_time};
use super::workflow::schema::{StageDef, ToolClass, Workflow, WorkflowAgent, WorkflowTool};
use crate::coordination::filelock::exclusive_lock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolGrant {
    pub agent: WorkflowAgent,
    pub stage: String,
    pub target: String,
    pub tools: Vec<WorkflowTool>,
    pub read_only: bool,
    pub has_repo_write: bool,
}

impl ToolGrant {
    pub fn from_tools(
        agent: WorkflowAgent,
        stage: impl Into<String>,
        target: impl Into<String>,
        tools: Vec<WorkflowTool>,
    ) -> Self {
        let has_repo_write = tools.iter().any(|t| t.class() == ToolClass::Write);
        Self {
            agent,
            stage: stage.into(),
            target: target.into(),
            tools,
            read_only: !has_repo_write,
            has_repo_write,
        }
    }

    pub fn read_only_for(
        agent: WorkflowAgent,
        stage: impl Into<String>,
        target: impl Into<String>,
    ) -> Self {
        Self::from_tools(agent, stage, target, Vec::new())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeniedReason {
    Gate3Required,
    NotAllowedForAgent,
    DeniedForTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("policy denied {tool} for {agent} in {stage}")]
pub struct Denied {
    pub tool: WorkflowTool,
    pub agent: WorkflowAgent,
    pub stage: String,
    pub target: String,
    pub reason: DeniedReason,
}

impl Denied {
    pub fn action_commands(&self, run_id: &str) -> Vec<String> {
        match self.reason {
            DeniedReason::Gate3Required => vec![
                format!(
                    "tandem factory run approve {run_id} --action production_deploy --confirm {run_id}"
                ),
                format!("tandem factory run resume {run_id}"),
            ],
            DeniedReason::NotAllowedForAgent => vec![
                format!(
                    "tandem factory permissions allow {} {}",
                    self.agent, self.tool
                ),
                format!("tandem factory run resume {run_id}"),
            ],
            DeniedReason::DeniedForTarget => vec![
                format!(
                    "tandem factory permissions allow --target {} {}",
                    self.target, self.tool
                ),
                format!("tandem factory run resume {run_id}"),
            ],
        }
    }
}

pub fn default_allowed_tools(agent: WorkflowAgent) -> Vec<WorkflowTool> {
    match agent {
        WorkflowAgent::Orchestrator => vec![WorkflowTool::RepoRead, WorkflowTool::IssueContext],
        WorkflowAgent::TestEngineer => vec![
            WorkflowTool::RepoRead,
            WorkflowTool::RepoWrite,
            WorkflowTool::TestRunner,
        ],
        WorkflowAgent::Implementer => vec![
            WorkflowTool::RepoRead,
            WorkflowTool::RepoWrite,
            WorkflowTool::Shell,
        ],
        WorkflowAgent::Verifier => vec![WorkflowTool::TestRunner, WorkflowTool::LintRunner],
        WorkflowAgent::Reviewer => vec![WorkflowTool::RepoRead, WorkflowTool::DiffReview],
        WorkflowAgent::DemoMaker => vec![WorkflowTool::Browser, WorkflowTool::Screenshot],
        WorkflowAgent::ReleaseEngineer => vec![WorkflowTool::Ci, WorkflowTool::Deployment],
    }
}

pub fn grant(
    agent: WorkflowAgent,
    stage: &StageDef,
    target: &str,
    _workflow: &Workflow,
    settings: &FactorySettings,
    has_gate3_approval: bool,
) -> Result<ToolGrant, Denied> {
    let agent_allowed = settings
        .allowed_tools
        .get(agent.as_str())
        .cloned()
        .unwrap_or_else(|| default_allowed_tools(agent));
    let target_denied = settings
        .denied_tools
        .get(target)
        .map(Vec::as_slice)
        .unwrap_or(&[]);

    let mut granted = Vec::with_capacity(stage.tools.len());
    for &tool in &stage.tools {
        if matches!(tool, WorkflowTool::Ci | WorkflowTool::Deployment) && !has_gate3_approval {
            return Err(Denied {
                tool,
                agent,
                stage: stage.name.clone(),
                target: target.to_string(),
                reason: DeniedReason::Gate3Required,
            });
        }
        if !agent_allowed.contains(&tool) {
            return Err(Denied {
                tool,
                agent,
                stage: stage.name.clone(),
                target: target.to_string(),
                reason: DeniedReason::NotAllowedForAgent,
            });
        }
        if target_denied.contains(&tool) {
            return Err(Denied {
                tool,
                agent,
                stage: stage.name.clone(),
                target: target.to_string(),
                reason: DeniedReason::DeniedForTarget,
            });
        }
        if !granted.contains(&tool) {
            granted.push(tool);
        }
    }

    Ok(ToolGrant::from_tools(agent, &stage.name, target, granted))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PermissionsView {
    pub default_allowed_tools: BTreeMap<String, Vec<WorkflowTool>>,
    pub allowed_tools: BTreeMap<String, Vec<WorkflowTool>>,
    pub denied_tools: BTreeMap<String, Vec<WorkflowTool>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PermissionChangeResult {
    pub scope: String,
    pub subject: String,
    pub tool: WorkflowTool,
    pub action: String,
    pub settings: FactorySettings,
}

const ALL_AGENTS: [WorkflowAgent; 7] = [
    WorkflowAgent::Orchestrator,
    WorkflowAgent::TestEngineer,
    WorkflowAgent::Implementer,
    WorkflowAgent::Verifier,
    WorkflowAgent::Reviewer,
    WorkflowAgent::DemoMaker,
    WorkflowAgent::ReleaseEngineer,
];

pub fn show(settings: &FactorySettings) -> PermissionsView {
    let mut defaults = BTreeMap::new();
    for agent in ALL_AGENTS {
        defaults.insert(agent.as_str().to_string(), default_allowed_tools(agent));
    }
    PermissionsView {
        default_allowed_tools: defaults,
        allowed_tools: settings.allowed_tools.clone(),
        denied_tools: settings.denied_tools.clone(),
    }
}

struct PermissionMutation<'a> {
    key: String,
    scope: &'a str,
    subject: &'a str,
    tool: WorkflowTool,
    action: &'a str,
}

fn mutate_permissions<F>(
    store: &FactoryStore,
    mutation: PermissionMutation<'_>,
    actor_id: &str,
    env: &dyn Fn(&str) -> Option<String>,
    mutate: F,
) -> Result<PermissionChangeResult, ApprovalError>
where
    F: FnOnce(&mut FactorySettings) -> Result<(serde_json::Value, serde_json::Value), FactoryError>,
{
    ensure_user_caller(env)?;
    store.ensure()?;

    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(store.lock_path())
        .map_err(FactoryError::Io)?;
    let _guard = exclusive_lock(&lock_file).map_err(FactoryError::Io)?;

    let mut settings = load_settings(store)?;
    let (old_value, new_value) = mutate(&mut settings)?;
    validate_settings(&settings)?;

    let actor = Actor {
        kind: ActorKind::User,
        id: actor_id.to_string(),
    };
    let ev = Event::from_data(
        current_time(),
        actor,
        "settings".to_string(),
        None,
        &EventData::SettingsChanged {
            key: mutation.key,
            old_value,
            new_value,
            actor: actor_id.to_string(),
        },
    )?;
    append_locked(
        &store.events_path(),
        &[redact_event(ev, &RedactContext::from_env())],
    )?;
    save_unlocked(store, &settings)?;

    Ok(PermissionChangeResult {
        scope: mutation.scope.to_string(),
        subject: mutation.subject.to_string(),
        tool: mutation.tool,
        action: mutation.action.to_string(),
        settings,
    })
}

pub fn allow_agent_tool(
    store: &FactoryStore,
    agent: WorkflowAgent,
    tool: WorkflowTool,
    actor_id: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<PermissionChangeResult, ApprovalError> {
    let agent_key = agent.as_str().to_string();
    mutate_permissions(
        store,
        PermissionMutation {
            key: format!("allowed_tools.{agent_key}"),
            scope: "agent",
            subject: &agent_key,
            tool,
            action: "allow",
        },
        actor_id,
        env,
        |settings| {
            let old = serde_json::to_value(settings.allowed_tools.get(&agent_key))?;
            let entry = settings
                .allowed_tools
                .entry(agent_key.clone())
                .or_insert_with(|| default_allowed_tools(agent));
            if !entry.contains(&tool) {
                entry.push(tool);
            }
            let new = serde_json::to_value(settings.allowed_tools.get(&agent_key))?;
            Ok((old, new))
        },
    )
}

pub fn deny_agent_tool(
    store: &FactoryStore,
    agent: WorkflowAgent,
    tool: WorkflowTool,
    actor_id: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<PermissionChangeResult, ApprovalError> {
    let agent_key = agent.as_str().to_string();
    mutate_permissions(
        store,
        PermissionMutation {
            key: format!("allowed_tools.{agent_key}"),
            scope: "agent",
            subject: &agent_key,
            tool,
            action: "deny",
        },
        actor_id,
        env,
        |settings| {
            let old = serde_json::to_value(settings.allowed_tools.get(&agent_key))?;
            let entry = settings
                .allowed_tools
                .entry(agent_key.clone())
                .or_insert_with(|| default_allowed_tools(agent));
            entry.retain(|t| *t != tool);
            let new = serde_json::to_value(settings.allowed_tools.get(&agent_key))?;
            Ok((old, new))
        },
    )
}

pub fn deny_target_tool(
    store: &FactoryStore,
    target: &str,
    tool: WorkflowTool,
    actor_id: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<PermissionChangeResult, ApprovalError> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return Err(ApprovalError::InvalidInput(
            "target name must not be empty".to_string(),
        ));
    }
    let target_key = trimmed.to_string();
    mutate_permissions(
        store,
        PermissionMutation {
            key: format!("denied_tools.{target_key}"),
            scope: "target",
            subject: &target_key,
            tool,
            action: "deny",
        },
        actor_id,
        env,
        |settings| {
            let old = serde_json::to_value(settings.denied_tools.get(&target_key))?;
            let entry = settings.denied_tools.entry(target_key.clone()).or_default();
            if !entry.contains(&tool) {
                entry.push(tool);
            }
            let new = serde_json::to_value(settings.denied_tools.get(&target_key))?;
            Ok((old, new))
        },
    )
}

pub fn allow_target_tool(
    store: &FactoryStore,
    target: &str,
    tool: WorkflowTool,
    actor_id: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<PermissionChangeResult, ApprovalError> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return Err(ApprovalError::InvalidInput(
            "target name must not be empty".to_string(),
        ));
    }
    let target_key = trimmed.to_string();
    mutate_permissions(
        store,
        PermissionMutation {
            key: format!("denied_tools.{target_key}"),
            scope: "target",
            subject: &target_key,
            tool,
            action: "allow",
        },
        actor_id,
        env,
        |settings| {
            let old = serde_json::to_value(settings.denied_tools.get(&target_key))?;
            if let Some(entry) = settings.denied_tools.get_mut(&target_key) {
                entry.retain(|t| *t != tool);
                if entry.is_empty() {
                    settings.denied_tools.remove(&target_key);
                }
            }
            let new = serde_json::to_value(settings.denied_tools.get(&target_key))?;
            Ok((old, new))
        },
    )
}
