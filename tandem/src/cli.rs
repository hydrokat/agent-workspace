use std::io::{IsTerminal, Write};
use std::path::Path;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};

use crate::config::Config;
use crate::state::SavedState;
use crate::tmux::client::TmuxClient;
use crate::tmux::layout::SESSION;

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("command not implemented yet")]
    NotImplemented,
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Parser, Debug, PartialEq, Eq)]
#[command(
    name = "tandem",
    bin_name = "tandem",
    version = env!("CARGO_PKG_VERSION"),
    about = "Launch planner and implementer AI CLIs side-by-side in tmux.",
    disable_help_subcommand = true
)]
pub struct Cli {
    #[arg(
        long,
        help = "resume the saved session without asking",
        conflicts_with = "fresh"
    )]
    pub resume: bool,

    #[arg(
        long,
        help = "start blank with new conversations, without asking (the saved session is replaced)",
        conflicts_with = "resume"
    )]
    pub fresh: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum Command {
    #[command(about = "add an agent to the running session (opened by the add-agent key)")]
    AddAgent,

    #[command(about = "switch agent tab in the focused column")]
    Tab(TabArgs),

    #[command(about = "focus a column or the status window")]
    Focus(FocusArgs),

    #[command(about = "close the focused agent")]
    CloseAgent,

    #[command(about = "restart an exited agent")]
    Restart,

    #[command(about = "redraw tab titles")]
    Refresh,

    #[command(about = "re-stack columns for the terminal size")]
    Reflow,

    #[command(about = "reset pane sizes to the defaults")]
    ResetLayout,

    #[command(about = "detach or quit")]
    Quit,

    #[command(about = "show the key help")]
    Help,

    #[command(about = "open help once on first attach")]
    FirstRun,

    #[command(about = "run the status window")]
    Status,

    #[command(about = "show what changed in the last update")]
    WhatsNew,

    #[command(about = "update tandem and the workspace; agents keep running (F10 menu)")]
    Update(UpdateArgs),

    #[command(about = "dispatch a status-bar or pane-menu click action")]
    Click(ClickArgs),

    #[command(about = "open the agent menu for a clicked column title")]
    ClickTab(ClickTabArgs),

    #[command(about = "Claude Code statusline hook: record quota, then run your own statusline")]
    QuotaFeed,

    #[command(about = "print the quota line shown under the window")]
    QuotaLine,

    #[command(about = "save the session state and end the session")]
    SaveExit,

    #[command(about = "end the session and delete the saved state")]
    DiscardExit,

    #[command(about = "restart tandem's own windows on the installed code")]
    Reload,

    #[command(about = "empty-column placeholder")]
    Placeholder(PlaceholderArgs),

    #[command(about = "run the agent sidebar")]
    Sidebar,

    #[command(about = "show an agent in the main window")]
    Show(ShowArgs),

    #[command(about = "select an agent or toggle it in the grid")]
    Select(SelectArgs),

    #[command(about = "scroll the multi-agent grid")]
    GridScroll(GridScrollArgs),

    #[command(about = "run the grid overflow scrollbar")]
    Gridbar,

    #[command(about = "equalize pane split or reset border to default")]
    Equalize(EqualizeArgs),

    #[command(about = "reset UI layout, sizes, grid and sidebar to defaults")]
    ResetUi,

    #[command(about = "output workspace snapshot in JSON format")]
    Snapshot(SnapshotArgs),

    #[command(about = "answer an agent coordination query")]
    AnswerQuery(AnswerQueryArgs),

    #[command(about = "list host network IPs and active workspaces")]
    ListIps(ListIpsArgs),

    #[command(about = "rename the current workspace to a custom alphanumeric name")]
    RenameWorkspace(RenameWorkspaceArgs),

    #[command(about = "rename an agent to a custom alphanumeric name")]
    RenameAgent(RenameAgentArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct RenameWorkspaceArgs {
    #[arg(help = "new alphanumeric name (1..=32 chars, or empty to reset to default)")]
    pub name: Option<String>,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct RenameAgentArgs {
    #[arg(help = "pane id (%...) or session id (role-index) to rename")]
    pub target: String,
    #[arg(help = "new alphanumeric name (1..=32 chars, or empty to reset to default)")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Args, PartialEq, Eq)]
pub struct SnapshotArgs {
    #[arg(long, default_value_t = true, help = "output snapshot in JSON format")]
    pub json: bool,

    #[arg(
        long,
        help = "keep running; print one JSON line each time the snapshot changes"
    )]
    pub watch: bool,

    #[arg(
        long,
        default_value_t = 1000,
        value_parser = clap::value_parser!(u64).range(250..=60_000),
        help = "watch mode: milliseconds between checks"
    )]
    pub interval_ms: u64,

    #[arg(long, help = "indent the JSON (one-shot mode only)")]
    pub pretty: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct AnswerQueryArgs {
    #[arg(long, help = "query ID to answer")]
    pub id: String,
    #[arg(long, help = "answer text")]
    pub answer: String,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct ListIpsArgs {
    #[arg(
        long,
        default_value_t = true,
        help = "output candidate IPs in JSON format"
    )]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct EqualizeArgs {
    #[arg(default_value = "")]
    pub pane: String,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct SelectArgs {
    pub target: String,
    #[arg(long)]
    pub toggle: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct GridScrollArgs {
    pub delta: isize,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct TabArgs {
    #[arg(value_enum)]
    pub direction: TabDirection,
    pub number: Option<i32>,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
#[value(rename_all = "snake_case")]
pub enum TabDirection {
    Prev,
    Next,
    Select,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FocusArgs {
    #[arg(value_enum)]
    pub target: FocusTarget,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
#[value(rename_all = "snake_case")]
pub enum FocusTarget {
    Planner,
    Implementer,
    Status,
    Queries,
    Sidebar,
    Settings,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct UpdateArgs {
    #[arg(long, help = "do not ask for confirmation")]
    pub yes: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct ClickArgs {
    #[arg(default_value = "", help = "action name from the hint bar")]
    pub action: String,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct ClickTabArgs {
    #[arg(help = "pane id (%N format)")]
    pub pane: String,

    #[arg(long, default_value = "", help = "pane id of the agent to switch to")]
    pub select: String,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct PlaceholderArgs {
    pub role: String,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct ShowArgs {
    #[arg(help = "pane id (%N format)")]
    pub pane: String,
}

pub fn build_parser() -> clap::Command {
    Cli::command()
}

pub fn parse_from<I, T>(itr: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    Cli::try_parse_from(itr)
}

pub fn dispatch(cli: &Cli) -> Result<(), CliError> {
    let config = Config::from_env().map_err(|e| anyhow::anyhow!("{e}"))?;
    let workspace = config
        .workspace
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    match &cli.command {
        None => {
            launch(&workspace, &config, cli.resume, cli.fresh)?;
        }
        Some(Command::Help) => {
            crate::commands::show_help();
        }
        Some(Command::WhatsNew) => {
            crate::commands::show_whats_new(&workspace);
        }
        Some(Command::Placeholder(args)) => {
            crate::commands::placeholder(&args.role);
        }
        Some(Command::Status) => {
            crate::status::run_status(&workspace, &config)?;
        }
        Some(Command::QuotaFeed) => {
            crate::commands::quota_feed(&config);
        }
        Some(Command::QuotaLine) => {
            crate::commands::quota_line(&workspace, &config);
        }
        Some(Command::Update(args)) => {
            let code = crate::commands::run_update(&workspace, &config, args.yes);
            std::process::exit(code);
        }
        Some(Command::AddAgent) => {
            let spec = crate::wizard::runner::run_add_agent(None, None)?;
            if let Some(s) = spec {
                crate::commands::add_agent(&s, &workspace, &config)?;
            }
        }
        Some(Command::Sidebar) => {
            crate::sidebar::run_sidebar(&workspace, &config)?;
        }
        Some(Command::Show(args)) => {
            let layout = crate::commands::make_layout(&workspace, &config);
            let _ = layout.show_pane(&args.pane);
        }
        Some(Command::Select(args)) => {
            let layout = crate::commands::make_layout(&workspace, &config);
            let _ = layout.select_agent(&args.target, args.toggle);
        }
        Some(Command::GridScroll(args)) => {
            let layout = crate::commands::make_layout(&workspace, &config);
            let _ = layout.scroll_grid(args.delta);
        }
        Some(Command::Gridbar) => {
            crate::gridbar::run_gridbar(&workspace, &config)?;
        }
        Some(Command::Snapshot(args)) => {
            if args.watch {
                let interval = std::time::Duration::from_millis(args.interval_ms);
                let mut out = std::io::stdout().lock();
                let _ =
                    crate::commands::snapshot_watch(&workspace, &config, interval, &mut out, None);
            } else {
                crate::commands::snapshot_json(&workspace, &config, args.pretty);
            }
        }
        Some(Command::AnswerQuery(args)) => {
            crate::commands::answer_query_cmd(&workspace, &config, &args.id, &args.answer);
        }
        Some(Command::ListIps(_)) => {
            crate::commands::list_ips_json(&workspace);
        }
        Some(Command::RenameWorkspace(args)) => {
            crate::commands::rename_workspace_cmd(&workspace, args.name.as_deref());
        }
        Some(Command::RenameAgent(args)) => {
            crate::commands::rename_agent_cmd(
                &workspace,
                &config,
                &args.target,
                args.name.as_deref(),
            );
        }
        Some(cmd) => {
            tmux_command(cmd, &workspace, &config)?;
        }
    }
    Ok(())
}

fn tmux_command(cmd: &Command, workspace: &Path, config: &Config) -> Result<(), CliError> {
    let mut layout = crate::commands::make_layout(workspace, config);
    match cmd {
        Command::Tab(args) => {
            crate::commands::tab(&mut layout, config, args.direction, args.number);
        }
        Command::Focus(args) => {
            crate::commands::focus(&mut layout, config, args.target);
        }
        Command::CloseAgent => {
            crate::commands::close_agent(&mut layout, config);
        }
        Command::Restart => {
            let pane = if !config.tmux_pane.is_empty() {
                config.tmux_pane.clone()
            } else {
                layout
                    .client
                    .run_checked(&["display-message", "-p", "#{pane_id}"])
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            };
            if !pane.is_empty() {
                crate::commands::restart(&mut layout, &pane, workspace, config);
            }
        }
        Command::Refresh => {
            let _ = layout.refresh();
        }
        Command::Reflow => {
            let size = layout
                .client
                .run_checked(&["display-message", "-p", "#{client_width} #{client_height}"])
                .unwrap_or_default();
            let parts: Vec<&str> = size.split_whitespace().collect();
            let ok =
                parts.len() == 2 && parts.iter().all(|x| x.chars().all(|c| c.is_ascii_digit()));
            let (w, h) = if ok {
                (
                    parts[0].parse().unwrap_or(200),
                    parts[1].parse().unwrap_or(0),
                )
            } else {
                (200, 0)
            };
            let _ = layout.reflow(w, h);
        }
        Command::ResetLayout => {
            let _ = layout.reset();
        }
        Command::ResetUi => {
            let _ = layout.reset_ui();
        }
        Command::Equalize(args) => {
            let _ = layout.equalize(&args.pane);
        }
        Command::Quit => {
            crate::commands::quit_menu(&layout, workspace.to_str());
        }
        Command::SaveExit => {
            crate::commands::save_exit(&layout, workspace);
        }
        Command::DiscardExit => {
            crate::commands::discard_exit(&layout, workspace);
        }
        Command::Reload => {
            crate::commands::reload_ui(&mut layout, workspace, config);
        }
        Command::Click(args) => {
            crate::commands::click(&mut layout, config, &args.action);
        }
        Command::ClickTab(args) => {
            crate::commands::click_tab(&mut layout, config, &args.pane, &args.select);
        }
        Command::FirstRun => {
            crate::commands::first_run(&layout, config);
        }
        _ => {}
    }
    Ok(())
}

pub fn launch(
    workspace: &Path,
    config: &Config,
    mut resume: bool,
    fresh: bool,
) -> Result<(), anyhow::Error> {
    let layout = crate::commands::make_layout(workspace, config);
    if fresh && layout.exists() {
        eprintln!("Ending the running tandem session so --fresh starts from a clean state…");
        crate::commands::discard_exit(&layout, workspace);
        for _ in 0..50 {
            if !layout.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    if layout.exists() {
        if let Some(notice) = crate::notice::read_notice(workspace)
            && !notice.seen
        {
            crate::commands::reload_in_fresh_process(workspace, &layout);
        }
        crate::commands::attach(workspace);
    }
    note_legacy_session();
    let mut saved = if fresh {
        if let Err(e) = crate::state::discard(&crate::state::state_path(workspace)) {
            eprintln!("Could not discard the saved session: {e}");
        }
        None
    } else {
        crate::state::load(&crate::state::state_path(workspace))
    };

    if resume && saved.is_none() {
        eprintln!("No saved session to resume.");
        std::process::exit(1);
    }

    if !resume && let Some(ref st) = saved {
        resume = ask_resume(st);
    }

    let mut specs = None;
    if resume && let Some(ref st) = saved {
        let (resumed_specs, notes) =
            crate::commands::resume_specs(st, workspace, crate::versions::default_which);
        for note in notes {
            eprintln!("{note}");
        }
        if resumed_specs.is_empty() {
            eprintln!("Nothing from the saved session can be started. Starting blank.");
            saved = None;
        } else {
            specs = Some(resumed_specs);
        }
    }

    if specs.is_none() {
        saved = None;
        specs = crate::wizard::runner::run_launch_wizard(None, None)?;
    }

    let final_specs = match specs {
        Some(s) if !s.is_empty() => s,
        _ => {
            eprintln!("Cancelled.");
            std::process::exit(1);
        }
    };

    crate::commands::start(&final_specs, workspace, config, saved.as_ref())
}

pub fn note_legacy_session() {
    let found = TmuxClient::new("tandem", None)
        .run(&["has-session", "-t", SESSION])
        .map(|r| r.code == 0)
        .unwrap_or(false);
    if found {
        eprintln!(
            "Note: an older tandem session (from before per-project sessions) is still running. \
             Detach and end it with `tmux -L tandem kill-server` once you no longer need it; \
             this project gets its own session."
        );
    }
}

pub fn ask_resume(saved: &SavedState) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    println!("{}", crate::state::describe(saved, None));
    loop {
        print!("\n[R]esume it, start [b]lank (new conversations), or [q]uit? ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_err() {
            return false;
        }
        let answer = line.trim().to_lowercase();
        if answer.is_empty() || answer == "r" || answer == "resume" {
            return true;
        }
        if answer == "b" || answer == "blank" {
            return false;
        }
        if answer == "q" || answer == "quit" {
            std::process::exit(0);
        }
    }
}

pub fn run() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();
    dispatch(&cli)?;
    Ok(())
}
