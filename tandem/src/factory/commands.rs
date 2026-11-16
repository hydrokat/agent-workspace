use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use clap::{Args, Subcommand};
use serde::Serialize;

use super::FactoryError;
use super::events::{self, Event};
use super::ids::is_valid_idea_id;
use super::machine::TransitionError;
use super::model::{
    Actor, ActorKind, FactorySettings, Idea, IdeaState, TEXT_MAX_LEN, validate_owner,
    validate_text, validate_title,
};
use super::store::FactoryStore;
use crate::config::Config;
use crate::registry;
use crate::sanitize;

const FACTORY_DISABLED_MSG: &str =
    "Factory Mode is not active in this workspace. Start it with ./run.sh --factory";

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryArgs {
    #[command(subcommand)]
    pub command: FactoryCommand,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum FactoryCommand {
    #[command(about = "show Factory Mode status and store summary")]
    Status(FactoryStatusArgs),

    #[command(about = "manage factory ideas")]
    Idea(FactoryIdeaArgs),

    #[command(about = "manage idea artifacts (feasibility studies and plans)")]
    Artifact(FactoryArtifactArgs),

    #[command(about = "check idea question queue")]
    Question(FactoryQuestionArgs),

    #[command(about = "read factory events from the log")]
    Events(FactoryEventsArgs),

    #[command(about = "rebuild materialized idea files from the event log")]
    Rebuild(FactoryRebuildArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryStatusArgs {
    #[arg(long, help = "output JSON (including structured errors)")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryIdeaArgs {
    #[command(subcommand)]
    pub command: FactoryIdeaCommand,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum FactoryIdeaCommand {
    #[command(about = "create a new idea in the inbox")]
    New(FactoryIdeaNewArgs),

    #[command(about = "list ideas sorted by updated timestamp descending")]
    List(FactoryIdeaListArgs),

    #[command(about = "show an idea and its events")]
    Show(FactoryIdeaShowArgs),

    #[command(about = "create an idea and launch its Orchestrator pane")]
    Start(FactoryIdeaStartArgs),

    #[command(about = "reopen or focus an idea's Orchestrator pane")]
    Resume(FactoryIdeaResumeArgs),

    #[command(about = "set or update an idea's title and description")]
    Describe(FactoryIdeaDescribeArgs),

    #[command(about = "record an assumption on an idea")]
    Assume(FactoryIdeaAssumeArgs),

    #[command(about = "advance idea from Ideation to Planning (Gate 1)")]
    SelectDirection(FactoryIdeaSelectDirectionArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryIdeaDescribeArgs {
    pub id: String,
    #[arg(long)]
    pub title: String,
    #[arg(long = "text-file")]
    pub text_file: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryIdeaAssumeArgs {
    pub id: String,
    #[arg(long)]
    pub text: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryIdeaSelectDirectionArgs {
    pub id: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryArtifactArgs {
    #[command(subcommand)]
    pub command: FactoryArtifactCommand,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum FactoryArtifactCommand {
    #[command(about = "store a feasibility study or plan artifact")]
    Put(FactoryArtifactPutArgs),
    #[command(about = "show a stored artifact")]
    Show(FactoryArtifactShowArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryArtifactPutArgs {
    pub id: String,
    #[arg(long)]
    pub kind: String,
    #[arg(long)]
    pub file: String,
    #[arg(long)]
    pub verdict: Option<String>,
    #[arg(long)]
    pub risk: Option<String>,
    #[arg(long)]
    pub workflow: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryArtifactShowArgs {
    pub id: String,
    #[arg(long)]
    pub kind: String,
    #[arg(long)]
    pub version: Option<u32>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryQuestionArgs {
    #[command(subcommand)]
    pub command: FactoryQuestionCommand,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum FactoryQuestionCommand {
    #[command(about = "check whether an idea has reached the open-question ceiling")]
    Check(FactoryQuestionCheckArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryQuestionCheckArgs {
    pub id: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryIdeaStartArgs {
    #[arg(long, help = "CLI provider (claude or agy)")]
    pub cli: String,

    #[arg(long, help = "model id")]
    pub model: String,

    #[arg(long, help = "optional idea title")]
    pub title: Option<String>,

    #[arg(long, help = "output JSON (including structured errors)")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryIdeaResumeArgs {
    #[arg(help = "idea id (i-xxxxxxxx)")]
    pub id: String,

    #[arg(long, help = "output JSON (including structured errors)")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryIdeaNewArgs {
    #[arg(long, help = "idea title")]
    pub title: String,

    #[arg(
        long,
        required_unless_present = "text_file",
        conflicts_with = "text_file",
        help = "idea body text"
    )]
    pub text: Option<String>,

    #[arg(
        long = "text-file",
        required_unless_present = "text",
        conflicts_with = "text",
        help = "read idea body text from a file, or '-' for stdin"
    )]
    pub text_file: Option<String>,

    #[arg(long, help = "idea owner (defaults to OS user or 'you')")]
    pub owner: Option<String>,

    #[arg(long, help = "output JSON (including structured errors)")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryIdeaListArgs {
    #[arg(long, help = "filter by idea state (snake_case)")]
    pub state: Option<String>,

    #[arg(long, help = "output JSON (including structured errors)")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryIdeaShowArgs {
    #[arg(help = "idea id (e.g. i-12345678)")]
    pub id: String,

    #[arg(long, help = "output JSON (including structured errors)")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryEventsArgs {
    #[arg(long, help = "only return events with seq > SEQ")]
    pub since: Option<u64>,

    #[arg(long, help = "filter events by idea id")]
    pub idea: Option<String>,

    #[arg(long, help = "output JSON (including structured errors)")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRebuildArgs {
    #[arg(long, help = "output JSON (including structured errors)")]
    pub json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CmdError {
    exit_code: i32,
    code: &'static str,
    message: String,
}

impl CmdError {
    fn invalid_input(message: impl Into<String>) -> Self {
        Self {
            exit_code: 1,
            code: "invalid_input",
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            exit_code: 1,
            code: "not_found",
            message: message.into(),
        }
    }

    fn factory_disabled() -> Self {
        Self {
            exit_code: 2,
            code: "factory_disabled",
            message: FACTORY_DISABLED_MSG.to_string(),
        }
    }
}

#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    error: ErrorBody<'a>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
}

#[derive(Serialize)]
struct StatusOutput {
    enabled: bool,
    ideas: usize,
    by_state: BTreeMap<String, usize>,
    settings: FactorySettings,
    log: LogSummary,
}

#[derive(Serialize)]
struct LogSummary {
    events: usize,
    truncated: bool,
}

#[derive(Serialize)]
struct IdeaListOutput {
    ideas: Vec<Idea>,
}

#[derive(Serialize)]
struct IdeaShowOutput {
    idea: Idea,
    events: Vec<Event>,
    questions: super::questions::IdeaQuestions,
}

#[derive(Serialize)]
struct EventsOutput {
    events: Vec<Event>,
    last_seq: u64,
    truncated: bool,
}

#[derive(Serialize)]
struct RebuildOutput {
    rebuilt: usize,
    truncated: bool,
}

pub fn run(args: &FactoryArgs, workspace: &Path, config: &Config) -> i32 {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let mut stdin = io::stdin().lock();
    run_with_io(
        args,
        workspace,
        config,
        &mut stdin,
        &mut stdout,
        &mut stderr,
    )
}

fn json_flag(args: &FactoryArgs) -> bool {
    match &args.command {
        FactoryCommand::Status(a) => a.json,
        FactoryCommand::Idea(idea_args) => match &idea_args.command {
            FactoryIdeaCommand::New(a) => a.json,
            FactoryIdeaCommand::List(a) => a.json,
            FactoryIdeaCommand::Show(a) => a.json,
            FactoryIdeaCommand::Start(a) => a.json,
            FactoryIdeaCommand::Resume(a) => a.json,
            FactoryIdeaCommand::Describe(a) => a.json,
            FactoryIdeaCommand::Assume(a) => a.json,
            FactoryIdeaCommand::SelectDirection(a) => a.json,
        },
        FactoryCommand::Artifact(art_args) => match &art_args.command {
            FactoryArtifactCommand::Put(a) => a.json,
            FactoryArtifactCommand::Show(a) => a.json,
        },
        FactoryCommand::Question(q_args) => match &q_args.command {
            FactoryQuestionCommand::Check(a) => a.json,
        },
        FactoryCommand::Events(a) => a.json,
        FactoryCommand::Rebuild(a) => a.json,
    }
}

pub fn run_with_io(
    args: &FactoryArgs,
    workspace: &Path,
    config: &Config,
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let json_mode = json_flag(args);
    match execute(args, workspace, config, stdin) {
        Ok(json_line) => {
            let _ = stdout.write_all(json_line.as_bytes());
            let _ = stdout.write_all(b"\n");
            let _ = stdout.flush();
            0
        }
        Err(err) => {
            let clean_msg = sanitize::clean(&err.message).replace('\n', " ");
            let _ = writeln!(stderr, "{clean_msg}");
            let _ = stderr.flush();
            if json_mode {
                let envelope = ErrorEnvelope {
                    error: ErrorBody {
                        code: err.code,
                        message: &clean_msg,
                    },
                };
                if let Ok(line) = serde_json::to_string(&envelope) {
                    let _ = stdout.write_all(line.as_bytes());
                    let _ = stdout.write_all(b"\n");
                    let _ = stdout.flush();
                }
            }
            err.exit_code
        }
    }
}

fn execute(
    args: &FactoryArgs,
    workspace: &Path,
    config: &Config,
    stdin: &mut dyn Read,
) -> Result<String, CmdError> {
    let enabled = config.factory || registry::factory_enabled(&config.state_dir, workspace);
    let store = FactoryStore::new(workspace);

    match &args.command {
        FactoryCommand::Status(_) => {
            let ideas = store.list_ideas().map_err(map_factory_error)?;
            let mut by_state = BTreeMap::new();
            for idea in &ideas {
                let key = state_key(idea.state);
                *by_state.entry(key).or_insert(0) += 1;
            }
            let settings = if store.exists() {
                store.settings().map_err(map_factory_error)?
            } else {
                FactorySettings::default()
            };
            let log = if store.exists() {
                let read = events::read_all(&store.root.join("events.jsonl"))
                    .map_err(map_factory_error)?;
                LogSummary {
                    events: read.events.len(),
                    truncated: read.truncated,
                }
            } else {
                LogSummary {
                    events: 0,
                    truncated: false,
                }
            };
            serialize_compact(&StatusOutput {
                enabled,
                ideas: ideas.len(),
                by_state,
                settings,
                log,
            })
        }
        FactoryCommand::Idea(idea_args) => match &idea_args.command {
            FactoryIdeaCommand::New(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let title = validate_title(&a.title).map_err(|e| CmdError::invalid_input(e.0))?;
                let raw_text = read_text_input(a.text.as_deref(), a.text_file.as_deref(), stdin)?;
                let text = validate_text(&raw_text).map_err(|e| CmdError::invalid_input(e.0))?;
                let owner = resolve_owner(a.owner.as_deref())?;
                let actor = resolve_actor(config);
                let idea = store
                    .create_idea(&title, &text, &owner, &actor)
                    .map_err(map_factory_error)?;
                serialize_compact(&idea)
            }
            FactoryIdeaCommand::List(a) => {
                let filter_state = a.state.as_deref().map(parse_idea_state).transpose()?;
                let mut ideas = store.list_ideas().map_err(map_factory_error)?;
                if let Some(target) = filter_state {
                    ideas.retain(|idea| idea.state == target);
                }
                serialize_compact(&IdeaListOutput { ideas })
            }
            FactoryIdeaCommand::Show(a) => {
                if !is_valid_idea_id(&a.id) {
                    return Err(CmdError::invalid_input(format!(
                        "invalid idea id: {}",
                        a.id
                    )));
                }
                let idea = store
                    .get_idea(&a.id)
                    .map_err(map_factory_error)?
                    .ok_or_else(|| CmdError::not_found(format!("idea not found: {}", a.id)))?;
                let mut idea_events = if store.exists() {
                    events::read_all(&store.root.join("events.jsonl"))
                        .map_err(map_factory_error)?
                        .events
                } else {
                    Vec::new()
                };
                idea_events.retain(|ev| ev.idea == a.id);
                let coord_dir = crate::commands::coord_dir_for(workspace, config);
                let coord_queries =
                    crate::coordination::store::CoordStore::new(coord_dir).queries();
                let questions = super::questions::for_idea(&coord_queries, &a.id);
                serialize_compact(&IdeaShowOutput {
                    idea,
                    events: idea_events,
                    questions,
                })
            }
            FactoryIdeaCommand::Start(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                if !matches!(a.cli.as_str(), "claude" | "agy") {
                    return Err(CmdError::invalid_input(format!(
                        "unsupported orchestrator cli: {} (expected claude or agy)",
                        a.cli
                    )));
                }
                let model = crate::adapters::base::validate_model_id(&a.model)
                    .map_err(|e| CmdError::invalid_input(e.to_string()))?;
                let raw_title = a.title.as_deref().unwrap_or("New idea");
                let title = validate_title(raw_title).map_err(|e| CmdError::invalid_input(e.0))?;
                let owner = resolve_owner(None)?;
                let actor = resolve_actor(config);
                let idea = store
                    .create_idea(&title, "", &owner, &actor)
                    .map_err(map_factory_error)?;
                let bound = store
                    .execute(
                        &idea.id,
                        super::machine::Command::BindOrchestrator {
                            cli: a.cli.clone(),
                            model: model.clone(),
                            conversation: None,
                        },
                        &actor,
                    )
                    .map_err(map_factory_error)?;
                let mut spec =
                    crate::adapters::base::AgentSpec::new(a.cli.clone(), "orchestrator", model);
                spec.idea = Some(bound.id.clone());
                if let Err(err) = crate::commands::add_agent(&spec, workspace, config) {
                    let _ = store.execute(
                        &bound.id,
                        super::machine::Command::Cancel {
                            reason: "orchestrator failed to start".to_string(),
                        },
                        &actor,
                    );
                    return Err(CmdError::invalid_input(format!(
                        "orchestrator failed to start: {err}"
                    )));
                }
                serialize_compact(&bound)
            }
            FactoryIdeaCommand::Resume(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                if !is_valid_idea_id(&a.id) {
                    return Err(CmdError::invalid_input(format!(
                        "invalid idea id: {}",
                        a.id
                    )));
                }
                let idea = store
                    .get_idea(&a.id)
                    .map_err(map_factory_error)?
                    .ok_or_else(|| CmdError::not_found(format!("idea not found: {}", a.id)))?;
                if idea.state.is_terminal() {
                    return Err(CmdError::invalid_input(format!(
                        "cannot resume terminal idea {} ({})",
                        idea.id,
                        idea.state.label()
                    )));
                }
                let orch = idea.orchestrator.as_ref().ok_or_else(|| {
                    CmdError::invalid_input(format!("idea {} has no orchestrator", idea.id))
                })?;
                let layout = crate::commands::make_layout(workspace, config);
                if let Ok(panes) = layout.panes()
                    && let Some(existing) = panes.iter().find(|p| p.idea == idea.id)
                {
                    let _ = layout.show_pane(&existing.id);
                    return serialize_compact(&idea);
                }
                let mut spec = crate::adapters::base::AgentSpec::new(
                    orch.cli.clone(),
                    "orchestrator",
                    orch.model.clone(),
                );
                spec.idea = Some(idea.id.clone());
                spec.conversation = orch.conversation.clone();
                crate::commands::add_agent(&spec, workspace, config)
                    .map_err(|e| CmdError::invalid_input(format!("resume failed: {e}")))?;
                serialize_compact(&idea)
            }
            FactoryIdeaCommand::Describe(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let raw_text = read_text_input(None, Some(&a.text_file), stdin)?;
                let actor = resolve_actor(config);
                let updated = store
                    .execute(
                        &a.id,
                        super::machine::Command::Describe {
                            title: a.title.clone(),
                            text: raw_text,
                        },
                        &actor,
                    )
                    .map_err(map_factory_error)?;
                serialize_compact(&updated)
            }
            FactoryIdeaCommand::Assume(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let actor = resolve_actor(config);
                let updated = store
                    .execute(
                        &a.id,
                        super::machine::Command::AddAssumption {
                            text: a.text.clone(),
                        },
                        &actor,
                    )
                    .map_err(map_factory_error)?;
                serialize_compact(&updated)
            }
            FactoryIdeaCommand::SelectDirection(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let actor = resolve_actor(config);
                let updated = store
                    .execute(&a.id, super::machine::Command::SelectDirection, &actor)
                    .map_err(map_factory_error)?;
                serialize_compact(&updated)
            }
        },
        FactoryCommand::Artifact(art_args) => match &art_args.command {
            FactoryArtifactCommand::Put(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let verdict = match a.verdict.as_deref() {
                    Some("feasible") => Some(super::model::FeasibilityVerdict::Feasible),
                    Some("feasible-with-risks") => {
                        Some(super::model::FeasibilityVerdict::FeasibleWithRisks)
                    }
                    Some("infeasible") => Some(super::model::FeasibilityVerdict::Infeasible),
                    Some(other) => {
                        return Err(CmdError::invalid_input(format!(
                            "invalid verdict: {other} (expected feasible, feasible-with-risks, or infeasible)"
                        )));
                    }
                    None => None,
                };
                let risk = match a.risk.as_deref() {
                    Some("low") => Some(super::model::Risk::Low),
                    Some("medium") => Some(super::model::Risk::Medium),
                    Some("high") => Some(super::model::Risk::High),
                    Some(other) => {
                        return Err(CmdError::invalid_input(format!(
                            "invalid risk: {other} (expected low, medium, or high)"
                        )));
                    }
                    None => None,
                };
                let src = Path::new(&a.file);
                let artifact = super::artifacts::put(&store, &a.id, &a.kind, src)
                    .map_err(map_factory_error)?;
                let missing_sections = if a.kind == "plan" {
                    let text =
                        super::artifacts::read(&store, &a.id, "plan", Some(artifact.version))
                            .map_err(map_factory_error)?;
                    super::artifacts::missing_plan_sections(&text)
                } else {
                    Vec::new()
                };
                let actor = resolve_actor(config);
                let updated = store
                    .execute(
                        &a.id,
                        super::machine::Command::StoreArtifact {
                            kind: a.kind.clone(),
                            artifact: artifact.clone(),
                            verdict,
                            risk,
                            workflow: a.workflow.clone(),
                            missing_sections: missing_sections.clone(),
                        },
                        &actor,
                    )
                    .map_err(map_factory_error)?;
                serialize_compact(&serde_json::json!({
                    "kind": artifact.kind,
                    "version": artifact.version,
                    "sha256": artifact.sha256,
                    "bytes": artifact.bytes,
                    "missing_sections": missing_sections,
                    "state": updated.state,
                }))
            }
            FactoryArtifactCommand::Show(a) => {
                if !is_valid_idea_id(&a.id) {
                    return Err(CmdError::invalid_input(format!(
                        "invalid idea id: {}",
                        a.id
                    )));
                }
                let text = super::artifacts::read(&store, &a.id, &a.kind, a.version)
                    .map_err(map_factory_error)?;
                Ok(text)
            }
        },
        FactoryCommand::Question(q_args) => match &q_args.command {
            FactoryQuestionCommand::Check(a) => {
                if !is_valid_idea_id(&a.id) {
                    return Err(CmdError::invalid_input(format!(
                        "invalid idea id: {}",
                        a.id
                    )));
                }
                let coord_dir = crate::commands::coord_dir_for(workspace, config);
                let coord_queries =
                    crate::coordination::store::CoordStore::new(coord_dir).queries();
                let summary = super::questions::for_idea(&coord_queries, &a.id);
                if summary.at_limit() {
                    return Err(CmdError {
                        exit_code: 3,
                        code: "question_limit_reached",
                        message: format!(
                            "idea {} already has {} open questions (limit {}); state an assumption instead",
                            a.id,
                            summary.open,
                            super::questions::OPEN_QUESTION_LIMIT
                        ),
                    });
                }
                serialize_compact(&summary)
            }
        },
        FactoryCommand::Events(a) => {
            if let Some(ref id) = a.idea
                && !is_valid_idea_id(id)
            {
                return Err(CmdError::invalid_input(format!("invalid idea id: {id}")));
            }
            if !store.exists() {
                return serialize_compact(&EventsOutput {
                    events: Vec::new(),
                    last_seq: a.since.unwrap_or(0),
                    truncated: false,
                });
            }
            let read =
                events::read_all(&store.root.join("events.jsonl")).map_err(map_factory_error)?;
            let log_last_seq = read.events.iter().map(|ev| ev.seq).max().unwrap_or(0);
            let mut filtered = read.events;
            if let Some(since) = a.since {
                filtered.retain(|ev| ev.seq > since);
            }
            if let Some(ref id) = a.idea {
                filtered.retain(|ev| ev.idea == *id);
            }
            let last_seq = filtered
                .iter()
                .map(|ev| ev.seq)
                .max()
                .unwrap_or(log_last_seq);
            serialize_compact(&EventsOutput {
                events: filtered,
                last_seq,
                truncated: read.truncated,
            })
        }
        FactoryCommand::Rebuild(_) => {
            if !enabled {
                return Err(CmdError::factory_disabled());
            }
            let report = store.rebuild().map_err(map_factory_error)?;
            serialize_compact(&RebuildOutput {
                rebuilt: report.ideas,
                truncated: report.truncated,
            })
        }
    }
}

fn serialize_compact<T: Serialize>(value: &T) -> Result<String, CmdError> {
    serde_json::to_string(value).map_err(|e| CmdError {
        exit_code: 1,
        code: "json_error",
        message: e.to_string(),
    })
}

fn state_key(state: IdeaState) -> String {
    match serde_json::to_value(state) {
        Ok(serde_json::Value::String(s)) => s,
        _ => "unknown".to_string(),
    }
}

fn parse_idea_state(raw: &str) -> Result<IdeaState, CmdError> {
    serde_json::from_value::<IdeaState>(serde_json::Value::String(raw.to_string()))
        .map_err(|_| CmdError::invalid_input(format!("invalid state: {raw}")))
}

fn read_text_input(
    text: Option<&str>,
    text_file: Option<&str>,
    stdin: &mut dyn Read,
) -> Result<String, CmdError> {
    if let Some(t) = text {
        return Ok(t.to_string());
    }
    let Some(path_str) = text_file else {
        return Err(CmdError::invalid_input(
            "either --text or --text-file is required",
        ));
    };

    // Bounded read so oversized files or stdin streams cannot exhaust memory before validate_text.
    let max_bytes = (TEXT_MAX_LEN as u64) * 4 + 4;
    let mut buf = Vec::new();
    if path_str == "-" {
        stdin
            .take(max_bytes)
            .read_to_end(&mut buf)
            .map_err(|e| CmdError::invalid_input(format!("failed to read stdin: {e}")))?;
    } else {
        let path = Path::new(path_str);
        let meta = fs::symlink_metadata(path)
            .map_err(|e| CmdError::invalid_input(format!("cannot read {}: {e}", path.display())))?;
        if meta.file_type().is_symlink() {
            return Err(CmdError::invalid_input(format!(
                "refusing to follow symlink at {}",
                path.display()
            )));
        }
        if meta.is_dir() {
            return Err(CmdError::invalid_input(format!(
                "refusing to read directory at {}",
                path.display()
            )));
        }
        let file = File::open(path)
            .map_err(|e| CmdError::invalid_input(format!("cannot open {}: {e}", path.display())))?;
        file.take(max_bytes).read_to_end(&mut buf).map_err(|e| {
            CmdError::invalid_input(format!("failed to read {}: {e}", path.display()))
        })?;
    }

    String::from_utf8(buf).map_err(|_| CmdError::invalid_input("text input must be valid UTF-8"))
}

fn resolve_owner(explicit: Option<&str>) -> Result<String, CmdError> {
    if let Some(owner) = explicit {
        return validate_owner(owner).map_err(|e| CmdError::invalid_input(e.0));
    }
    let os_user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok();
    if let Some(candidate) = os_user
        && let Ok(valid) = validate_owner(&candidate)
    {
        return Ok(valid);
    }
    Ok("you".to_string())
}

fn resolve_actor(config: &Config) -> Actor {
    if std::env::var("TANDEM_ROLE").ok().as_deref() == Some("orchestrator") {
        let id = if config.session_id.is_empty() {
            "orchestrator".to_string()
        } else {
            config.session_id.clone()
        };
        Actor {
            kind: ActorKind::Orchestrator,
            id,
        }
    } else {
        Actor {
            kind: ActorKind::User,
            id: "cli".to_string(),
        }
    }
}

fn map_factory_error(err: FactoryError) -> CmdError {
    match err {
        FactoryError::Transition(TransitionError::NotFound) => {
            CmdError::not_found("idea not found")
        }
        FactoryError::Transition(TransitionError::ActorNotAllowed { .. }) => CmdError {
            exit_code: 3,
            code: "actor_not_allowed",
            message: err.to_string(),
        },
        FactoryError::Transition(TransitionError::GateNotMet { .. }) => CmdError {
            exit_code: 3,
            code: "gate_not_met",
            message: err.to_string(),
        },
        FactoryError::Transition(t) => CmdError::invalid_input(t.to_string()),
        FactoryError::Symlink(_)
        | FactoryError::EventTooLarge { .. }
        | FactoryError::InvalidInput(_) => CmdError::invalid_input(err.to_string()),
        FactoryError::Io(e) => CmdError {
            exit_code: 1,
            code: "io_error",
            message: e.to_string(),
        },
        FactoryError::Json(e) => CmdError {
            exit_code: 1,
            code: "json_error",
            message: e.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::factory::model::ApprovalGate;

    #[test]
    fn test_map_factory_error_exit_codes() {
        let actor_err =
            map_factory_error(FactoryError::Transition(TransitionError::ActorNotAllowed {
                command: "Deliver",
                actor: ActorKind::User,
            }));
        assert_eq!(actor_err.exit_code, 3);
        assert_eq!(actor_err.code, "actor_not_allowed");

        let gate_err = map_factory_error(FactoryError::Transition(TransitionError::GateNotMet {
            gate: ApprovalGate::Feasibility,
            reason: "missing".to_string(),
        }));
        assert_eq!(gate_err.exit_code, 3);
        assert_eq!(gate_err.code, "gate_not_met");

        let not_found = map_factory_error(FactoryError::Transition(TransitionError::NotFound));
        assert_eq!(not_found.exit_code, 1);
        assert_eq!(not_found.code, "not_found");
    }
}
