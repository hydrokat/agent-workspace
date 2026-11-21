use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::FactoryError;
use super::board::run_is_active;
use super::broker::job::{
    JobInput, JobState, correlation_id, job_dir, jobs_dir, read_job_state, set_job_state,
};
use super::broker::result::{JobResult, JobResultStatus};
use super::events::{append, redact_event, reject_if_symlink, set_private_file_mode};
use super::ids::is_valid_run_id;
use super::machine::EventData;
use super::model::{Actor, ActorKind, FactorySettings, KNOWN_PROVIDERS, Run};
use super::permissions::ToolGrant;
use super::redact::RedactContext;
use super::routing::{
    CliCatalog, Complexity, ModelTier, Route, RoutingContext, Selection, Unroutable, escalate,
    interactive_map_tool, map_agent_definition, route_interactive_selected,
};
use super::run::{RunError, get_run, list_runs};
use super::store::{FactoryStore, current_time, write_atomic};
use super::worker::{LaunchedWorker, WorkerAssignment, WorkerError, WorkerLaunchSpec, launch};
use super::workflow::parse::load_workflow_file;
use super::workflow::schema::{StageDef, Workflow};
use super::workflow::validate::validate;
use crate::adapters::base::{ToolFlag, validate_model_for_cli};
use crate::tmux::client::{PaneKind, TmuxClient, TmuxError};
use crate::tmux::tabs::parse_panes;

pub const MAX_SEND_MESSAGE_BYTES: usize = 64 * 1024;
pub const MAX_RUNS_SCANNED: usize = 200;
pub const MAX_FINISHED_RUNS: usize = 25;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
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
    pub selection: Option<Selection>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Selection>,
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

fn open_assignments_lock(store: &FactoryStore, run_id: &str) -> Result<fs::File, TeamError> {
    let path = store.run_dir(run_id).join("assignments.lock");
    reject_if_symlink(&path)?;
    let file = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)?;
    set_private_file_mode(&path)?;
    Ok(file)
}

pub fn assignments_path(store: &FactoryStore, run_id: &str) -> PathBuf {
    store.run_dir(run_id).join("assignments.json")
}

pub fn load_assignments(store: &FactoryStore, run_id: &str) -> Result<AssignmentsFile, TeamError> {
    if !is_valid_run_id(run_id) {
        return Err(TeamError::InvalidInput(format!("invalid run id: {run_id}")));
    }
    let path = assignments_path(store, run_id);
    reject_if_symlink(&path)?;
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

    let lock = open_assignments_lock(store, input.run_id)?;
    let _guard = crate::coordination::exclusive_lock(&lock)?;
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
        selection: None,
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
                selection: assigned.selection.clone(),
            });
        }
    }

    let fallback_clis: Vec<&str> =
        if !settings.orchestrator_may_choose && settings.default_worker_cli != "auto" {
            ready_clis
                .iter()
                .copied()
                .filter(|&c| c == settings.default_worker_cli)
                .collect()
        } else {
            ready_clis.to_vec()
        };
    if fallback_clis.is_empty() {
        return Err(TeamError::InvalidInput(format!(
            "gate closed: configured default_worker_cli '{}' is not installed and signed in",
            settings.default_worker_cli
        )));
    }
    let (route, selection) = route_interactive_selected(
        stage,
        grant,
        workflow,
        settings,
        &fallback_clis,
        attempt,
        ctx,
    )?;
    let role = route.agent_definition.to_string();
    Ok(ResolvedStageAssignment {
        route,
        role,
        source: AssignmentSource::Default,
        selection,
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
    record_job_started_with_selection(
        store, idea_id, run_id, job_id, stage, attempt, cli, model, role, source, pane_id, actor,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn record_job_started_with_selection(
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
    selection: Option<Selection>,
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
        selection,
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

pub type ModelRunner<'a> =
    &'a dyn Fn(
        &str,
    )
        -> Result<Vec<crate::adapters::base::Model>, crate::adapters::base::AdapterError>;

pub fn adapter_model_runner(
    cli: &str,
) -> Result<Vec<crate::adapters::base::Model>, crate::adapters::base::AdapterError> {
    crate::adapters::adapter_for(cli)
        .map_err(|e| crate::adapters::base::AdapterError::UnknownCli(e.to_string()))?
        .list_models()
}

pub fn discover_catalogs(
    store: &FactoryStore,
    settings: &FactorySettings,
    clis: &[&str],
    runner: ModelRunner<'_>,
) -> Vec<CliCatalog> {
    clis.iter()
        .map(|&cli| CliCatalog {
            cli: cli.to_string(),
            models: catalog_models(store, settings, cli, runner),
        })
        .collect()
}

fn catalog_models(
    store: &FactoryStore,
    settings: &FactorySettings,
    cli: &str,
    runner: ModelRunner<'_>,
) -> Vec<super::model_catalog::CatalogModel> {
    let live = super::model_catalog::discover(cli, settings, store.root_dir(), false, runner);
    super::model_catalog::with_pinned(cli, live, settings)
}

pub fn slice_hints(store: &FactoryStore, run_id: &str) -> (Option<Complexity>, Option<ModelTier>) {
    let path = store.run_dir(run_id).join("artifacts/slices.json");
    if reject_if_symlink(&path).is_err() || !path.is_file() {
        return (None, None);
    }
    let Some(manifest) = fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<super::stages::plan::SlicesManifest>(&raw).ok())
    else {
        return (None, None);
    };
    let complexity = manifest.slices.iter().filter_map(|s| s.complexity).max();
    let required = manifest.slices.iter().filter_map(|s| s.required_tier).max();
    (complexity, required)
}

pub fn record_default_selection(
    store: &FactoryStore,
    run_id: &str,
    stage: &str,
    cli: &str,
    model: &str,
    role: &str,
    selection: &Selection,
) -> Result<(), TeamError> {
    let lock = open_assignments_lock(store, run_id)?;
    let _guard = crate::coordination::exclusive_lock(&lock)?;
    let mut file = load_assignments(store, run_id)?;
    if file
        .stages
        .get(stage)
        .is_some_and(|a| a.source != AssignmentSource::Default)
    {
        return Ok(());
    }
    file.stages.insert(
        stage.to_string(),
        StageAssignment {
            stage: stage.to_string(),
            cli: cli.to_string(),
            model: model.to_string(),
            role: role.to_string(),
            source: AssignmentSource::Default,
            updated: current_time(),
            updated_by: "default".to_string(),
            selection: Some(selection.clone()),
        },
    );
    save_assignments(store, run_id, &file)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StageRecommendation {
    pub run: String,
    pub stage: String,
    pub cli: String,
    pub model: String,
    pub role: String,
    pub tier: ModelTier,
    pub selection: Option<Selection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<StageAssignment>,
}

pub struct RecommendInput<'a> {
    pub run_id: &'a str,
    pub stage: &'a str,
    pub complexity: Option<Complexity>,
    pub required_tier: Option<ModelTier>,
    pub ready_clis: &'a [&'a str],
}

pub fn recommend_stage(
    store: &FactoryStore,
    input: RecommendInput<'_>,
    runner: ModelRunner<'_>,
) -> Result<StageRecommendation, TeamError> {
    if !is_valid_run_id(input.run_id) {
        return Err(TeamError::InvalidInput(format!(
            "invalid run id: {}",
            input.run_id
        )));
    }
    let run = get_run(store, input.run_id)?
        .ok_or_else(|| TeamError::NotFound(format!("run {} not found", input.run_id)))?;
    let settings = store.settings()?;
    let wf_path = store.run_dir(input.run_id).join("workflow.yaml");
    reject_if_symlink(&wf_path)?;
    let raw_wf = load_workflow_file(&wf_path)
        .map_err(|e| TeamError::InvalidInput(format!("workflow invalid: {e}")))?;
    let wf = validate(&raw_wf, &settings).map_err(|diags| {
        let msgs: Vec<String> = diags.iter().map(ToString::to_string).collect();
        TeamError::InvalidInput(format!("workflow invalid: {}", msgs.join("; ")))
    })?;
    let stage_def = wf
        .stages
        .iter()
        .find(|s| s.name == input.stage)
        .ok_or_else(|| TeamError::InvalidInput(format!("stage '{}' not found", input.stage)))?;
    let approvals = super::protected::load_run_approvals(store, input.run_id)?;
    let matching_approval = super::protected::find_matching_approval(
        &approvals,
        input.run_id,
        input.stage,
        &run.workflow_sha256,
        current_time(),
    )
    .is_some();
    let grant = super::permissions::grant(
        stage_def.agent,
        stage_def,
        run.target.as_deref().unwrap_or("workspace"),
        &wf,
        &settings,
        matching_approval,
    )
    .map_err(|e| TeamError::Forbidden(e.to_string()))?;

    let (slice_complexity, slice_tier) = slice_hints(store, input.run_id);
    let (avoid_provider, avoid_model) = if input.stage == "review" {
        super::executor::last_stage_provider_model(store, &run, "develop")
    } else {
        (None, None)
    };
    let ctx = RoutingContext {
        preferred_provider: (settings.default_worker_cli != "auto")
            .then(|| settings.default_worker_cli.clone()),
        is_ui_work: super::runner::has_ui_slices(store, input.run_id),
        avoid_provider,
        avoid_model,
        complexity: input.complexity.or(slice_complexity),
        required_tier: input.required_tier.or(slice_tier),
        catalog: discover_catalogs(store, &settings, input.ready_clis, runner),
    };
    let (route, selection) =
        route_interactive_selected(stage_def, &grant, &wf, &settings, input.ready_clis, 1, &ctx)?;
    let current = load_assignments(store, input.run_id)?
        .stages
        .get(input.stage)
        .cloned();
    Ok(StageRecommendation {
        run: input.run_id.to_string(),
        stage: input.stage.to_string(),
        cli: route.provider.clone(),
        model: route.model.clone(),
        role: route.agent_definition.to_string(),
        tier: route.tier,
        selection,
        current,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct EscalationSuggestion {
    pub cli: String,
    pub model: String,
    pub selection: Selection,
    pub applies_automatically: bool,
}

pub fn suggest_escalation(
    store: &FactoryStore,
    run_id: &str,
    job_id: &str,
    runner: ModelRunner<'_>,
) -> Result<Option<EscalationSuggestion>, TeamError> {
    let (stage, attempt) = parse_job_id(run_id, job_id)?;
    let jdir = job_dir(store, run_id, &stage, attempt);
    let result_path = jdir.join("result.json");
    reject_if_symlink(&result_path)?;
    let failed = fs::read_to_string(&result_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<JobResult>(&raw).ok())
        .is_some_and(|r| r.status == JobResultStatus::Failed);
    if !failed {
        return Ok(None);
    }
    let input_path = jdir.join("input.json");
    reject_if_symlink(&input_path)?;
    let modified = |path: &Path| fs::metadata(path).and_then(|m| m.modified()).ok();
    if let (Some(result_at), Some(input_at)) = (modified(&result_path), modified(&input_path))
        && result_at < input_at
    {
        return Ok(None);
    }
    let input: JobInput = serde_json::from_str(&fs::read_to_string(&input_path)?)?;
    let settings = store.settings()?;
    let models =
        super::model_catalog::discover(&input.provider, &settings, store.root_dir(), false, runner);
    let Some((next, selection)) = escalate(&models, &input.model) else {
        return Ok(None);
    };
    let assignments = load_assignments(store, run_id)?;
    let applies_automatically = match assignments.stages.get(&stage) {
        Some(a) if a.source == AssignmentSource::User => false,
        Some(a) if a.source == AssignmentSource::Orchestrator => !settings.orchestrator_may_choose,
        _ => true,
    };
    Ok(Some(EscalationSuggestion {
        cli: input.provider,
        model: next.id,
        selection,
        applies_automatically,
    }))
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
    reject_if_symlink(&pane_path)?;
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
    let live_panes = worker_panes(tmux).remove(run_id).unwrap_or_default();
    agents_for_run(store, run_id, &live_panes)
}

type RunPanes = BTreeMap<String, (String, bool)>;

fn worker_panes(tmux: &TmuxClient) -> HashMap<String, RunPanes> {
    let mut by_run: HashMap<String, RunPanes> = HashMap::new();
    if let Ok(res) = tmux.run(&["list-panes", "-a", "-F", crate::tmux::tabs::FORMAT])
        && res.code == 0
    {
        for p in parse_panes(&res.out) {
            if PaneKind::from_tag(&p.kind) == PaneKind::FactoryWorker {
                by_run
                    .entry(p.run.clone())
                    .or_default()
                    .insert(p.job.clone(), (p.id, !p.dead));
            }
        }
    }
    by_run
}

fn agents_for_run(
    store: &FactoryStore,
    run_id: &str,
    live_panes: &RunPanes,
) -> Result<Vec<AgentSummary>, TeamError> {
    let assignments = load_assignments(store, run_id)?;
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

            let input_path = dir.join("input.json");
            let input: Option<JobInput> = if reject_if_symlink(&input_path).is_ok() {
                fs::read_to_string(&input_path)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
            } else {
                None
            };
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
            let selection = assignments
                .stages
                .get(&stage)
                .and_then(|a| a.selection.clone());
            let job_state = read_job_state(&dir)
                .map(|s| s.as_str().to_string())
                .unwrap_or_else(|_| "unknown".to_string());
            let pane_id_path = dir.join("pane_id");
            let pane_from_file = if reject_if_symlink(&pane_id_path).is_ok() {
                fs::read_to_string(&pane_id_path)
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            } else {
                None
            };
            let (pane_id, pane_alive) = match live_panes.get(&name) {
                Some((id, alive)) => (Some(id.clone()), *alive),
                None => (pane_from_file, false),
            };
            let activity_path = dir.join("activity.json");
            let activity: Option<WorkerActivity> = if reject_if_symlink(&activity_path).is_ok() {
                fs::read_to_string(&activity_path)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
            } else {
                None
            };
            let result_path = dir.join("result.json");
            let has_result = reject_if_symlink(&result_path).is_ok() && result_path.is_file();
            let result_status = if has_result {
                fs::read_to_string(&result_path)
                    .ok()
                    .and_then(|s| serde_json::from_str::<JobResult>(&s).ok())
                    .map(|r| r.status)
            } else {
                None
            };

            out.push(AgentSummary {
                job_id: name,
                run_id: run_id.to_string(),
                stage,
                attempt,
                cli,
                model,
                role,
                source,
                selection,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlobalAgentSummary {
    pub idea_id: String,
    pub idea_title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<ModelTier>,
    #[serde(flatten)]
    pub agent: AgentSummary,
}

pub fn select_runs(runs: &[Run], active_only: bool) -> Vec<&Run> {
    let mut active: Vec<&Run> = runs.iter().filter(|r| run_is_active(r.state)).collect();
    active.sort_by(|a, b| b.updated.total_cmp(&a.updated));
    active.truncate(MAX_RUNS_SCANNED);
    if active_only {
        return active;
    }
    let mut finished: Vec<&Run> = runs.iter().filter(|r| !run_is_active(r.state)).collect();
    finished.sort_by(|a, b| b.updated.total_cmp(&a.updated));
    finished.truncate(MAX_FINISHED_RUNS.min(MAX_RUNS_SCANNED.saturating_sub(active.len())));
    active.extend(finished);
    active
}

pub fn list_all_agents(
    store: &FactoryStore,
    tmux: &TmuxClient,
    active_only: bool,
) -> Result<Vec<GlobalAgentSummary>, TeamError> {
    let runs = list_runs(store, None)?;
    let selected = select_runs(&runs, active_only);
    let mut panes = if selected.iter().any(|r| run_is_active(r.state)) {
        worker_panes(tmux)
    } else {
        HashMap::new()
    };
    let mut titles: HashMap<String, String> = HashMap::new();
    let mut out = Vec::new();
    for run in selected {
        let live = if run_is_active(run.state) {
            panes.remove(&run.id).unwrap_or_default()
        } else {
            RunPanes::new()
        };
        let title = titles
            .entry(run.idea.clone())
            .or_insert_with(|| {
                store
                    .get_idea(&run.idea)
                    .ok()
                    .flatten()
                    .map(|i| i.title)
                    .unwrap_or_default()
            })
            .clone();
        for agent in agents_for_run(store, &run.id, &live)? {
            if active_only && agent.job_state != JobState::Running.as_str() {
                continue;
            }
            out.push(GlobalAgentSummary {
                idea_id: run.idea.clone(),
                idea_title: title.clone(),
                tier: agent.selection.as_ref().map(|s| s.selected_tier),
                agent,
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
    reject_if_symlink(file_path)?;
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
    relaunch_agent_with_env(store, tmux, run_id, job_id, std::env::vars().collect())
}

pub fn relaunch_agent_with_env(
    store: &FactoryStore,
    tmux: &TmuxClient,
    run_id: &str,
    job_id: &str,
    env: BTreeMap<String, String>,
) -> Result<LaunchedWorker, TeamError> {
    relaunch_agent_with_runner(store, tmux, run_id, job_id, env, &adapter_model_runner)
}

pub fn relaunch_agent_with_runner(
    store: &FactoryStore,
    tmux: &TmuxClient,
    run_id: &str,
    job_id: &str,
    env: BTreeMap<String, String>,
    runner: ModelRunner<'_>,
) -> Result<LaunchedWorker, TeamError> {
    if !is_valid_run_id(run_id) {
        return Err(TeamError::InvalidInput(format!("invalid run id: {run_id}")));
    }
    let run = get_run(store, run_id)?
        .ok_or_else(|| TeamError::NotFound(format!("run {run_id} not found")))?;
    let (stage, attempt) = parse_job_id(run_id, job_id)?;
    let jdir = job_dir(store, run_id, &stage, attempt);
    let input_path = jdir.join("input.json");
    reject_if_symlink(&input_path)?;
    let input_raw = fs::read_to_string(&input_path)
        .map_err(|_| TeamError::NotFound(format!("job input.json not found for '{job_id}'")))?;
    let input: JobInput = serde_json::from_str(&input_raw)?;

    let ws_root = store
        .root
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| TeamError::InvalidInput("invalid factory store root".to_string()))?;
    let cwd_path = PathBuf::from(&input.cwd);
    let cwd_meta = fs::symlink_metadata(&cwd_path)
        .map_err(|e| TeamError::InvalidInput(format!("invalid job cwd {}: {e}", input.cwd)))?;
    if cwd_meta.file_type().is_symlink() || !cwd_meta.is_dir() {
        return Err(TeamError::InvalidInput(format!(
            "job cwd must be a real directory: {}",
            input.cwd
        )));
    }
    let canon_ws = ws_root.canonicalize()?;
    let canon_cwd = cwd_path.canonicalize()?;
    if run.worktree_path.is_some() || canon_cwd != canon_ws {
        super::broker::spawn::validate_worktree(ws_root, &cwd_path)
            .map_err(|e| TeamError::InvalidInput(e.to_string()))?;
    }

    let settings = store.settings()?;
    let assignments = load_assignments(store, run_id)?;
    let (cli, model, role, source) = match assignments.stages.get(&stage) {
        Some(a)
            if a.source == AssignmentSource::User
                || (a.source == AssignmentSource::Orchestrator
                    && settings.orchestrator_may_choose) =>
        {
            (a.cli.clone(), a.model.clone(), a.role.clone(), a.source)
        }
        _ => (
            input.provider.clone(),
            input.model.clone(),
            input.agent.clone(),
            AssignmentSource::Default,
        ),
    };
    let mut escalation: Option<Selection> = None;
    let (cli, model) = if source == AssignmentSource::Default {
        match suggest_escalation(store, run_id, job_id, runner)? {
            Some(s)
                if s.applies_automatically && validate_model_for_cli(&s.cli, &s.model).is_ok() =>
            {
                escalation = Some(s.selection.clone());
                (s.cli, s.model)
            }
            _ => (cli, model),
        }
    } else {
        (cli, model)
    };

    let wf_path = store.run_dir(run_id).join("workflow.yaml");
    reject_if_symlink(&wf_path)?;
    let raw_wf = load_workflow_file(&wf_path)
        .map_err(|e| TeamError::InvalidInput(format!("workflow invalid for relaunch: {e}")))?;
    let wf = validate(&raw_wf, &settings).map_err(|diags| {
        let msgs: Vec<String> = diags.iter().map(ToString::to_string).collect();
        TeamError::InvalidInput(format!(
            "workflow invalid for relaunch: {}",
            msgs.join("; ")
        ))
    })?;
    let st_def =
        wf.stages.iter().find(|s| s.name == stage).ok_or_else(|| {
            TeamError::InvalidInput(format!("stage '{stage}' missing in workflow"))
        })?;
    let run_approvals = super::protected::load_run_approvals(store, run_id)?;
    let matching_approval = super::protected::find_matching_approval(
        &run_approvals,
        run_id,
        &stage,
        &run.workflow_sha256,
        current_time(),
    )
    .is_some();
    let grant = super::permissions::grant(
        st_def.agent,
        st_def,
        run.target.as_deref().unwrap_or("workspace"),
        &wf,
        &settings,
        matching_approval,
    )
    .map_err(|e| TeamError::Forbidden(e.to_string()))?;
    let read_only = !grant.has_repo_write;
    let mut tools: Vec<ToolFlag> = Vec::new();
    for &tool in &grant.tools {
        interactive_map_tool(&cli, tool, &mut tools)?;
    }

    let mut updated_input = input.clone();
    updated_input.provider = cli.clone();
    updated_input.model = model.clone();
    updated_input.agent = role.clone();
    let updated_input_bytes = serde_json::to_vec_pretty(&updated_input)?;
    write_atomic(&input_path, &updated_input_bytes)?;

    let old_pane = resolve_job_pane_id(store, tmux, run_id, job_id, &stage, attempt).ok();

    set_job_state(&jdir, JobState::Running).map_err(|e| TeamError::InvalidInput(e.to_string()))?;

    let spec = WorkerLaunchSpec {
        run_id: run_id.to_string(),
        stage: stage.clone(),
        attempt,
        job_dir: jdir.clone(),
        prompt_file: jdir.join("prompt.md"),
        result_path: jdir.join("result.json"),
        worktree_path: cwd_path,
        assignment: WorkerAssignment {
            cli: cli.clone(),
            model: model.clone(),
            read_only,
            tools,
            agent: role.clone(),
            role: role.clone(),
        },
        env,
        time_limit_secs: input.time_limit_secs,
    };
    let launched = launch(tmux, &spec)?;
    if let Some(old) = old_pane
        && old != launched.pane_id
        && tmux.capture_worker(run_id, &old, 1).is_ok()
    {
        let _ = tmux.kill_pane(&old);
    }
    let actor = Actor {
        kind: ActorKind::Orchestrator,
        id: "team".to_string(),
    };
    record_job_started_with_selection(
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
        escalation.clone(),
    )?;
    if let Some(selection) = &escalation {
        record_default_selection(store, run_id, &stage, &cli, &model, &role, selection)?;
    }
    Ok(launched)
}
