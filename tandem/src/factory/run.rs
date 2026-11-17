use std::cmp::Ordering;
use std::fs::{self, OpenOptions};
use std::path::Path;

use super::FactoryError;
use super::approval::verify_snapshot;
use super::events::{Event, append_locked, read_all, reject_if_symlink, set_private_file_mode};
use super::ids::{is_valid_idea_id, is_valid_run_id, new_run_id};
use super::machine::{self, Command, EventData, TransitionError};
use super::model::{Actor, IdeaState, Run, RunState, Stage, StageState};
use super::store::{FactoryStore, create_private_dir, current_time, event_run_id, write_atomic};
use super::workflow::parse::load_workflow_file;
use super::workflow::validate::{resolve_workflow_path, validate};
use super::workflow::{WorkflowSource, load_default_workflow};
use crate::coordination::filelock::exclusive_lock;

#[derive(Debug, Clone, Default)]
pub struct CreateRunOptions {
    pub deploy_requested: bool,
    pub workflow: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("idea is not ready for development: {0}")]
    NotReady(String),
    #[error("plan snapshot verification failed: {0}")]
    SnapshotInvalid(String),
    #[error("workflow validation failed: {0}")]
    WorkflowInvalid(String),
    #[error("cannot withdraw run: {0}")]
    StageAlreadyStarted(String),
    #[error(transparent)]
    Transition(#[from] TransitionError),
    #[error(transparent)]
    Factory(#[from] FactoryError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl RunError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::NotReady(_) | Self::StageAlreadyStarted(_) => "gate_not_met",
            Self::SnapshotInvalid(_) => "snapshot_invalid",
            Self::WorkflowInvalid(_) => "workflow_invalid",
            Self::Transition(te) => match te {
                TransitionError::NotFound => "not_found",
                TransitionError::ActorNotAllowed { .. } => "actor_not_allowed",
                TransitionError::IllegalTransition { .. } => "illegal_transition",
                TransitionError::GateNotMet { .. } => "gate_not_met",
                TransitionError::InvalidInput(_) | TransitionError::AlreadyExists => {
                    "invalid_input"
                }
            },
            Self::Factory(FactoryError::InvalidInput(_)) => "invalid_input",
            Self::Factory(_) | Self::Io(_) => "internal_error",
        }
    }
}

pub fn create_run(
    store: &FactoryStore,
    workspace: &Path,
    idea_id: &str,
    options: CreateRunOptions,
    actor: &Actor,
) -> Result<Run, RunError> {
    if !is_valid_idea_id(idea_id) {
        return Err(RunError::Factory(FactoryError::InvalidInput(format!(
            "invalid idea id: {idea_id}"
        ))));
    }

    store.ensure()?;
    let lock_path = store.lock_path();
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    set_private_file_mode(&lock_path)?;
    let _guard = exclusive_lock(&lock_file)?;

    let idea = store
        .load_idea_for_update(idea_id)?
        .ok_or_else(|| RunError::NotFound(format!("idea {idea_id}")))?;

    if idea.state != IdeaState::ReadyForDevelopment {
        return Err(RunError::NotReady(format!(
            "idea {idea_id} is in state {}",
            idea.state.label()
        )));
    }

    let snap = verify_snapshot(store, &idea)
        .ok_or_else(|| RunError::SnapshotInvalid("plan snapshot missing".to_string()))?;
    if !snap.verified {
        return Err(RunError::SnapshotInvalid(snap.error.unwrap_or_else(|| {
            "plan snapshot verification failed".to_string()
        })));
    }

    let (raw, source) = match options.workflow.as_deref() {
        None | Some("default-delivery") => load_default_workflow(workspace)
            .map_err(|e| RunError::WorkflowInvalid(e.to_string()))?,
        Some(name) => {
            let path = resolve_workflow_path(workspace, name).map_err(RunError::WorkflowInvalid)?;
            let raw =
                load_workflow_file(&path).map_err(|e| RunError::WorkflowInvalid(e.to_string()))?;
            (raw, WorkflowSource::Workspace)
        }
    };

    let settings = store.settings()?;
    let wf = validate(&raw, &settings).map_err(|diags| {
        let msgs: Vec<String> = diags.iter().map(ToString::to_string).collect();
        RunError::WorkflowInvalid(msgs.join("; "))
    })?;

    let source_label = match source {
        WorkflowSource::Workspace => "file",
        WorkflowSource::Embedded => "embedded",
    };

    let run_id = new_run_id();
    let stages: Vec<Stage> = wf
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
        })
        .collect();

    let ev_data = EventData::RunCreated {
        run: run_id.clone(),
        workflow: wf.name.clone(),
        version: wf.version.to_string(),
        sha256: raw.sha256.clone(),
        source: source_label.to_string(),
        plan_sha256: snap.sha256,
        deploy_requested: options.deploy_requested,
        stages,
    };

    let ts = current_time();
    let pending = vec![Event::from_data(
        ts,
        actor.clone(),
        idea_id.to_string(),
        Some(run_id.clone()),
        &ev_data,
    )?];
    let appended = append_locked(&store.events_path(), &pending)?;
    let ev_ts = appended[0].ts;

    let run_dir = store.run_dir(&run_id);
    create_private_dir(&run_dir)?;
    write_atomic(&run_dir.join("workflow.yaml"), raw.raw_text.as_bytes())?;

    let run = machine::apply_run(None, &ev_data, idea_id, ev_ts).ok_or_else(|| {
        RunError::Factory(FactoryError::InvalidInput(
            "failed to materialize run".to_string(),
        ))
    })?;
    let run_bytes = serde_json::to_vec_pretty(&run).map_err(FactoryError::from)?;
    write_atomic(&store.run_json_path(&run_id), &run_bytes)?;

    let updated_idea = machine::apply(Some(idea), &ev_data, ev_ts).ok_or_else(|| {
        RunError::Factory(FactoryError::InvalidInput(
            "failed to materialize idea".to_string(),
        ))
    })?;
    let idea_bytes = serde_json::to_vec_pretty(&updated_idea).map_err(FactoryError::from)?;
    write_atomic(&store.idea_path(idea_id), &idea_bytes)?;

    Ok(run)
}

pub fn withdraw_run(store: &FactoryStore, run_id: &str, actor: &Actor) -> Result<Run, RunError> {
    if !is_valid_run_id(run_id) {
        return Err(RunError::Factory(FactoryError::InvalidInput(format!(
            "invalid run id: {run_id}"
        ))));
    }

    store.ensure()?;
    let lock_path = store.lock_path();
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    set_private_file_mode(&lock_path)?;
    let _guard = exclusive_lock(&lock_file)?;

    let run = load_run_from_log(store, run_id)?
        .ok_or_else(|| RunError::NotFound(format!("run {run_id}")))?;

    if run
        .stages
        .iter()
        .any(|s| s.attempts > 0 || s.state != StageState::Pending)
    {
        return Err(RunError::StageAlreadyStarted(
            "at least one stage has already started".to_string(),
        ));
    }
    if run.state != RunState::Queued {
        return Err(RunError::StageAlreadyStarted(format!(
            "run {run_id} is not in queued state"
        )));
    }

    let idea = store
        .load_idea_for_update(&run.idea)?
        .ok_or_else(|| RunError::NotFound(format!("idea {}", run.idea)))?;

    let cmd = Command::WithdrawRun {
        run: run_id.to_string(),
    };
    let decided = machine::decide(Some(&idea), &cmd, actor, &super::machine::Facts::default())?;

    let ts = current_time();
    let mut pending = Vec::with_capacity(decided.len());
    for ev_data in &decided {
        pending.push(Event::from_data(
            ts,
            actor.clone(),
            run.idea.clone(),
            event_run_id(ev_data),
            ev_data,
        )?);
    }
    let appended = append_locked(&store.events_path(), &pending)?;

    let mut next_run = Some(run);
    let mut next_idea = Some(idea);
    for (ev, ev_data) in appended.iter().zip(decided.iter()) {
        next_run = machine::apply_run(next_run, ev_data, &ev.idea, ev.ts);
        next_idea = machine::apply(next_idea, ev_data, ev.ts);
    }

    let updated_run = next_run.ok_or_else(|| {
        RunError::Factory(FactoryError::InvalidInput(
            "withdraw did not produce run state".to_string(),
        ))
    })?;
    let updated_idea = next_idea.ok_or_else(|| {
        RunError::Factory(FactoryError::InvalidInput(
            "withdraw did not produce idea state".to_string(),
        ))
    })?;

    let run_dir = store.run_dir(run_id);
    if !run_dir.exists() {
        create_private_dir(&run_dir)?;
    }
    let run_bytes = serde_json::to_vec_pretty(&updated_run).map_err(FactoryError::from)?;
    write_atomic(&store.run_json_path(run_id), &run_bytes)?;

    let idea_bytes = serde_json::to_vec_pretty(&updated_idea).map_err(FactoryError::from)?;
    write_atomic(&store.idea_path(&updated_idea.id), &idea_bytes)?;

    Ok(updated_run)
}

pub fn get_run(store: &FactoryStore, run_id: &str) -> Result<Option<Run>, RunError> {
    if !is_valid_run_id(run_id) {
        return Err(RunError::Factory(FactoryError::InvalidInput(format!(
            "invalid run id: {run_id}"
        ))));
    }
    reject_if_symlink(&store.root)?;
    reject_if_symlink(&store.runs_dir())?;
    let path = store.run_json_path(run_id);
    reject_if_symlink(&path)?;

    if !store.exists() {
        return Ok(None);
    }
    if let Ok(content) = fs::read_to_string(&path)
        && let Ok(run) = serde_json::from_str::<Run>(&content)
    {
        return Ok(Some(run));
    }
    load_run_from_log(store, run_id)
}

pub fn list_runs(store: &FactoryStore, idea_id: Option<&str>) -> Result<Vec<Run>, RunError> {
    if let Some(id) = idea_id
        && !is_valid_idea_id(id)
    {
        return Err(RunError::Factory(FactoryError::InvalidInput(format!(
            "invalid idea id: {id}"
        ))));
    }
    if !store.exists() {
        return Ok(Vec::new());
    }
    let runs_dir = store.runs_dir();
    if !runs_dir.exists() {
        return Ok(Vec::new());
    }
    reject_if_symlink(&runs_dir)?;

    let mut runs = Vec::new();
    for entry in fs::read_dir(&runs_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(run_id) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !is_valid_run_id(run_id) {
            continue;
        }
        if let Some(run) = get_run(store, run_id)? {
            if let Some(filter) = idea_id
                && run.idea != filter
            {
                continue;
            }
            runs.push(run);
        }
    }
    runs.sort_by(|a, b| b.created.partial_cmp(&a.created).unwrap_or(Ordering::Equal));
    Ok(runs)
}

fn load_run_from_log(store: &FactoryStore, run_id: &str) -> Result<Option<Run>, RunError> {
    let log = read_all(&store.events_path())?;
    let mut acc = None;
    for ev in &log.events {
        let Ok(ev_data) = ev.to_event_data() else {
            continue;
        };
        if event_run_id(&ev_data).as_deref() != Some(run_id) {
            continue;
        }
        acc = machine::apply_run(acc, &ev_data, &ev.idea, ev.ts);
    }
    Ok(acc)
}
