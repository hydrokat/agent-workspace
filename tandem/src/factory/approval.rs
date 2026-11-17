use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::FactoryError;
use super::ids::is_valid_idea_id;
use super::machine::Command;
use super::model::{Actor, ActorKind, ArtifactRef, Idea, IdeaState, Run};
use super::run::{CreateRunOptions, create_run};
use super::store::FactoryStore;
use super::workflow::load_default_workflow;
use super::workflow::parse::load_workflow_file;
use super::workflow::validate::{resolve_workflow_path, validate};

const AGENT_ENV_VARS: &[&str] = &["TANDEM_ROLE", "TANDEM_SESSION_ID", "TANDEM_FACTORY_IDEA"];

#[derive(Debug, thiserror::Error)]
pub enum ApprovalError {
    #[error("only the user can mark an idea ready for development")]
    AgentCaller,
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
            Self::AgentCaller | Self::Gate(_) => 3,
            Self::InvalidInput(_) | Self::Factory(_) | Self::Io(_) => 1,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::AgentCaller => "agent_caller_forbidden",
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

#[derive(Debug, Clone, Serialize)]
pub struct ReadyResult {
    pub idea: Idea,
    pub snapshot: SnapshotVerification,
    pub message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<Run>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_error: Option<String>,
}

pub fn ensure_user_caller(env: &dyn Fn(&str) -> Option<String>) -> Result<(), ApprovalError> {
    for var in AGENT_ENV_VARS {
        if let Some(val) = env(var)
            && !val.trim().is_empty()
        {
            return Err(ApprovalError::AgentCaller);
        }
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::new().chain_update(bytes).finalize();
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn mark_ready(
    store: &FactoryStore,
    idea_id: &str,
    confirm: &str,
    via: &str,
    approved_by: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<ReadyResult, ApprovalError> {
    ensure_user_caller(env)?;

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
    if !matches!(via, "cli" | "tui" | "web") {
        return Err(ApprovalError::InvalidInput(format!(
            "invalid --via value: {via} (expected cli, tui, or web)"
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
    })
}

#[allow(clippy::too_many_arguments)]
pub fn mark_ready_with_workflow(
    store: &FactoryStore,
    workspace: &Path,
    idea_id: &str,
    confirm: &str,
    via: &str,
    approved_by: &str,
    workflow_override: Option<&str>,
    deploy_requested: bool,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<ReadyResult, ApprovalError> {
    ensure_user_caller(env)?;

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
    if !matches!(via, "cli" | "tui" | "web") {
        return Err(ApprovalError::InvalidInput(format!(
            "invalid --via value: {via} (expected cli, tui, or web)"
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
    let resolved_wf_name = if let Some(explicit) = workflow_override {
        let raw = if explicit == "default-delivery" {
            load_default_workflow(workspace)
                .map(|(r, _)| r)
                .map_err(|e| ApprovalError::InvalidInput(e.to_string()))?
        } else {
            let path =
                resolve_workflow_path(workspace, explicit).map_err(ApprovalError::InvalidInput)?;
            load_workflow_file(&path).map_err(|e| ApprovalError::InvalidInput(e.to_string()))?
        };
        let wf = validate(&raw, &settings).map_err(|diags| {
            let msgs: Vec<String> = diags.iter().map(ToString::to_string).collect();
            ApprovalError::InvalidInput(format!("workflow {explicit} invalid: {}", msgs.join("; ")))
        })?;
        wf.name
    } else {
        let rec = plan.workflow.trim();
        let mut chosen = None;
        if !rec.is_empty()
            && rec != "default-delivery"
            && let Ok(path) = resolve_workflow_path(workspace, rec)
            && let Ok(raw) = load_workflow_file(&path)
            && let Ok(wf) = validate(&raw, &settings)
        {
            chosen = Some(wf.name);
        }
        if let Some(name) = chosen {
            name
        } else {
            let (raw, _) = load_default_workflow(workspace)
                .map_err(|e| ApprovalError::InvalidInput(e.to_string()))?;
            let wf = validate(&raw, &settings).map_err(|diags| {
                let msgs: Vec<String> = diags.iter().map(ToString::to_string).collect();
                ApprovalError::InvalidInput(format!(
                    "default workflow invalid: {}",
                    msgs.join("; ")
                ))
            })?;
            wf.name
        }
    };

    let mut ready = mark_ready(store, idea_id, confirm, via, approved_by, env)?;
    let sys = Actor {
        kind: ActorKind::System,
        id: "engine".to_string(),
    };
    match create_run(
        store,
        workspace,
        idea_id,
        CreateRunOptions {
            deploy_requested,
            workflow: Some(resolved_wf_name),
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
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Idea, ApprovalError> {
    ensure_user_caller(env)?;
    if !is_valid_idea_id(idea_id) {
        return Err(ApprovalError::InvalidInput(format!(
            "invalid idea id: {idea_id}"
        )));
    }
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
