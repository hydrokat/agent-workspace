use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::FactoryError;
use super::broker::gate::{check as check_provider_gate, today_iso_date};
use super::budget::{
    Estimate, EstimateHistory, RunBudget, cost_cap_label, estimate, provider_reports_usage,
    routes_for_workflow,
};
use super::events::read_all;
use super::ids::is_valid_idea_id;
use super::machine::Command;
use super::model::{Actor, ActorKind, ArtifactRef, Budget, Idea, IdeaState, KNOWN_PROVIDERS, Run};
use super::run::{CreateRunOptions, create_run};
use super::store::FactoryStore;
use super::workflow::load_default_workflow;
use super::workflow::parse::load_workflow_file;
use super::workflow::schema::WorkflowAgent;
use super::workflow::validate::{resolve_workflow_path, validate};

const AGENT_ENV_VARS: &[&str] = &["TANDEM_ROLE", "TANDEM_SESSION_ID", "TANDEM_FACTORY_IDEA"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Cli,
    Tui,
    Web,
}

impl Surface {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Tui => "tui",
            Self::Web => "web",
        }
    }
}

pub struct Caller<'a> {
    pub surface: Surface,
    pub is_terminal: bool,
    pub env: &'a dyn Fn(&str) -> Option<String>,
    pub pane_role: &'a dyn Fn(&str) -> Option<String>,
}

impl<'a> Caller<'a> {
    pub fn cli(
        is_terminal: bool,
        env: &'a dyn Fn(&str) -> Option<String>,
        pane_role: &'a dyn Fn(&str) -> Option<String>,
    ) -> Self {
        Self {
            surface: Surface::Cli,
            is_terminal,
            env,
            pane_role,
        }
    }

    pub fn tui(
        env: &'a dyn Fn(&str) -> Option<String>,
        pane_role: &'a dyn Fn(&str) -> Option<String>,
    ) -> Self {
        Self {
            surface: Surface::Tui,
            is_terminal: true,
            env,
            pane_role,
        }
    }

    pub fn web(env: &'a dyn Fn(&str) -> Option<String>) -> Self {
        Self {
            surface: Surface::Web,
            is_terminal: true,
            env,
            pane_role: &|_| None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApprovalError {
    #[error("only the user can mark an idea ready for development")]
    AgentCaller,
    #[error(
        "Refused: this action needs a terminal. Use the Factory tab, the web UI, or run it in your own shell."
    )]
    TerminalRequired,
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    Gate(String),
    #[error(transparent)]
    Factory(#[from] FactoryError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl ApprovalError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::AgentCaller | Self::TerminalRequired | Self::Gate(_) => 3,
            Self::InvalidInput(_) | Self::Factory(_) | Self::Io(_) => 1,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::AgentCaller => "agent_caller_forbidden",
            Self::TerminalRequired => "terminal_required",
            Self::Gate(_) => "gate_not_met",
            Self::InvalidInput(_) => "invalid_input",
            Self::Factory(_) | Self::Io(_) => "internal_error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotVerification {
    pub version: u32,
    pub sha256: String,
    pub verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct MarkReadyOptions {
    pub workflow_override: Option<String>,
    pub deploy_requested: bool,
    pub max_cost_usd: Option<f64>,
    pub max_duration_secs: Option<u64>,
    pub max_retries: Option<u32>,
    pub accept_estimate: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReadyResult {
    pub idea: Idea,
    pub snapshot: SnapshotVerification,
    pub message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<Run>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<RunBudget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate: Option<Estimate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_cap_note: Option<String>,
}

fn is_agent_role(role: &str) -> bool {
    let trimmed = role.trim();
    crate::state::ROLES.contains(&trimmed)
        || crate::state::FACTORY_ROLES.contains(&trimmed)
        || WorkflowAgent::parse(trimmed).is_some()
}

pub fn require_user(caller: &Caller<'_>) -> Result<Surface, ApprovalError> {
    for var in AGENT_ENV_VARS {
        if let Some(val) = (caller.env)(var)
            && !val.trim().is_empty()
        {
            return Err(ApprovalError::AgentCaller);
        }
    }
    if caller.surface == Surface::Cli && !caller.is_terminal {
        return Err(ApprovalError::TerminalRequired);
    }
    if let Some(pane_id) = (caller.env)("TMUX_PANE")
        && !pane_id.trim().is_empty()
        && let Some(role) = (caller.pane_role)(pane_id.trim())
        && is_agent_role(&role)
    {
        return Err(ApprovalError::AgentCaller);
    }
    Ok(caller.surface)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRunConfig {
    pub deploy_requested: bool,
    pub workflow: Option<String>,
    pub budget: Option<Budget>,
}

pub fn pending_run_path(store: &FactoryStore, idea_id: &str) -> PathBuf {
    store
        .root
        .join("ideas")
        .join(idea_id)
        .join("pending-run.json")
}

fn save_pending_run(
    store: &FactoryStore,
    idea_id: &str,
    cfg: &PendingRunConfig,
) -> Result<(), ApprovalError> {
    let path = pending_run_path(store, idea_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes =
        serde_json::to_vec_pretty(cfg).map_err(|e| ApprovalError::InvalidInput(e.to_string()))?;
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn take_pending_run(store: &FactoryStore, idea_id: &str) -> Option<PendingRunConfig> {
    let path = pending_run_path(store, idea_id);
    let bytes = fs::read(&path).ok()?;
    let _ = fs::remove_file(&path);
    serde_json::from_slice(&bytes).ok()
}

fn remove_pending_run(store: &FactoryStore, idea_id: &str) {
    let _ = fs::remove_file(pending_run_path(store, idea_id));
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::new().chain_update(bytes).finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn mark_ready(
    store: &FactoryStore,
    idea_id: &str,
    confirm: &str,
    approved_by: &str,
    caller: &Caller<'_>,
) -> Result<ReadyResult, ApprovalError> {
    let surface = require_user(caller)?;
    let via = surface.as_str();

    if !is_valid_idea_id(idea_id) {
        return Err(ApprovalError::InvalidInput(format!(
            "invalid idea id: {idea_id}"
        )));
    }
    if confirm != idea_id {
        return Err(ApprovalError::InvalidInput(format!(
            "confirmation mismatch: pass --confirm {idea_id}"
        )));
    }

    let idea = store
        .get_idea(idea_id)?
        .ok_or_else(|| ApprovalError::InvalidInput(format!("idea not found: {idea_id}")))?;
    if idea.state != IdeaState::Planning {
        return Err(ApprovalError::Gate(format!(
            "idea {idea_id} is in {}; advance to Planning first",
            idea.state.label()
        )));
    }
    let plan = idea.plan.as_ref().ok_or_else(|| {
        ApprovalError::Gate(
            "a plan is required before marking an idea ready for development".into(),
        )
    })?;
    if !plan.missing_sections.is_empty() {
        return Err(ApprovalError::Gate(format!(
            "plan is missing required sections: {}",
            plan.missing_sections.join(", ")
        )));
    }

    let plan_src = store
        .root
        .join("ideas")
        .join(idea_id)
        .join("artifacts")
        .join(format!("plan-v{}.md", plan.artifact.version));
    let bytes = fs::read(&plan_src)?;
    let sha256 = sha256_hex(&bytes);

    let artifacts_dir = store.root.join("ideas").join(idea_id).join("artifacts");
    fs::create_dir_all(&artifacts_dir)?;
    let snap_dest = artifacts_dir.join("plan-snapshot.md");
    let snap_tmp = artifacts_dir.join(".plan-snapshot.md.tmp");
    fs::write(&snap_tmp, &bytes)?;
    fs::rename(&snap_tmp, &snap_dest)?;

    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let snapshot_ref = ArtifactRef {
        kind: "plan_snapshot".to_string(),
        version: plan.artifact.version,
        path: format!("ideas/{idea_id}/artifacts/plan-snapshot.md"),
        sha256: sha256.clone(),
        bytes: bytes.len() as u64,
        created,
    };

    let actor = Actor {
        kind: ActorKind::User,
        id: approved_by.to_string(),
    };
    let updated = store
        .execute(
            idea_id,
            Command::MarkReadyWithSnapshot {
                snapshot: snapshot_ref.clone(),
                approved_by: approved_by.to_string(),
                via: via.to_string(),
            },
            &actor,
        )
        .map_err(|e| match e {
            FactoryError::Transition(te) => ApprovalError::Gate(te.to_string()),
            other => ApprovalError::Factory(other),
        })?;

    Ok(ReadyResult {
        idea: updated,
        snapshot: SnapshotVerification {
            version: snapshot_ref.version,
            sha256,
            verified: true,
            error: None,
        },
        message: "Ready for development. Production runs arrive with the workflow engine.",
        run: None,
        run_error: None,
        budget: None,
        estimate: None,
        cost_cap_note: None,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn mark_ready_with_workflow(
    store: &FactoryStore,
    workspace: &Path,
    idea_id: &str,
    confirm: &str,
    approved_by: &str,
    workflow_override: Option<&str>,
    deploy_requested: bool,
    caller: &Caller<'_>,
) -> Result<ReadyResult, ApprovalError> {
    mark_ready_with_options(
        store,
        workspace,
        idea_id,
        confirm,
        approved_by,
        MarkReadyOptions {
            workflow_override: workflow_override.map(str::to_string),
            deploy_requested,
            ..Default::default()
        },
        caller,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn mark_ready_with_options(
    store: &FactoryStore,
    workspace: &Path,
    idea_id: &str,
    confirm: &str,
    approved_by: &str,
    options: MarkReadyOptions,
    caller: &Caller<'_>,
) -> Result<ReadyResult, ApprovalError> {
    require_user(caller)?;

    if !is_valid_idea_id(idea_id) {
        return Err(ApprovalError::InvalidInput(format!(
            "invalid idea id: {idea_id}"
        )));
    }
    if confirm != idea_id {
        return Err(ApprovalError::InvalidInput(format!(
            "confirmation mismatch: pass --confirm {idea_id}"
        )));
    }

    let idea = store
        .get_idea(idea_id)?
        .ok_or_else(|| ApprovalError::InvalidInput(format!("idea not found: {idea_id}")))?;
    if idea.state != IdeaState::Planning {
        return Err(ApprovalError::Gate(format!(
            "idea {idea_id} is in {}; advance to Planning first",
            idea.state.label()
        )));
    }
    let plan = idea.plan.as_ref().ok_or_else(|| {
        ApprovalError::Gate(
            "a plan is required before marking an idea ready for development".into(),
        )
    })?;
    if !plan.missing_sections.is_empty() {
        return Err(ApprovalError::Gate(format!(
            "plan is missing required sections: {}",
            plan.missing_sections.join(", ")
        )));
    }

    let settings = store.settings()?;
    let resolved_wf = if let Some(explicit) = options.workflow_override.as_deref() {
        let raw = if explicit == "default-delivery" {
            load_default_workflow(workspace)
                .map(|(r, _)| r)
                .map_err(|e| ApprovalError::InvalidInput(e.to_string()))?
        } else {
            let path =
                resolve_workflow_path(workspace, explicit).map_err(ApprovalError::InvalidInput)?;
            load_workflow_file(&path).map_err(|e| ApprovalError::InvalidInput(e.to_string()))?
        };
        validate(&raw, &settings).map_err(|diags| {
            let msgs: Vec<String> = diags.iter().map(ToString::to_string).collect();
            ApprovalError::InvalidInput(format!("workflow {explicit} invalid: {}", msgs.join("; ")))
        })?
    } else {
        let rec = plan.workflow.trim();
        let mut chosen = None;
        if !rec.is_empty()
            && rec != "default-delivery"
            && let Ok(path) = resolve_workflow_path(workspace, rec)
            && let Ok(raw) = load_workflow_file(&path)
            && let Ok(wf) = validate(&raw, &settings)
        {
            chosen = Some(wf);
        }
        if let Some(wf) = chosen {
            wf
        } else {
            let (raw, _) = load_default_workflow(workspace)
                .map_err(|e| ApprovalError::InvalidInput(e.to_string()))?;
            validate(&raw, &settings).map_err(|diags| {
                let msgs: Vec<String> = diags.iter().map(ToString::to_string).collect();
                ApprovalError::InvalidInput(format!(
                    "default workflow invalid: {}",
                    msgs.join("; ")
                ))
            })?
        }
    };

    let run_budget = RunBudget::from_settings(&settings).with_overrides(
        options.max_cost_usd,
        options.max_duration_secs,
        options.max_retries,
        &settings,
    )?;

    let log = read_all(&store.root.join("events.jsonl"))?;
    let history = EstimateHistory::from_events(&log.events);
    let today = today_iso_date();
    let open_providers: Vec<&str> = KNOWN_PROVIDERS
        .iter()
        .copied()
        .filter(|p| check_provider_gate(p, &settings, &today).is_ok())
        .collect();
    let preferred = idea.orchestrator.as_ref().map(|o| o.cli.clone());
    let (routes, primary_provider) =
        routes_for_workflow(&resolved_wf, &settings, &open_providers, preferred);
    let est = estimate(&resolved_wf, &routes, &settings.price_table, &history);

    if let Some(reason) = est.exceeds_cap(&run_budget)
        && !options.accept_estimate
    {
        return Err(ApprovalError::Gate(format!(
            "{reason}; pass --accept-estimate to confirm and start anyway"
        )));
    }

    let cost_cap_note = match (run_budget.max_cost_usd, primary_provider.as_deref()) {
        (Some(_), Some(p)) if !provider_reports_usage(p) => {
            Some(cost_cap_label(&run_budget, Some(p)))
        }
        _ => None,
    };

    let mut ready = mark_ready(store, idea_id, confirm, approved_by, caller)?;
    ready.budget = Some(run_budget.clone());
    ready.estimate = Some(est);
    ready.cost_cap_note = cost_cap_note;

    if settings.approval_grace_secs > 0 {
        save_pending_run(
            store,
            idea_id,
            &PendingRunConfig {
                deploy_requested: options.deploy_requested,
                workflow: Some(resolved_wf.name),
                budget: Some(run_budget.into()),
            },
        )?;
        ready.message = "Ready for development. Run starts after grace period.";
        return Ok(ready);
    }

    let sys = Actor {
        kind: ActorKind::System,
        id: "engine".to_string(),
    };
    match create_run(
        store,
        workspace,
        idea_id,
        CreateRunOptions {
            deploy_requested: options.deploy_requested,
            workflow: Some(resolved_wf.name),
            budget: Some(run_budget.into()),
        },
        &sys,
    ) {
        Ok(run) => {
            if let Ok(Some(updated_idea)) = store.get_idea(idea_id) {
                ready.idea = updated_idea;
            }
            ready.run = Some(run);
            ready.message = "Run queued.";
        }
        Err(err) => {
            ready.run_error = Some(err.to_string());
        }
    }

    Ok(ready)
}

pub fn revoke_ready(
    store: &FactoryStore,
    idea_id: &str,
    user_id: &str,
    caller: &Caller<'_>,
) -> Result<Idea, ApprovalError> {
    require_user(caller)?;
    if !is_valid_idea_id(idea_id) {
        return Err(ApprovalError::InvalidInput(format!(
            "invalid idea id: {idea_id}"
        )));
    }
    remove_pending_run(store, idea_id);
    let actor = Actor {
        kind: ActorKind::User,
        id: user_id.to_string(),
    };
    store
        .execute(idea_id, Command::RevokeReady, &actor)
        .map_err(|e| match e {
            FactoryError::Transition(te) => ApprovalError::Gate(te.to_string()),
            other => ApprovalError::Factory(other),
        })
}

pub fn verify_snapshot(store: &FactoryStore, idea: &Idea) -> Option<SnapshotVerification> {
    let snap = idea.plan_snapshot.as_ref()?;
    let snap_path = store
        .root
        .join("ideas")
        .join(&idea.id)
        .join("artifacts")
        .join("plan-snapshot.md");
    match fs::read(&snap_path) {
        Ok(bytes) => {
            let actual = sha256_hex(&bytes);
            if actual == snap.sha256 {
                Some(SnapshotVerification {
                    version: snap.version,
                    sha256: snap.sha256.clone(),
                    verified: true,
                    error: None,
                })
            } else {
                Some(SnapshotVerification {
                    version: snap.version,
                    sha256: snap.sha256.clone(),
                    verified: false,
                    error: Some("plan snapshot was modified after approval".to_string()),
                })
            }
        }
        Err(_) => Some(SnapshotVerification {
            version: snap.version,
            sha256: snap.sha256.clone(),
            verified: false,
            error: Some("plan snapshot file missing".to_string()),
        }),
    }
}
