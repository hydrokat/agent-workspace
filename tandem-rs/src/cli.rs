use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("command not implemented yet")]
    NotImplemented,
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

pub fn dispatch(_cli: &Cli) -> Result<(), CliError> {
    Err(CliError::NotImplemented)
}

pub fn run() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();
    dispatch(&cli)?;
    Ok(())
}
