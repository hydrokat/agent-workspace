use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::FactoryError;
use super::broker::job::{
    JobInput, JobState, correlation_id, job_dir, jobs_dir, read_job_state, set_job_state,
};
use super::broker::result::{JobResult, JobResultStatus};
use super::events::{append, redact_event, set_private_file_mode};
use super::ids::is_valid_run_id;
use super::machine::EventData;
use super::model::{Actor, ActorKind, FactorySettings, KNOWN_PROVIDERS, Run};
use super::permissions::ToolGrant;
use super::redact::RedactContext;
use super::routing::{
    ModelTier, Route, RoutingContext, Unroutable, interactive_map_tool, map_agent_definition,
    route_interactive,
};
use super::run::{RunError, get_run};
use super::store::{FactoryStore, current_time, write_atomic};
use super::worker::{LaunchedWorker, WorkerAssignment, WorkerError, WorkerLaunchSpec, launch};
use super::workflow::parse::load_workflow_file;
use super::workflow::schema::{StageDef, Workflow};
use super::workflow::validate::validate;
use crate::adapters::base::{ToolFlag, validate_model_for_cli};
use crate::tmux::client::{PaneKind, TmuxClient, TmuxError};
use crate::tmux::tabs::parse_panes;

pub const MAX_SEND_MESSAGE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentSource {
    User,
    Orchestrator,
    Default,
}

impl AssignmentSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Orchestrator => "orchestrator",
            Self::Default => "default",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageAssignment {
    pub stage: String,
    pub cli: String,
    pub model: String,
    pub role: String,
    pub source: AssignmentSource,
    pub updated: f64,
    pub updated_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AssignmentsFile {
    #[serde(default)]
    pub stages: BTreeMap<String, StageAssignment>,
}

#[derive(Debug, Clone)]
pub struct AssignStageInput<'a> {
    pub run_id: &'a str,
    pub stage: &'a str,
    pub cli: &'a str,
    pub model: &'a str,
    pub role: Option<&'a str>,
    pub by: AssignmentSource,
    pub actor_id: &'a str,
}

#[derive(Debug, Clone)]
pub struct ResolvedStageAssignment {
    pub route: Route,
    pub role: String,
    pub source: AssignmentSource,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerActivity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_stop_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSummary {
    pub job_id: String,
    pub run_id: String,
    pub stage: String,
    pub attempt: u32,
    pub cli: String,
    pub model: String,
    pub role: String,
    pub source: AssignmentSource,
    pub job_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    pub pane_alive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<WorkerActivity>,
    pub has_result: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_status: Option<JobResultStatus>,
}

#[derive(Debug, thiserror::Error)]
pub enum TeamError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    NotFound(String),
    #[error(transparent)]
    Unroutable(#[from] Unroutable),
    #[error(transparent)]
    Tmux(#[from] TmuxError),
    #[error(transparent)]
    Worker(#[from] WorkerError),
    #[error(transparent)]
    Factory(#[from] FactoryError),
    #[error(transparent)]
    Run(#[from] RunError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub fn assignments_path(store: &FactoryStore, run_id: &str) -> PathBuf {
    store.run_dir(run_id).join("assignments.json")
}

pub fn load_assignments(store: &FactoryStore, run_id: &str) -> Result<AssignmentsFile, TeamError> {
    if !is_valid_run_id(run_id) {
        return Err(TeamError::InvalidInput(format!("invalid run id: {run_id}")));
    }
    let path = assignments_path(store, run_id);
    if !path.is_file() {
        return Ok(AssignmentsFile::default());
    }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn save_assignments(
    store: &FactoryStore,
    run_id: &str,
    data: &AssignmentsFile,
) -> Result<(), TeamError> {
    if !is_valid_run_id(run_id) {
        return Err(TeamError::InvalidInput(format!("invalid run id: {run_id}")));
    }
    let path = assignments_path(store, run_id);
    let bytes = serde_json::to_vec_pretty(data)?;
    write_atomic(&path, &bytes)?;
    set_private_file_mode(&path)?;
    Ok(())
}

fn validate_cli(cli: &str) -> Result<(), TeamError> {
    if KNOWN_PROVIDERS.contains(&cli) || cli == "opencode" {
        return Ok(());
    }
    Err(TeamError::InvalidInput(format!(
        "unsupported CLI '{cli}' (expected claude, agy, or opencode)"
    )))
}

pub fn assign_stage(
    store: &FactoryStore,
    input: AssignStageInput<'_>,
) -> Result<StageAssignment, TeamError> {
    if !is_valid_run_id(input.run_id) {
        return Err(TeamError::InvalidInput(format!(
            "invalid run id: {}",
            input.run_id
        )));
    }
    let run = get_run(store, input.run_id)?
        .ok_or_else(|| TeamError::NotFound(format!("run {} not found", input.run_id)))?;
    if !run.stages.iter().any(|s| s.name == input.stage) {
        return Err(TeamError::InvalidInput(format!(
            "stage '{}' not found in run {}",
            input.stage, input.run_id
        )));
    }
    validate_cli(input.cli)?;
    validate_model_for_cli(input.cli, input.model)
        .map_err(|e| TeamError::InvalidInput(e.to_string()))?;

    let settings = store.settings()?;
    if input.by == AssignmentSource::Orchestrator && !settings.orchestrator_may_choose {
        return Err(TeamError::Forbidden(
            "orchestrator stage assignment is disabled by factory settings (orchestrator_may_choose=false)"
                .to_string(),
        ));
    }

    let mut file = load_assignments(store, input.run_id)?;
    if let Some(existing) = file.stages.get(input.stage)
        && existing.source == AssignmentSource::User
        && input.by == AssignmentSource::Orchestrator
    {
        return Err(TeamError::Forbidden(format!(
            "stage '{}' has an explicit user assignment ({}/{}); pass --by user to override",
            input.stage, existing.cli, existing.model
        )));
    }

    let role = if let Some(r) = input.role {
        if r.trim().is_empty() {
            "factory-worker".to_string()
        } else {
            r.trim().to_string()
        }
    } else if let Ok(raw) = load_workflow_file(&store.run_dir(input.run_id).join("workflow.yaml"))
        && let Ok(wf) = validate(&raw, &settings)
        && let Some(st) = wf.stages.iter().find(|s| s.name == input.stage)
    {
        map_agent_definition(st.agent, false).to_string()
    } else {
        "factory-worker".to_string()
    };

    let assignment = StageAssignment {
        stage: input.stage.to_string(),
        cli: input.cli.to_string(),
        model: input.model.to_string(),
        role,
        source: input.by,
        updated: current_time(),
        updated_by: input.actor_id.to_string(),
    };
    file.stages
        .insert(input.stage.to_string(), assignment.clone());
    save_assignments(store, input.run_id, &file)?;
    Ok(assignment)
}

#[allow(clippy::too_many_arguments)]
pub fn resolve_stage_assignment(
    store: &FactoryStore,
    run: &Run,
    stage: &StageDef,
    grant: &ToolGrant,
    workflow: &Workflow,
    settings: &FactorySettings,
    ready_clis: &[&str],
    attempt: u32,
    ctx: &RoutingContext,
) -> Result<ResolvedStageAssignment, TeamError> {
    let file = load_assignments(store, &run.id)?;
    if let Some(assigned) = file.stages.get(&stage.name) {
        let allowed_by_settings = assigned.source == AssignmentSource::User
            || (assigned.source == AssignmentSource::Orchestrator
                && settings.orchestrator_may_choose);
        if allowed_by_settings && assigned.source != AssignmentSource::Default {
            if !ready_clis.contains(&assigned.cli.as_str()) {
                return Err(TeamError::InvalidInput(format!(
                    "assigned CLI '{}' for stage '{}' is not installed and signed in",
                    assigned.cli, stage.name
                )));
            }
            validate_model_for_cli(&assigned.cli, &assigned.model)
                .map_err(|e| TeamError::InvalidInput(e.to_string()))?;
            let mut tool_flags = Vec::new();
            for &tool in &grant.tools {
                interactive_map_tool(&assigned.cli, tool, &mut tool_flags)?;
            }
            if assigned.cli == "opencode" && !grant.has_repo_write {
                return Err(Unroutable::UnsupportedTool {
                    provider: assigned.cli.clone(),
                    tool: crate::factory::workflow::schema::WorkflowTool::DiffReview,
                }
                .into());
            }
            let route = Route {
                agent_definition: map_agent_definition(stage.agent, ctx.is_ui_work),
                provider: assigned.cli.clone(),
                tier: ModelTier::Standard,
                upgraded_from: None,
                model: assigned.model.clone(),
                tool_flags,
                temperature: super::routing::TemperatureSupport::Unsupported,
            };
            return Ok(ResolvedStageAssignment {
                route,
                role: assigned.role.clone(),
                source: assigned.source,
            });
        }
    }

    let route = route_interactive(stage, grant, workflow, settings, ready_clis, attempt, ctx)?;
    let role = route.agent_definition.to_string();
    Ok(ResolvedStageAssignment {
        route,
        role,
        source: AssignmentSource::Default,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn record_job_started(
    store: &FactoryStore,
    idea_id: &str,
    run_id: &str,
    job_id: &str,
    stage: &str,
    attempt: u32,
    cli: &str,
    model: &str,
    role: &str,
    source: AssignmentSource,
    pane_id: &str,
    actor: &Actor,
) -> Result<(), FactoryError> {
    store.ensure()?;
    let ev_data = EventData::JobStarted {
        run: run_id.to_string(),
        job: job_id.to_string(),
        stage: stage.to_string(),
        attempt,
        cli: cli.to_string(),
        model: model.to_string(),
        role: role.to_string(),
        source,
        pane_id: pane_id.to_string(),
    };
    let ev = super::events::Event::from_data(
        current_time(),
        actor.clone(),
        idea_id.to_string(),
        Some(run_id.to_string()),
        &ev_data,
    )?;
    let redacted = redact_event(ev, &RedactContext::from_env());
    append(&store.events_path(), &store.lock_path(), &[redacted])?;
    Ok(())
}

fn parse_job_id(run_id: &str, job_id: &str) -> Result<(String, u32), TeamError> {
    let prefix = format!("{run_id}.");
    let Some(rest) = job_id.strip_prefix(&prefix) else {
        return Err(TeamError::InvalidInput(format!(
            "job '{job_id}' does not belong to run '{run_id}'"
        )));
    };
    let Some((stage, attempt_str)) = rest.rsplit_once('.') else {
        return Err(TeamError::InvalidInput(format!("invalid job id: {job_id}")));
    };
    let attempt = attempt_str
        .parse::<u32>()
        .map_err(|_| TeamError::InvalidInput(format!("invalid job attempt in '{job_id}'")))?;
    if stage.is_empty() || attempt == 0 {
        return Err(TeamError::InvalidInput(format!("invalid job id: {job_id}")));
    }
    Ok((stage.to_string(), attempt))
}

fn resolve_job_pane_id(
    store: &FactoryStore,
    tmux: &TmuxClient,
    run_id: &str,
    job_id: &str,
    stage: &str,
    attempt: u32,
) -> Result<String, TeamError> {
    let dir = job_dir(store, run_id, stage, attempt);
    let pane_path = dir.join("pane_id");
    if pane_path.is_file()
        && let Ok(raw) = fs::read_to_string(&pane_path)
    {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    if let Ok(res) = tmux.run(&["list-panes", "-a", "-F", crate::tmux::tabs::FORMAT])
        && res.code == 0
    {
        for pane in parse_panes(&res.out) {
            if PaneKind::from_tag(&pane.kind) == PaneKind::FactoryWorker
                && pane.run == run_id
                && pane.job == job_id
            {
                return Ok(pane.id);
            }
        }
    }
    Err(TeamError::NotFound(format!(
        "no worker pane found for job '{job_id}'"
    )))
}

pub fn list_agents(
    store: &FactoryStore,
    tmux: &TmuxClient,
    run_id: &str,
) -> Result<Vec<AgentSummary>, TeamError> {
    if !is_valid_run_id(run_id) {
        return Err(TeamError::InvalidInput(format!("invalid run id: {run_id}")));
    }
    let _ = get_run(store, run_id)?
        .ok_or_else(|| TeamError::NotFound(format!("run {run_id} not found")))?;
    let assignments = load_assignments(store, run_id)?;

    let mut live_panes: BTreeMap<String, (String, bool)> = BTreeMap::new();
    if let Ok(res) = tmux.run(&["list-panes", "-a", "-F", crate::tmux::tabs::FORMAT])
        && res.code == 0
    {
        for p in parse_panes(&res.out) {
            if PaneKind::from_tag(&p.kind) == PaneKind::FactoryWorker && p.run == run_id {
                live_panes.insert(p.job.clone(), (p.id, !p.dead));
            }
        }
    }

    let j_root = jobs_dir(store, run_id);
    let mut out = Vec::new();
    if j_root.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(&j_root)?.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let Ok((stage, attempt)) = parse_job_id(run_id, &name) else {
                continue;
            };

            let input: Option<JobInput> = fs::read_to_string(dir.join("input.json"))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok());
            let cli = input
                .as_ref()
                .map(|i| i.provider.clone())
                .unwrap_or_default();
            let model = input.as_ref().map(|i| i.model.clone()).unwrap_or_default();
            let role = input.as_ref().map(|i| i.agent.clone()).unwrap_or_default();
            let source = assignments
                .stages
                .get(&stage)
                .map(|a| a.source)
                .unwrap_or(AssignmentSource::Default);
            let job_state = read_job_state(&dir)
                .map(|s| s.as_str().to_string())
                .unwrap_or_else(|_| "unknown".to_string());
            let pane_from_file = fs::read_to_string(dir.join("pane_id"))
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let (pane_id, pane_alive) = match live_panes.get(&name) {
                Some((id, alive)) => (Some(id.clone()), *alive),
                None => (pane_from_file, false),
            };
            let activity: Option<WorkerActivity> = fs::read_to_string(dir.join("activity.json"))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok());
            let result_path = dir.join("result.json");
            let has_result = result_path.is_file();
            let result_status = fs::read_to_string(&result_path)
                .ok()
                .and_then(|s| serde_json::from_str::<JobResult>(&s).ok())
                .map(|r| r.status);

            out.push(AgentSummary {
                job_id: name,
                run_id: run_id.to_string(),
                stage,
                attempt,
                cli,
                model,
                role,
                source,
                job_state,
                pane_id,
                pane_alive,
                activity,
                has_result,
                result_status,
            });
        }
    }
    Ok(out)
}

pub fn send_agent_message(
    store: &FactoryStore,
    tmux: &TmuxClient,
    run_id: &str,
    job_id: &str,
    file_path: &Path,
) -> Result<(), TeamError> {
    if !is_valid_run_id(run_id) {
        return Err(TeamError::InvalidInput(format!("invalid run id: {run_id}")));
    }
    let (stage, attempt) = parse_job_id(run_id, job_id)?;
    let msg = fs::read_to_string(file_path)?;
    if msg.trim().is_empty() {
        return Err(TeamError::InvalidInput("message file is empty".to_string()));
    }
    if msg.len() > MAX_SEND_MESSAGE_BYTES {
        return Err(TeamError::InvalidInput(format!(
            "message exceeds {} byte limit",
            MAX_SEND_MESSAGE_BYTES
        )));
    }
    let pane_id = resolve_job_pane_id(store, tmux, run_id, job_id, &stage, attempt)?;
    tmux.paste_worker(run_id, &pane_id, &msg)?;
    Ok(())
}

pub fn capture_agent_output(
    store: &FactoryStore,
    tmux: &TmuxClient,
    run_id: &str,
    job_id: &str,
    lines: usize,
) -> Result<String, TeamError> {
    if !is_valid_run_id(run_id) {
        return Err(TeamError::InvalidInput(format!("invalid run id: {run_id}")));
    }
    let (stage, attempt) = parse_job_id(run_id, job_id)?;
    let pane_id = resolve_job_pane_id(store, tmux, run_id, job_id, &stage, attempt)?;
    Ok(tmux.capture_worker(run_id, &pane_id, lines)?)
}

pub fn terminate_agent(
    store: &FactoryStore,
    tmux: &TmuxClient,
    run_id: &str,
    job_id: &str,
    confirm: &str,
) -> Result<(), TeamError> {
    if !is_valid_run_id(run_id) {
        return Err(TeamError::InvalidInput(format!("invalid run id: {run_id}")));
    }
    if confirm != job_id {
        return Err(TeamError::InvalidInput(format!(
            "confirmation mismatch: pass --confirm {job_id}"
        )));
    }
    let (stage, attempt) = parse_job_id(run_id, job_id)?;
    let pane_id = resolve_job_pane_id(store, tmux, run_id, job_id, &stage, attempt)?;
    tmux.capture_worker(run_id, &pane_id, 1)?;
    tmux.kill_pane(&pane_id)?;
    let jdir = job_dir(store, run_id, &stage, attempt);
    if jdir.is_dir()
        && matches!(
            read_job_state(&jdir),
            Ok(JobState::Running | JobState::Prepared)
        )
    {
        set_job_state(&jdir, JobState::Interrupted)
            .map_err(|e| TeamError::InvalidInput(e.to_string()))?;
    }
    Ok(())
}

pub fn relaunch_agent(
    store: &FactoryStore,
    tmux: &TmuxClient,
    run_id: &str,
    job_id: &str,
) -> Result<LaunchedWorker, TeamError> {
    if !is_valid_run_id(run_id) {
        return Err(TeamError::InvalidInput(format!("invalid run id: {run_id}")));
    }
    let run = get_run(store, run_id)?
        .ok_or_else(|| TeamError::NotFound(format!("run {run_id} not found")))?;
    let (stage, attempt) = parse_job_id(run_id, job_id)?;
    let jdir = job_dir(store, run_id, &stage, attempt);
    let input_raw = fs::read_to_string(jdir.join("input.json"))
        .map_err(|_| TeamError::NotFound(format!("job input.json not found for '{job_id}'")))?;
    let input: JobInput = serde_json::from_str(&input_raw)?;

    if let Ok(old_pane) = resolve_job_pane_id(store, tmux, run_id, job_id, &stage, attempt)
        && tmux.capture_worker(run_id, &old_pane, 1).is_ok()
    {
        let _ = tmux.kill_pane(&old_pane);
    }

    let settings = store.settings()?;
    let assignments = load_assignments(store, run_id)?;
    let (cli, model, role, source) = if let Some(a) = assignments.stages.get(&stage) {
        (a.cli.clone(), a.model.clone(), a.role.clone(), a.source)
    } else {
        (
            input.provider.clone(),
            input.model.clone(),
            input.agent.clone(),
            AssignmentSource::Default,
        )
    };

    let mut tools: Vec<ToolFlag> = Vec::new();
    let mut read_only = true;
    if let Ok(raw_wf) = load_workflow_file(&store.run_dir(run_id).join("workflow.yaml"))
        && let Ok(wf) = validate(&raw_wf, &settings)
        && let Some(st_def) = wf.stages.iter().find(|s| s.name == stage)
        && let Ok(grant) = super::permissions::grant(
            st_def.agent,
            st_def,
            run.target.as_deref().unwrap_or("workspace"),
            &wf,
            &settings,
            false,
        )
    {
        read_only = !grant.has_repo_write;
        for &tool in &grant.tools {
            interactive_map_tool(&cli, tool, &mut tools)?;
        }
    }

    set_job_state(&jdir, JobState::Running).map_err(|e| TeamError::InvalidInput(e.to_string()))?;

    let spec = WorkerLaunchSpec {
        run_id: run_id.to_string(),
        stage: stage.clone(),
        attempt,
        job_dir: jdir.clone(),
        prompt_file: jdir.join("prompt.md"),
        result_path: jdir.join("result.json"),
        worktree_path: PathBuf::from(&input.cwd),
        assignment: WorkerAssignment {
            cli: cli.clone(),
            model: model.clone(),
            read_only,
            tools,
            agent: role.clone(),
            role: role.clone(),
        },
        env: std::env::vars().collect(),
        time_limit_secs: input.time_limit_secs,
    };
    let launched = launch(tmux, &spec)?;
    let actor = Actor {
        kind: ActorKind::Orchestrator,
        id: "team".to_string(),
    };
    let _ = record_job_started(
        store,
        &run.idea,
        run_id,
        &correlation_id(run_id, &stage, attempt),
        &stage,
        attempt,
        &cli,
        &model,
        &role,
        source,
        &launched.pane_id,
        &actor,
    );
    Ok(launched)
}
