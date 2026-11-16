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
                serialize_compact(&IdeaShowOutput {
                    idea,
                    events: idea_events,
                })
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
