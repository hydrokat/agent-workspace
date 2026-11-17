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

    #[command(about = "validate and list factory workflows")]
    Workflow(FactoryWorkflowArgs),

    #[command(about = "manage workflow runs")]
    Run(FactoryRunArgs),

    #[command(about = "manage headless provider gates and consent")]
    Provider(FactoryProviderArgs),

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

    #[command(about = "mark idea ready for development (user-only Gate 2)")]
    Ready(FactoryIdeaReadyArgs),

    #[command(about = "revoke ready-for-development tag before a run exists")]
    RevokeReady(FactoryIdeaRevokeReadyArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryIdeaReadyArgs {
    pub id: String,
    #[arg(long)]
    pub confirm: String,
    #[arg(long, default_value = "cli")]
    pub via: String,
    #[arg(long)]
    pub workflow: Option<String>,
    #[arg(long)]
    pub deploy: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryIdeaRevokeReadyArgs {
    pub id: String,
    #[arg(long)]
    pub json: bool,
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
pub struct FactoryWorkflowArgs {
    #[command(subcommand)]
    pub command: FactoryWorkflowCommand,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum FactoryWorkflowCommand {
    #[command(about = "validate a workflow YAML file by name")]
    Validate(FactoryWorkflowValidateArgs),
    #[command(about = "list workflows in <workspace>/workflows/")]
    List(FactoryWorkflowListArgs),
    #[command(about = "print the embedded default workflow YAML to stdout")]
    Default,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryWorkflowValidateArgs {
    pub name: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryWorkflowListArgs {
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunArgs {
    #[command(subcommand)]
    pub command: FactoryRunCommand,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum FactoryRunCommand {
    #[command(about = "create a run from a ready idea")]
    Create(FactoryRunCreateArgs),
    #[command(about = "show a run")]
    Show(FactoryRunShowArgs),
    #[command(about = "list runs")]
    List(FactoryRunListArgs),
    #[command(about = "withdraw a queued run before any stage starts")]
    Withdraw(FactoryRunWithdrawArgs),
    #[command(about = "simulate a workflow run without executing or writing anything")]
    Simulate(FactoryRunSimulateArgs),
    #[command(about = "select target repository for a run")]
    Target(FactoryRunTargetArgs),
    #[command(about = "remove a completed/cancelled/withdrawn run worktree")]
    Cleanup(FactoryRunCleanupArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunCreateArgs {
    pub idea: String,
    #[arg(long)]
    pub deploy: bool,
    #[arg(long)]
    pub workflow: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunShowArgs {
    pub id: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunListArgs {
    #[arg(long)]
    pub idea: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunWithdrawArgs {
    pub id: String,
    #[arg(long)]
    pub confirm: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunSimulateArgs {
    pub idea: Option<String>,
    #[arg(long)]
    pub workflow: Option<String>,
    #[arg(long)]
    pub ui_changed: bool,
    #[arg(long)]
    pub no_ui_changed: bool,
    #[arg(long)]
    pub deploy: bool,
    #[arg(long)]
    pub approve_deploy: bool,
    #[arg(long)]
    pub fail: Vec<String>,
    #[arg(long)]
    pub retries: Option<u32>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunProviderStub;

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunTargetArgs {
    pub run: String,
    pub name: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunCleanupArgs {
    pub run: String,
    #[arg(long)]
    pub confirm: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryProviderArgs {
    #[command(subcommand)]
    pub command: FactoryProviderCommand,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum FactoryProviderCommand {
    #[command(about = "show provider terms verdicts and gate state")]
    Status(FactoryProviderStatusArgs),
    #[command(about = "record user consent to open a provider gate")]
    Consent(FactoryProviderConsentArgs),
    #[command(about = "close a provider gate")]
    Revoke(FactoryProviderRevokeArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryProviderStatusArgs {
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryProviderConsentArgs {
    pub provider: String,
    #[arg(long)]
    pub auth: String,
    #[arg(long)]
    pub confirm: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryProviderRevokeArgs {
    pub provider: String,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    plan_snapshot: Option<super::approval::SnapshotVerification>,
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
            FactoryIdeaCommand::Ready(a) => a.json,
            FactoryIdeaCommand::RevokeReady(a) => a.json,
        },
        FactoryCommand::Artifact(art_args) => match &art_args.command {
            FactoryArtifactCommand::Put(a) => a.json,
            FactoryArtifactCommand::Show(a) => a.json,
        },
        FactoryCommand::Question(q_args) => match &q_args.command {
            FactoryQuestionCommand::Check(a) => a.json,
        },
        FactoryCommand::Workflow(w_args) => match &w_args.command {
            FactoryWorkflowCommand::Validate(a) => a.json,
            FactoryWorkflowCommand::List(a) => a.json,
            FactoryWorkflowCommand::Default => false,
        },
        FactoryCommand::Run(r_args) => match &r_args.command {
            FactoryRunCommand::Create(a) => a.json,
            FactoryRunCommand::Show(a) => a.json,
            FactoryRunCommand::List(a) => a.json,
            FactoryRunCommand::Withdraw(a) => a.json,
            FactoryRunCommand::Simulate(a) => a.json,
            FactoryRunCommand::Target(a) => a.json,
            FactoryRunCommand::Cleanup(a) => a.json,
        },
        FactoryCommand::Provider(p_args) => match &p_args.command {
            FactoryProviderCommand::Status(a) => a.json,
            FactoryProviderCommand::Consent(a) => a.json,
            FactoryProviderCommand::Revoke(a) => a.json,
        },
        FactoryCommand::Events(a) => a.json,
        FactoryCommand::Rebuild(a) => a.json,
    }
}

fn run_simulate(
    args: &FactoryRunSimulateArgs,
    workspace: &Path,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let store = FactoryStore::new(workspace);
    let settings = if store.exists() {
        store.settings().unwrap_or_default()
    } else {
        FactorySettings::default()
    };

    let wf_name = if let Some(ref name) = args.workflow {
        Some(name.clone())
    } else if let Some(ref idea_id) = args.idea {
        match store.get_idea(idea_id) {
            Ok(Some(idea)) => idea
                .plan
                .as_ref()
                .map(|p| p.workflow.clone())
                .filter(|w| !w.is_empty()),
            Ok(None) => {
                let _ = writeln!(stderr, "idea not found: {idea_id}");
                return 1;
            }
            Err(e) => {
                let _ = writeln!(stderr, "{e}");
                return 1;
            }
        }
    } else {
        None
    };

    let loaded = match wf_name.as_deref() {
        None | Some("default-delivery") => super::workflow::load_default_workflow(workspace)
            .map(|(raw, src)| {
                let label = match src {
                    super::workflow::WorkflowSource::Workspace => "file",
                    super::workflow::WorkflowSource::Embedded => "embedded",
                };
                (raw, label)
            })
            .map_err(|e| e.to_string()),
        Some(name) => {
            super::workflow::validate::resolve_workflow_path(workspace, name).and_then(|p| {
                super::workflow::parse::load_workflow_file(&p)
                    .map(|raw| (raw, "file"))
                    .map_err(|e| e.to_string())
            })
        }
    };

    let (raw, source) = match loaded {
        Ok(v) => v,
        Err(msg) => {
            let _ = writeln!(stderr, "{msg}");
            return 1;
        }
    };

    let wf = match super::workflow::validate::validate(&raw, &settings) {
        Ok(wf) => wf,
        Err(diags) => {
            for d in &diags {
                let _ = writeln!(stderr, "{d}");
            }
            return 1;
        }
    };

    let mut fail_counts = std::collections::HashMap::new();
    for spec in &args.fail {
        if let Some((stage, n_str)) = spec.split_once(':') {
            let Ok(n) = n_str.parse::<u32>() else {
                let _ = writeln!(stderr, "invalid --fail count: {spec}");
                return 1;
            };
            fail_counts.insert(stage.to_string(), n);
        } else {
            fail_counts.insert(spec.clone(), 1);
        }
    }

    let script = super::simulate::SimulationScript {
        ui_changed: args.ui_changed && !args.no_ui_changed,
        deploy_requested: args.deploy,
        approve_deploy: args.approve_deploy,
        fail_counts,
        max_retries: args.retries,
    };

    let trace = super::simulate::simulate(&wf, source, &settings, &script);
    if args.json {
        if let Ok(line) = serde_json::to_string(&trace) {
            let _ = writeln!(stdout, "{line}");
        }
    } else {
        let _ = writeln!(stdout, "{}", super::simulate::format_trace_text(&trace));
    }
    let _ = stdout.flush();

    match trace.outcome {
        super::simulate::SimulationOutcome::Finished { .. } => 0,
        super::simulate::SimulationOutcome::Blocked { .. }
        | super::simulate::SimulationOutcome::Stopped { .. } => 1,
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
    if let FactoryCommand::Run(r_args) = &args.command
        && let FactoryRunCommand::Simulate(sim_args) = &r_args.command
    {
        return run_simulate(sim_args, workspace, stdout, stderr);
    }
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
                let plan_snapshot = super::approval::verify_snapshot(&store, &idea);
                serialize_compact(&IdeaShowOutput {
                    idea,
                    events: idea_events,
                    questions,
                    plan_snapshot,
                })
            }
            FactoryIdeaCommand::Start(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let bound =
                    start_idea_cmd(workspace, config, &a.cli, &a.model, a.title.as_deref())?;
                serialize_compact(&bound)
            }
            FactoryIdeaCommand::Resume(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let idea = resume_idea_cmd(workspace, config, &a.id)?;
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
            FactoryIdeaCommand::Ready(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let approved_by = resolve_owner(None)?;
                let res = super::approval::mark_ready_with_workflow(
                    &store,
                    workspace,
                    &a.id,
                    &a.confirm,
                    &a.via,
                    &approved_by,
                    a.workflow.as_deref(),
                    a.deploy,
                    &|k| std::env::var(k).ok(),
                )
                .map_err(|e| CmdError {
                    exit_code: e.exit_code(),
                    code: e.code(),
                    message: e.to_string(),
                })?;
                serialize_compact(&res)
            }
            FactoryIdeaCommand::RevokeReady(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let user_id = resolve_owner(None)?;
                let updated = super::approval::revoke_ready(&store, &a.id, &user_id, &|k| {
                    std::env::var(k).ok()
                })
                .map_err(|e| CmdError {
                    exit_code: e.exit_code(),
                    code: e.code(),
                    message: e.to_string(),
                })?;
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
        FactoryCommand::Workflow(w_args) => {
            let settings = if store.exists() {
                store.settings().unwrap_or_default()
            } else {
                FactorySettings::default()
            };
            match &w_args.command {
                FactoryWorkflowCommand::Validate(a) => {
                    let path = super::workflow::validate::resolve_workflow_path(workspace, &a.name)
                        .map_err(CmdError::invalid_input)?;
                    let raw = super::workflow::parse::load_workflow_file(&path)
                        .map_err(|e| CmdError::invalid_input(e.to_string()))?;
                    match super::workflow::validate::validate(&raw, &settings) {
                        Ok(wf) => serialize_compact(&serde_json::json!({
                            "valid": true,
                            "name": wf.name,
                            "version": wf.version,
                            "sha256": wf.sha256,
                            "diagnostics": [],
                        })),
                        Err(diags) => {
                            let msg = diags
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join("; ");
                            Err(CmdError::invalid_input(msg))
                        }
                    }
                }
                FactoryWorkflowCommand::List(_) => {
                    let dir = workspace.join("workflows");
                    let mut items = Vec::new();
                    if dir.is_dir()
                        && let Ok(entries) = fs::read_dir(&dir)
                    {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                                continue;
                            }
                            let stem = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("")
                                .to_string();
                            match super::workflow::parse::load_workflow_file(&path) {
                                Ok(raw) => {
                                    match super::workflow::validate::validate(&raw, &settings) {
                                        Ok(wf) => items.push(serde_json::json!({
                                            "name": wf.name,
                                            "version": wf.version,
                                            "sha256": wf.sha256,
                                            "valid": true,
                                            "diagnostics": 0,
                                        })),
                                        Err(diags) => items.push(serde_json::json!({
                                            "name": raw.name.map(|n| n.value).unwrap_or(stem),
                                            "version": raw.version.map(|v| v.value).unwrap_or(0),
                                            "sha256": raw.sha256,
                                            "valid": false,
                                            "diagnostics": diags.len(),
                                        })),
                                    }
                                }
                                Err(_) => items.push(serde_json::json!({
                                    "name": stem,
                                    "version": 0,
                                    "sha256": "",
                                    "valid": false,
                                    "diagnostics": 1,
                                })),
                            }
                        }
                    }
                    serialize_compact(&serde_json::json!({ "workflows": items }))
                }
                FactoryWorkflowCommand::Default => Ok(super::workflow::DEFAULT_WORKFLOW_YAML
                    .trim_end_matches('\n')
                    .to_string()),
            }
        }
        FactoryCommand::Run(r_args) => match &r_args.command {
            FactoryRunCommand::Create(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                super::approval::ensure_user_caller(&|k| std::env::var(k).ok()).map_err(|e| {
                    CmdError {
                        exit_code: e.exit_code(),
                        code: e.code(),
                        message: e.to_string(),
                    }
                })?;
                let actor = Actor {
                    kind: ActorKind::User,
                    id: resolve_owner(None)?,
                };
                let run = super::run::create_run(
                    &store,
                    workspace,
                    &a.idea,
                    super::run::CreateRunOptions {
                        deploy_requested: a.deploy,
                        workflow: a.workflow.clone(),
                    },
                    &actor,
                )
                .map_err(|e| CmdError {
                    exit_code: 1,
                    code: e.code(),
                    message: e.to_string(),
                })?;
                serialize_compact(&run)
            }
            FactoryRunCommand::Show(a) => {
                let run = super::run::get_run(&store, &a.id)
                    .map_err(|e| CmdError {
                        exit_code: 1,
                        code: e.code(),
                        message: e.to_string(),
                    })?
                    .ok_or_else(|| CmdError::not_found(format!("run {}", a.id)))?;
                serialize_compact(&run)
            }
            FactoryRunCommand::List(a) => {
                let runs =
                    super::run::list_runs(&store, a.idea.as_deref()).map_err(|e| CmdError {
                        exit_code: 1,
                        code: e.code(),
                        message: e.to_string(),
                    })?;
                serialize_compact(&runs)
            }
            FactoryRunCommand::Withdraw(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                if a.confirm != a.id {
                    return Err(CmdError::invalid_input(format!(
                        "--confirm {} does not match run {}",
                        a.confirm, a.id
                    )));
                }
                super::approval::ensure_user_caller(&|k| std::env::var(k).ok()).map_err(|e| {
                    CmdError {
                        exit_code: e.exit_code(),
                        code: e.code(),
                        message: e.to_string(),
                    }
                })?;
                let actor = Actor {
                    kind: ActorKind::User,
                    id: resolve_owner(None)?,
                };
                let run =
                    super::run::withdraw_run(&store, &a.id, &actor).map_err(|e| CmdError {
                        exit_code: 1,
                        code: e.code(),
                        message: e.to_string(),
                    })?;
                serialize_compact(&run)
            }
            FactoryRunCommand::Simulate(_) => {
                unreachable!("simulate handled directly in run_with_io")
            }
            FactoryRunCommand::Target(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let run = super::workspace::set_run_target(&store, workspace, &a.run, &a.name)
                    .map_err(|e| CmdError::invalid_input(e.to_string()))?;
                serialize_compact(&run)
            }
            FactoryRunCommand::Cleanup(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                if a.confirm != a.run {
                    return Err(CmdError::invalid_input(format!(
                        "--confirm {} does not match run {}",
                        a.confirm, a.run
                    )));
                }
                let actor = Actor {
                    kind: ActorKind::User,
                    id: resolve_owner(None)?,
                };
                super::workspace::cleanup_worktree(&store, workspace, &a.run, &actor, &|k| {
                    std::env::var(k).ok()
                })
                .map_err(|e| CmdError::invalid_input(e.to_string()))?;
                serialize_compact(&serde_json::json!({ "cleaned": a.run }))
            }
        },
        FactoryCommand::Provider(p_args) => {
            if !enabled {
                return Err(CmdError::factory_disabled());
            }
            match &p_args.command {
                FactoryProviderCommand::Status(_) => {
                    let settings = if store.exists() {
                        store.settings().map_err(map_factory_error)?
                    } else {
                        FactorySettings::default()
                    };
                    let today = super::broker::gate::today_iso_date();
                    let items = super::broker::gate::provider_statuses(&settings, &today);
                    serialize_compact(&items)
                }
                FactoryProviderCommand::Consent(a) => {
                    let auth_type = super::broker::gate::AuthType::parse(&a.auth).ok_or_else(|| {
                        CmdError::invalid_input(format!(
                            "invalid --auth value: {} (expected subscription, api-key, or cloud)",
                            a.auth
                        ))
                    })?;
                    let consented_by = resolve_owner(None)?;
                    let gate = super::broker::gate::consent_provider(
                        &store,
                        &a.provider,
                        auth_type,
                        &a.confirm,
                        &consented_by,
                        &|k| std::env::var(k).ok(),
                    )
                    .map_err(|e| CmdError {
                        exit_code: e.exit_code(),
                        code: e.code(),
                        message: e.to_string(),
                    })?;
                    serialize_compact(&gate)
                }
                FactoryProviderCommand::Revoke(a) => {
                    let revoked_by = resolve_owner(None)?;
                    let gate = super::broker::gate::revoke_provider(
                        &store,
                        &a.provider,
                        &revoked_by,
                        &|k| std::env::var(k).ok(),
                    )
                    .map_err(|e| CmdError {
                        exit_code: e.exit_code(),
                        code: e.code(),
                        message: e.to_string(),
                    })?;
                    serialize_compact(&gate)
                }
            }
        }
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

pub fn start_idea(
    workspace: &Path,
    config: &Config,
    cli: &str,
    model: &str,
    title: Option<&str>,
) -> Result<Idea, String> {
    start_idea_cmd(workspace, config, cli, model, title).map_err(|e| e.message)
}

pub fn resume_idea(workspace: &Path, config: &Config, idea_id: &str) -> Result<Idea, String> {
    resume_idea_cmd(workspace, config, idea_id).map_err(|e| e.message)
}

fn start_idea_cmd(
    workspace: &Path,
    config: &Config,
    cli: &str,
    model: &str,
    title: Option<&str>,
) -> Result<Idea, CmdError> {
    if !matches!(cli, "claude" | "agy") {
        return Err(CmdError::invalid_input(format!(
            "unsupported orchestrator cli: {cli} (expected claude or agy)"
        )));
    }
    let validated_model = crate::adapters::base::validate_model_id(model)
        .map_err(|e| CmdError::invalid_input(e.to_string()))?;
    let raw_title = title.unwrap_or("New idea");
    let validated_title = validate_title(raw_title).map_err(|e| CmdError::invalid_input(e.0))?;
    let owner = resolve_owner(None)?;
    let actor = resolve_actor(config);
    let store = FactoryStore::new(workspace);
    let idea = store
        .create_idea(&validated_title, "", &owner, &actor)
        .map_err(map_factory_error)?;
    let bound = store
        .execute(
            &idea.id,
            super::machine::Command::BindOrchestrator {
                cli: cli.to_string(),
                model: validated_model.clone(),
                conversation: None,
            },
            &actor,
        )
        .map_err(map_factory_error)?;
    let mut spec =
        crate::adapters::base::AgentSpec::new(cli.to_string(), "orchestrator", validated_model);
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
    Ok(bound)
}

fn resume_idea_cmd(workspace: &Path, config: &Config, idea_id: &str) -> Result<Idea, CmdError> {
    if !is_valid_idea_id(idea_id) {
        return Err(CmdError::invalid_input(format!(
            "invalid idea id: {idea_id}"
        )));
    }
    let store = FactoryStore::new(workspace);
    let idea = store
        .get_idea(idea_id)
        .map_err(map_factory_error)?
        .ok_or_else(|| CmdError::not_found(format!("idea not found: {idea_id}")))?;
    if idea.state.is_terminal() {
        return Err(CmdError::invalid_input(format!(
            "cannot resume terminal idea {} ({})",
            idea.id,
            idea.state.label()
        )));
    }
    let orch = idea
        .orchestrator
        .as_ref()
        .ok_or_else(|| CmdError::invalid_input(format!("idea {} has no orchestrator", idea.id)))?;
    let layout = crate::commands::make_layout(workspace, config);
    if let Ok(panes) = layout.panes()
        && let Some(existing) = panes.iter().find(|p| p.idea == idea.id)
    {
        let _ = layout.show_pane(&existing.id);
        return Ok(idea);
    }
    let mut spec =
        crate::adapters::base::AgentSpec::new(orch.cli.clone(), "orchestrator", orch.model.clone());
    spec.idea = Some(idea.id.clone());
    spec.conversation = orch.conversation.clone();
    crate::commands::add_agent(&spec, workspace, config)
        .map_err(|e| CmdError::invalid_input(format!("resume failed: {e}")))?;
    Ok(idea)
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
