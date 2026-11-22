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

    #[command(about = "discover, confirm, and inspect project test and lint commands")]
    Checks(FactoryChecksArgs),

    #[command(about = "inspect and update factory settings")]
    Settings(FactorySettingsArgs),

    #[command(about = "manage agent and target tool permissions")]
    Permissions(FactoryPermissionsArgs),

    #[command(about = "run the factory execution loop (factory sessions only)")]
    Runner,

    #[command(about = "read factory events from the log")]
    Events(FactoryEventsArgs),

    #[command(about = "rebuild materialized idea files from the event log")]
    Rebuild(FactoryRebuildArgs),

    #[command(about = "test and inspect secret redaction")]
    Redact(FactoryRedactArgs),

    #[command(about = "inspect the factory audit timeline or verify log integrity")]
    Audit(FactoryAuditArgs),

    #[command(about = "diagnose and safely repair Factory Mode state")]
    Doctor(FactoryDoctorArgs),

    #[command(about = "summarize Factory Mode pilot metrics, rework, cost, and evidence")]
    Report(FactoryReportArgs),

    #[command(about = "list the models available to an orchestrator CLI")]
    Models(FactoryModelsArgs),

    #[command(about = "check CLI sign-in readiness and open CLI-native login panes")]
    Login(FactoryLoginArgs),

    #[command(about = "manage interactive worker stage assignments and panes")]
    Agent(FactoryAgentArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryAgentArgs {
    #[command(subcommand)]
    pub command: FactoryAgentCommand,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum FactoryAgentCommand {
    #[command(about = "assign a CLI and model to a run stage")]
    Assign(FactoryAgentAssignArgs),
    #[command(about = "list worker agents for a run")]
    List(FactoryAgentListArgs),
    #[command(about = "send a message file to a running worker pane")]
    Send(FactoryAgentSendArgs),
    #[command(about = "capture recent lines from a running worker pane")]
    Capture(FactoryAgentCaptureArgs),
    #[command(about = "terminate a worker pane cleanly")]
    Terminate(FactoryAgentTerminateArgs),
    #[command(about = "relaunch a worker job pane with current assignments")]
    Relaunch(FactoryAgentRelaunchArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryAgentAssignArgs {
    #[arg(help = "run ID (r-...)")]
    pub run: String,
    #[arg(help = "stage name")]
    pub stage: String,
    #[arg(long, help = "worker CLI (claude, agy, or opencode)")]
    pub cli: String,
    #[arg(long, help = "model id")]
    pub model: String,
    #[arg(long, help = "optional role label")]
    pub role: Option<String>,
    #[arg(long, help = "assignment source (user or orchestrator)")]
    pub by: Option<String>,
    #[arg(long, help = "output JSON")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryAgentListArgs {
    #[arg(
        help = "run ID (r-...)",
        required_unless_present = "all",
        conflicts_with = "all"
    )]
    pub run: Option<String>,
    #[arg(long, help = "list worker agents across every idea and run")]
    pub all: bool,
    #[arg(long, requires = "all", help = "with --all, only running jobs")]
    pub active: bool,
    #[arg(long, help = "output JSON")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryAgentSendArgs {
    #[arg(help = "run ID (r-...)")]
    pub run: String,
    #[arg(help = "job ID (<run>.<stage>.<attempt>)")]
    pub job: String,
    #[arg(long, help = "path to message file")]
    pub file: String,
    #[arg(long, help = "output JSON")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryAgentCaptureArgs {
    #[arg(help = "run ID (r-...)")]
    pub run: String,
    #[arg(help = "job ID (<run>.<stage>.<attempt>)")]
    pub job: String,
    #[arg(long, default_value_t = 50, help = "number of lines to capture")]
    pub lines: usize,
    #[arg(long, help = "output JSON")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryAgentTerminateArgs {
    #[arg(help = "run ID (r-...)")]
    pub run: String,
    #[arg(help = "job ID (<run>.<stage>.<attempt>)")]
    pub job: String,
    #[arg(long, help = "must match job ID")]
    pub confirm: String,
    #[arg(long, help = "output JSON")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryAgentRelaunchArgs {
    #[arg(help = "run ID (r-...)")]
    pub run: String,
    #[arg(help = "job ID (<run>.<stage>.<attempt>)")]
    pub job: String,
    #[arg(long, help = "output JSON")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryLoginArgs {
    #[command(subcommand)]
    pub command: FactoryLoginCommand,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum FactoryLoginCommand {
    #[command(about = "check install and sign-in status for worker CLIs")]
    Status(FactoryLoginStatusArgs),
    #[command(about = "open a CLI's own sign-in flow in a login pane")]
    Open(FactoryLoginOpenArgs),
    #[command(about = "show whether each CLI's sign-in method is risky for orchestrated use")]
    Risk(FactoryLoginStatusArgs),
    #[command(about = "confirm you accept the risk of a CLI's current sign-in method (user only)")]
    AcknowledgeRisk(FactoryLoginAcknowledgeRiskArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryLoginAcknowledgeRiskArgs {
    #[arg(help = "CLI name (claude, agy, or opencode)")]
    pub cli: String,

    #[arg(
        long,
        help = "sign-in method being acknowledged (as shown by `login risk`)"
    )]
    pub method: String,

    #[arg(long, help = "type the CLI name to confirm")]
    pub confirm: String,

    #[arg(long, help = "output JSON")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryLoginStatusArgs {
    #[arg(long, help = "output JSON")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryLoginOpenArgs {
    #[arg(help = "CLI name (claude, agy, or opencode)")]
    pub cli: String,

    #[arg(long, help = "output JSON")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
#[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
pub struct FactoryModelsArgs {
    #[command(subcommand)]
    pub command: Option<FactoryModelsCommand>,

    #[arg(
        long,
        required = true,
        help = "CLI provider (claude, agy, or opencode)"
    )]
    pub cli: Option<String>,

    #[arg(long, help = "bypass cache and re-discover models from the CLI")]
    pub refresh: bool,

    #[arg(long, help = "output JSON (including structured errors)")]
    pub json: bool,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum FactoryModelsCommand {
    #[command(about = "recommend the least-cost capable CLI and model for a run stage")]
    Recommend(FactoryModelsRecommendArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryModelsRecommendArgs {
    #[arg(long, help = "run ID (r-...)")]
    pub run: String,

    #[arg(long, help = "stage name")]
    pub stage: String,

    #[arg(long, help = "task complexity hint: low, medium, or high")]
    pub complexity: Option<String>,

    #[arg(long, help = "minimum tier: economy, standard, or premium")]
    pub required_tier: Option<String>,

    #[arg(long, help = "output JSON (including structured errors)")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryReportArgs {
    #[arg(long, help = "output JSON")]
    pub json: bool,

    #[arg(long, help = "output Markdown")]
    pub markdown: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryDoctorArgs {
    #[arg(
        long,
        help = "apply safe repairs and interactively remove clean orphan worktrees"
    )]
    pub fix: bool,

    #[arg(long, help = "output JSON")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
#[command(args_conflicts_with_subcommands = true)]
pub struct FactoryAuditArgs {
    #[command(subcommand)]
    pub command: Option<FactoryAuditCommand>,

    #[arg(value_name = "IDEA", help = "idea ID to inspect")]
    pub idea: Option<String>,

    #[arg(long, conflicts_with = "markdown", help = "output JSON")]
    pub json: bool,

    #[arg(long, conflicts_with = "json", help = "output redacted Markdown")]
    pub markdown: bool,

    #[arg(long, value_name = "RUN", help = "filter by run ID")]
    pub run: Option<String>,

    #[arg(
        long,
        value_name = "KIND",
        help = "filter by kind (job_start, job_end, approval, decision, lifecycle)"
    )]
    pub kind: Option<String>,

    #[arg(long, value_name = "ACTOR", help = "filter by actor")]
    pub actor: Option<String>,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum FactoryAuditCommand {
    #[command(about = "verify the SHA-256 prev hash chain of the event log")]
    Verify(FactoryAuditVerifyArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryAuditVerifyArgs {
    #[arg(long, help = "output JSON")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRedactArgs {
    #[command(subcommand)]
    pub command: FactoryRedactCommand,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum FactoryRedactCommand {
    #[command(about = "scan a file and report what would be redacted")]
    Test(FactoryRedactTestArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRedactTestArgs {
    pub file: std::path::PathBuf,

    #[arg(long, help = "output JSON")]
    pub json: bool,
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

    #[command(about = "cancel an idea and any active run")]
    Cancel(FactoryIdeaCancelArgs),

    #[command(about = "resolve the currently bound idea id for this pane or environment")]
    Current(FactoryIdeaCurrentArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryIdeaCurrentArgs {
    #[arg(long, help = "output JSON (including structured errors)")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryIdeaReadyArgs {
    pub id: String,
    #[arg(long)]
    pub confirm: String,
    #[arg(long)]
    pub workflow: Option<String>,
    #[arg(long)]
    pub deploy: bool,
    #[arg(long)]
    pub max_cost: Option<String>,
    #[arg(long)]
    pub max_duration: Option<u64>,
    #[arg(long)]
    pub max_retries: Option<u32>,
    #[arg(long)]
    pub accept_estimate: bool,
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
pub struct FactoryIdeaCancelArgs {
    pub id: String,
    #[arg(long)]
    pub confirm: String,
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
    #[command(about = "grant one more retry attempt on a paused stage")]
    Retry(FactoryRunRetryArgs),
    #[command(about = "pause a running or queued run")]
    Pause(FactoryRunPauseArgs),
    #[command(about = "resume a paused run from the last valid stage")]
    Resume(FactoryRunResumeArgs),
    #[command(about = "view or update budget caps for a run")]
    Budget(FactoryRunBudgetArgs),
    #[command(about = "cancel an active run while keeping its worktree and branch")]
    Cancel(FactoryRunCancelArgs),
    #[command(about = "tail a job log for a run")]
    Log(FactoryRunLogArgs),
    #[command(about = "list or inspect evidence items for a run")]
    Evidence(FactoryRunEvidenceArgs),
    #[command(about = "list or refresh pull request status for a run")]
    Prs(FactoryRunPrsArgs),
    #[command(about = "copy plan stage specs into workspace specs/ under next free phase")]
    ExportSpecs(FactoryRunExportSpecsArgs),
    #[command(about = "show usage, cost basis, and rework signals for a run")]
    Usage(FactoryRunUsageArgs),
    #[command(about = "show what data was sent and readable for each job in a run")]
    Data(FactoryRunDataArgs),
    #[command(about = "grant a one-time Gate 3 approval for a protected action (user-only)")]
    Approve(FactoryRunApproveArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunDataArgs {
    pub run: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunApproveArgs {
    pub run: String,
    #[arg(long)]
    pub action: String,
    #[arg(long)]
    pub confirm: String,
    #[arg(long)]
    pub stage: Option<String>,
    #[arg(long)]
    pub json: bool,
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
pub struct FactoryRunRetryArgs {
    pub run: String,
    pub stage: String,
    #[arg(long)]
    pub hypothesis: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunPauseArgs {
    pub run: String,
    #[arg(long, help = "stop running jobs immediately")]
    pub now: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunResumeArgs {
    pub run: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunBudgetArgs {
    pub run: String,
    #[arg(long)]
    pub max_cost: Option<String>,
    #[arg(long)]
    pub max_duration: Option<u64>,
    #[arg(long)]
    pub max_retries: Option<u32>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunCancelArgs {
    pub run: String,
    #[arg(long)]
    pub confirm: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunLogArgs {
    pub run: String,
    pub job: String,
    #[arg(long, default_value_t = 65536)]
    pub tail: usize,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunEvidenceArgs {
    pub run: String,
    #[arg(long)]
    pub show: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunPrsArgs {
    pub run: String,
    #[arg(long)]
    pub refresh: bool,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunExportSpecsArgs {
    pub run: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryRunUsageArgs {
    pub run: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryChecksArgs {
    #[command(subcommand)]
    pub command: FactoryChecksCommand,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum FactoryChecksCommand {
    #[command(about = "propose test and lint commands by reading target files")]
    Discover(FactoryChecksDiscoverArgs),
    #[command(about = "confirm test and lint commands for a target (user-only)")]
    Set(FactoryChecksSetArgs),
    #[command(about = "show configured check commands")]
    Show(FactoryChecksShowArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryChecksDiscoverArgs {
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryChecksSetArgs {
    #[arg(long)]
    pub target: String,
    #[arg(long)]
    pub test: Option<String>,
    #[arg(long)]
    pub lint: Option<String>,
    #[arg(long)]
    pub deploy: Option<String>,
    #[arg(long)]
    pub timeout_secs: Option<u64>,
    #[arg(long)]
    pub ui_paths: Option<String>,
    #[arg(long)]
    pub confirm: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryChecksShowArgs {
    #[arg(long)]
    pub target: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryIdeaStartArgs {
    #[arg(long, help = "CLI provider (claude, agy, or opencode)")]
    pub cli: String,

    #[arg(long, help = "model id")]
    pub model: String,

    #[arg(long, help = "optional idea title")]
    pub title: Option<String>,

    #[arg(
        long = "text-file",
        conflicts_with = "text_stdin",
        help = "read the idea description from a file, or '-' for stdin"
    )]
    pub text_file: Option<String>,

    #[arg(
        long = "text-stdin",
        help = "read the idea description from stdin (same as --text-file -)"
    )]
    pub text_stdin: bool,

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

    #[arg(
        long = "start-discovery",
        help = "immediately transition the new idea from Inbox to Ideation"
    )]
    pub start_discovery: bool,

    #[arg(
        long = "bind-current",
        help = "bind the active orchestrator metadata and current tmux pane to the new idea"
    )]
    pub bind_current: bool,

    #[arg(
        long,
        help = "orchestrator CLI provider (claude, agy, or opencode) when binding"
    )]
    pub cli: Option<String>,

    #[arg(long, help = "orchestrator model id when binding")]
    pub model: Option<String>,

    #[arg(long, help = "optional orchestrator conversation id when binding")]
    pub conversation: Option<String>,

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
pub struct FactorySettingsArgs {
    #[command(subcommand)]
    pub command: FactorySettingsCommand,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum FactorySettingsCommand {
    #[command(about = "show current factory settings")]
    Get(FactorySettingsGetArgs),

    #[command(about = "update a factory setting (user-only)")]
    Set(FactorySettingsSetArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactorySettingsGetArgs {
    #[arg(long, help = "output JSON (including structured errors)")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactorySettingsSetArgs {
    #[arg(help = "setting key")]
    pub key: String,

    #[arg(help = "setting value (scalar or JSON)")]
    pub value: String,

    #[arg(long, help = "output JSON (including structured errors)")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryPermissionsArgs {
    #[command(subcommand)]
    pub command: FactoryPermissionsCommand,
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum FactoryPermissionsCommand {
    #[command(about = "allow a tool for an agent (or un-deny for --target) (user-only)")]
    Allow(FactoryPermissionsMutateArgs),

    #[command(about = "deny a tool for a target repository (or --agent) (user-only)")]
    Deny(FactoryPermissionsMutateArgs),

    #[command(about = "show configured and default tool permissions")]
    Show(FactoryPermissionsShowArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryPermissionsMutateArgs {
    #[arg(help = "agent name (for allow) or target name (for deny)")]
    pub subject: String,

    #[arg(help = "workflow tool name")]
    pub tool: String,

    #[arg(long, help = "apply to a target repository instead of an agent")]
    pub target: bool,

    #[arg(long, help = "apply to an agent instead of a target repository")]
    pub agent: bool,

    #[arg(long, help = "output JSON (including structured errors)")]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct FactoryPermissionsShowArgs {
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
            FactoryIdeaCommand::Cancel(a) => a.json,
            FactoryIdeaCommand::Current(a) => a.json,
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
            FactoryRunCommand::Retry(a) => a.json,
            FactoryRunCommand::Pause(a) => a.json,
            FactoryRunCommand::Resume(a) => a.json,
            FactoryRunCommand::Budget(a) => a.json,
            FactoryRunCommand::Cancel(a) => a.json,
            FactoryRunCommand::Log(a) => a.json,
            FactoryRunCommand::Evidence(a) => a.json,
            FactoryRunCommand::Prs(a) => a.json,
            FactoryRunCommand::ExportSpecs(a) => a.json,
            FactoryRunCommand::Usage(a) => a.json,
            FactoryRunCommand::Data(a) => a.json,
            FactoryRunCommand::Approve(a) => a.json,
        },
        FactoryCommand::Checks(c_args) => match &c_args.command {
            FactoryChecksCommand::Discover(a) => a.json,
            FactoryChecksCommand::Set(a) => a.json,
            FactoryChecksCommand::Show(a) => a.json,
        },
        FactoryCommand::Settings(s_args) => match &s_args.command {
            FactorySettingsCommand::Get(a) => a.json,
            FactorySettingsCommand::Set(a) => a.json,
        },
        FactoryCommand::Permissions(p_args) => match &p_args.command {
            FactoryPermissionsCommand::Allow(a) | FactoryPermissionsCommand::Deny(a) => a.json,
            FactoryPermissionsCommand::Show(a) => a.json,
        },
        FactoryCommand::Runner => false,
        FactoryCommand::Events(a) => a.json,
        FactoryCommand::Rebuild(a) => a.json,
        FactoryCommand::Redact(r_args) => match &r_args.command {
            FactoryRedactCommand::Test(a) => a.json,
        },
        FactoryCommand::Audit(a) => match &a.command {
            Some(FactoryAuditCommand::Verify(v)) => v.json,
            None => a.json,
        },
        FactoryCommand::Doctor(a) => a.json,
        FactoryCommand::Report(a) => a.json,
        FactoryCommand::Models(a) => match &a.command {
            Some(FactoryModelsCommand::Recommend(r)) => r.json,
            None => a.json,
        },
        FactoryCommand::Login(l_args) => match &l_args.command {
            FactoryLoginCommand::Status(a) => a.json,
            FactoryLoginCommand::Open(a) => a.json,
            FactoryLoginCommand::Risk(a) => a.json,
            FactoryLoginCommand::AcknowledgeRisk(a) => a.json,
        },
        FactoryCommand::Agent(a_args) => match &a_args.command {
            FactoryAgentCommand::Assign(a) => a.json,
            FactoryAgentCommand::List(a) => a.json,
            FactoryAgentCommand::Send(a) => a.json,
            FactoryAgentCommand::Capture(a) => a.json,
            FactoryAgentCommand::Terminate(a) => a.json,
            FactoryAgentCommand::Relaunch(a) => a.json,
        },
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

fn lookup_tmux_pane_option(workspace: &Path, pane_id: &str, option: &str) -> Option<String> {
    let is_valid_pane =
        pane_id.starts_with('%') && pane_id[1..].chars().all(|c| c.is_ascii_digit());
    if !is_valid_pane {
        return None;
    }
    let socket = std::env::var("TMUX")
        .ok()
        .and_then(|v| {
            let sock_path = v.split(',').next()?;
            Path::new(sock_path)
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| crate::tmux::client::socket_for(workspace));
    let client = crate::tmux::client::TmuxClient::new(socket, None);
    let fmt = format!("#{{{option}}}");
    let res = client
        .run(&["display-message", "-p", "-t", pane_id, &fmt])
        .ok()?;
    if res.code != 0 {
        return None;
    }
    let val = res.out.trim().to_string();
    if val.is_empty() { None } else { Some(val) }
}

fn lookup_tmux_pane_role(workspace: &Path, pane_id: &str) -> Option<String> {
    lookup_tmux_pane_option(workspace, pane_id, "@tandem_role")
}

fn resolve_active_pane_id(config: &Config, caller: &super::approval::Caller<'_>) -> Option<String> {
    if let Some(p) = (caller.env)("TMUX_PANE")
        && !p.trim().is_empty()
    {
        return Some(p.trim().to_string());
    }
    if !config.tmux_pane.trim().is_empty() {
        return Some(config.tmux_pane.trim().to_string());
    }
    None
}

fn resolve_orchestrator_binding(
    workspace: &Path,
    config: &Config,
    caller: &super::approval::Caller<'_>,
    explicit_cli: Option<&str>,
    explicit_model: Option<&str>,
    explicit_conv: Option<&str>,
) -> Result<Option<(String, String, Option<String>)>, CmdError> {
    let pane_spec = resolve_active_pane_id(config, caller)
        .and_then(|pane_id| lookup_tmux_pane_option(workspace, &pane_id, "@tandem_spec"))
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());

    let cli = explicit_cli
        .map(str::to_string)
        .or_else(|| {
            pane_spec
                .as_ref()
                .and_then(|v| v.get("cli").and_then(|c| c.as_str()).map(str::to_string))
        })
        .or_else(|| (caller.env)("TANDEM_CLI"))
        .or_else(|| std::env::var("TANDEM_CLI").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let model = explicit_model
        .map(str::to_string)
        .or_else(|| {
            pane_spec
                .as_ref()
                .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(str::to_string))
        })
        .or_else(|| (caller.env)("TANDEM_MODEL"))
        .or_else(|| std::env::var("TANDEM_MODEL").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let conversation = explicit_conv
        .map(str::to_string)
        .or_else(|| {
            pane_spec.as_ref().and_then(|v| {
                v.get("conversation")
                    .and_then(|c| c.as_str())
                    .map(str::to_string)
            })
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    match (cli, model) {
        (Some(c), Some(m)) => {
            if !crate::state::CLIS.contains(&c.as_str()) {
                return Err(CmdError::invalid_input(format!(
                    "unsupported orchestrator cli: {c} (expected claude, agy, or opencode)"
                )));
            }
            let valid_model = crate::adapters::base::validate_model_for_cli(&c, &m)
                .map_err(|e| CmdError::invalid_input(e.to_string()))?;
            Ok(Some((c, valid_model, conversation)))
        }
        (Some(c), None) if explicit_cli.is_some() => Err(CmdError::invalid_input(format!(
            "--model is required when --cli {c} is specified"
        ))),
        (None, Some(m)) if explicit_model.is_some() => Err(CmdError::invalid_input(format!(
            "--cli is required when --model {m} is specified"
        ))),
        _ => Ok(None),
    }
}

fn rebind_tmux_pane_idea(
    workspace: &Path,
    pane_id: &str,
    idea_id: &str,
    binding: Option<&(String, String, Option<String>)>,
) {
    let is_valid_pane =
        pane_id.starts_with('%') && pane_id[1..].chars().all(|c| c.is_ascii_digit());
    if !is_valid_pane {
        return;
    }
    let socket = std::env::var("TMUX")
        .ok()
        .and_then(|v| {
            let sock_path = v.split(',').next()?;
            Path::new(sock_path)
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| crate::tmux::client::socket_for(workspace));
    let client = crate::tmux::client::TmuxClient::new(socket, None);
    let mut batch = vec![vec![
        "set-option".to_string(),
        "-p".to_string(),
        "-t".to_string(),
        pane_id.to_string(),
        "@tandem_idea".to_string(),
        idea_id.to_string(),
    ]];
    if let Some((cli, model, _)) = binding {
        batch.push(vec![
            "set-option".to_string(),
            "-p".to_string(),
            "-t".to_string(),
            pane_id.to_string(),
            "@tandem_label".to_string(),
            format!("{idea_id} {cli}·{model}"),
        ]);
    }
    if let Some(raw_spec) = lookup_tmux_pane_option(workspace, pane_id, "@tandem_spec")
        && let Ok(mut spec_val) = serde_json::from_str::<serde_json::Value>(&raw_spec)
        && let Some(obj) = spec_val.as_object_mut()
    {
        obj.insert(
            "idea".to_string(),
            serde_json::Value::String(idea_id.to_string()),
        );
        if let Ok(updated_spec) = serde_json::to_string(&spec_val) {
            batch.push(vec![
                "set-option".to_string(),
                "-p".to_string(),
                "-t".to_string(),
                pane_id.to_string(),
                "@tandem_spec".to_string(),
                updated_spec,
            ]);
        }
    }
    let _ = client.run_batch_checked(&batch);
}

fn is_authenticated_web_gateway(config: &Config) -> bool {
    if std::env::var("TANDEM_INTERNAL_SURFACE").ok().as_deref() != Some("web") {
        return false;
    }
    let Ok(token) = std::env::var("TANDEM_AUTH_TOKEN") else {
        return false;
    };
    if token.trim().is_empty() {
        return false;
    }
    let cfg_path = crate::web_manager::web_config_path(&config.state_dir);
    let Ok(web_cfg) = crate::web_manager::load_web_config(&cfg_path) else {
        return false;
    };
    !web_cfg.auth_token.is_empty() && web_cfg.auth_token == token
}

pub fn run_with_io(
    args: &FactoryArgs,
    workspace: &Path,
    config: &Config,
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    use std::io::IsTerminal;
    let env_fn = |k: &str| std::env::var(k).ok();
    let pane_fn = |pane: &str| lookup_tmux_pane_role(workspace, pane);
    let caller = if is_authenticated_web_gateway(config) {
        super::approval::Caller::web(&env_fn)
    } else {
        super::approval::Caller::cli(std::io::stdin().is_terminal(), &env_fn, &pane_fn)
    };
    run_with_caller_io(args, workspace, config, &caller, stdin, stdout, stderr)
}

pub fn run_with_caller(
    args: &FactoryArgs,
    workspace: &Path,
    config: &Config,
    caller: &super::approval::Caller<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    run_with_caller_and_probe(
        args,
        workspace,
        config,
        caller,
        &super::login::HostLoginProbe,
        stdout,
        stderr,
    )
}

pub fn run_with_caller_and_probe(
    args: &FactoryArgs,
    workspace: &Path,
    config: &Config,
    caller: &super::approval::Caller<'_>,
    probe: &dyn super::login::LoginProbe,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let mut empty = std::io::empty();
    run_with_caller_io_and_probe(
        args, workspace, config, caller, probe, &mut empty, stdout, stderr,
    )
}

pub fn run_with_caller_io(
    args: &FactoryArgs,
    workspace: &Path,
    config: &Config,
    caller: &super::approval::Caller<'_>,
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    run_with_caller_io_and_probe(
        args,
        workspace,
        config,
        caller,
        &super::login::HostLoginProbe,
        stdin,
        stdout,
        stderr,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_with_caller_io_and_probe(
    args: &FactoryArgs,
    workspace: &Path,
    config: &Config,
    caller: &super::approval::Caller<'_>,
    probe: &dyn super::login::LoginProbe,
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
    if let FactoryCommand::Doctor(doc_args) = &args.command {
        return run_doctor_command(doc_args, workspace, caller, stdin, stdout, stderr);
    }
    match execute_with_probe(args, workspace, config, caller, probe, stdin) {
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

fn execute_with_probe(
    args: &FactoryArgs,
    workspace: &Path,
    config: &Config,
    caller: &super::approval::Caller<'_>,
    probe: &dyn super::login::LoginProbe,
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
                let binding = if a.bind_current || a.cli.is_some() || a.model.is_some() {
                    resolve_orchestrator_binding(
                        workspace,
                        config,
                        caller,
                        a.cli.as_deref(),
                        a.model.as_deref(),
                        a.conversation.as_deref(),
                    )?
                } else {
                    None
                };
                let mut idea = store
                    .create_idea(&title, &text, &owner, &actor)
                    .map_err(map_factory_error)?;
                if a.start_discovery || (a.bind_current && !text.trim().is_empty()) {
                    idea = store
                        .execute(&idea.id, super::machine::Command::StartDiscovery, &actor)
                        .map_err(map_factory_error)?;
                }
                if let Some((ref cli, ref model, ref conversation)) = binding {
                    idea = store
                        .execute(
                            &idea.id,
                            super::machine::Command::BindOrchestrator {
                                cli: cli.clone(),
                                model: model.clone(),
                                conversation: conversation.clone(),
                            },
                            &actor,
                        )
                        .map_err(map_factory_error)?;
                }
                if a.bind_current
                    && let Some(pane_id) = resolve_active_pane_id(config, caller)
                {
                    rebind_tmux_pane_idea(workspace, &pane_id, &idea.id, binding.as_ref());
                }
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
                let idea_events = if store.exists() {
                    events::read_idea_events(&store.root.join("events.jsonl"), &a.id)
                        .map_err(map_factory_error)?
                } else {
                    Vec::new()
                };
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
                let text_source = if a.text_stdin {
                    Some("-")
                } else {
                    a.text_file.as_deref()
                };
                let text = match text_source {
                    Some(source) => read_text_input(None, Some(source), stdin)?,
                    None => String::new(),
                };
                ensure_cli_risk_acknowledged(&store, &a.cli, probe)?;
                let bound = start_idea_cmd(
                    workspace,
                    config,
                    &a.cli,
                    &a.model,
                    a.title.as_deref(),
                    &text,
                )?;
                serialize_compact(&bound)
            }
            FactoryIdeaCommand::Resume(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                if let Some(orch) = store
                    .get_idea(&a.id)
                    .map_err(map_factory_error)?
                    .and_then(|i| i.orchestrator)
                {
                    ensure_cli_risk_acknowledged(&store, &orch.cli, probe)?;
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
                let max_cost_usd = a
                    .max_cost
                    .as_deref()
                    .map(|raw| {
                        raw.trim().parse::<f64>().map_err(|_| {
                            CmdError::invalid_input(format!("invalid --max-cost value: {raw}"))
                        })
                    })
                    .transpose()?;
                let res = super::approval::mark_ready_with_options(
                    &store,
                    workspace,
                    &a.id,
                    &a.confirm,
                    &approved_by,
                    super::approval::MarkReadyOptions {
                        workflow_override: a.workflow.clone(),
                        deploy_requested: a.deploy,
                        max_cost_usd,
                        max_duration_secs: a.max_duration,
                        max_retries: a.max_retries,
                        accept_estimate: a.accept_estimate,
                    },
                    caller,
                )
                .map_err(|e| CmdError {
                    exit_code: e.exit_code(),
                    code: e.code(),
                    message: e.to_string(),
                })?;
                if a.json {
                    serialize_compact(&res)
                } else {
                    Ok(format_ready_text(&res))
                }
            }
            FactoryIdeaCommand::RevokeReady(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let user_id = resolve_owner(None)?;
                let updated = super::approval::revoke_ready(&store, &a.id, &user_id, caller)
                    .map_err(|e| CmdError {
                        exit_code: e.exit_code(),
                        code: e.code(),
                        message: e.to_string(),
                    })?;
                serialize_compact(&updated)
            }
            FactoryIdeaCommand::Cancel(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let actor = Actor {
                    kind: ActorKind::User,
                    id: resolve_owner(None)?,
                };
                let res = super::runner::cancel_idea(&store, &a.id, &a.confirm, &actor, caller)
                    .map_err(|e| CmdError {
                        exit_code: e.exit_code(),
                        code: e.code(),
                        message: e.to_string(),
                    })?;
                if a.json {
                    serialize_compact(&res)
                } else {
                    Ok(format_cancel_idea_text(&res))
                }
            }
            FactoryIdeaCommand::Current(a) => {
                let pane_idea = resolve_active_pane_id(config, caller)
                    .and_then(|pane_id| {
                        lookup_tmux_pane_option(workspace, &pane_id, "@tandem_idea")
                    })
                    .filter(|id| is_valid_idea_id(id));
                let env_idea = (caller.env)("TANDEM_FACTORY_IDEA")
                    .map(|s| s.trim().to_string())
                    .filter(|id| is_valid_idea_id(id));
                let (idea_opt, source) = if let Some(id) = pane_idea {
                    (Some(id), "pane")
                } else if let Some(id) = env_idea {
                    (Some(id), "env")
                } else {
                    (None, "none")
                };
                if a.json {
                    serialize_compact(&serde_json::json!({
                        "idea": idea_opt,
                        "source": source,
                    }))
                } else {
                    Ok(idea_opt.unwrap_or_default())
                }
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
                super::approval::require_user(caller).map_err(|e| CmdError {
                    exit_code: e.exit_code(),
                    code: e.code(),
                    message: e.to_string(),
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
                        budget: None,
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
                super::approval::require_user(caller).map_err(|e| CmdError {
                    exit_code: e.exit_code(),
                    code: e.code(),
                    message: e.to_string(),
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
                let run =
                    super::workspace::set_run_target(&store, workspace, &a.run, &a.name, caller)
                        .map_err(|e| match e {
                            super::workspace::WorkspaceError::Approval(ae) => CmdError {
                                exit_code: ae.exit_code(),
                                code: ae.code(),
                                message: ae.to_string(),
                            },
                            other => CmdError::invalid_input(other.to_string()),
                        })?;
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
                super::workspace::cleanup_worktree(&store, workspace, &a.run, &actor, caller)
                    .map_err(|e| CmdError::invalid_input(e.to_string()))?;
                serialize_compact(&serde_json::json!({ "cleaned": a.run }))
            }
            FactoryRunCommand::Retry(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                super::approval::require_user(caller).map_err(|e| CmdError {
                    exit_code: e.exit_code(),
                    code: e.code(),
                    message: e.to_string(),
                })?;
                let actor = Actor {
                    kind: ActorKind::User,
                    id: resolve_owner(None)?,
                };
                let run = super::runner::retry_paused_stage_with_probe(
                    &store,
                    &a.run,
                    &a.stage,
                    &a.hypothesis,
                    &actor,
                    caller,
                    probe,
                )
                .map_err(|e| CmdError {
                    exit_code: e.exit_code(),
                    code: e.code(),
                    message: e.to_string(),
                })?;
                serialize_compact(&run)
            }
            FactoryRunCommand::Pause(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let actor = Actor {
                    kind: ActorKind::User,
                    id: resolve_owner(None)?,
                };
                let res = super::runner::pause_run_by_user(&store, &a.run, a.now, &actor, caller)
                    .map_err(|e| CmdError {
                    exit_code: e.exit_code(),
                    code: e.code(),
                    message: e.to_string(),
                })?;
                if a.json {
                    serialize_compact(&res)
                } else {
                    Ok(format_pause_run_text(&res, a.now))
                }
            }
            FactoryRunCommand::Resume(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                super::approval::require_user(caller).map_err(|e| CmdError {
                    exit_code: e.exit_code(),
                    code: e.code(),
                    message: e.to_string(),
                })?;
                let actor = Actor {
                    kind: ActorKind::User,
                    id: resolve_owner(None)?,
                };
                let run =
                    super::runner::resume_paused_run_with_probe(&store, &a.run, &actor, probe)
                        .map_err(|e| CmdError {
                            exit_code: e.exit_code(),
                            code: e.code(),
                            message: e.to_string(),
                        })?;
                if a.json {
                    serialize_compact(&run)
                } else {
                    Ok(format!("Run {} resumed.", run.id))
                }
            }
            FactoryRunCommand::Budget(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let max_cost_usd = a
                    .max_cost
                    .as_deref()
                    .map(|raw| {
                        raw.trim().parse::<f64>().map_err(|_| {
                            CmdError::invalid_input(format!("invalid --max-cost value: {raw}"))
                        })
                    })
                    .transpose()?;
                let actor = Actor {
                    kind: ActorKind::User,
                    id: resolve_owner(None)?,
                };
                let res = super::runner::change_run_budget(
                    &store,
                    &a.run,
                    max_cost_usd,
                    a.max_duration,
                    a.max_retries,
                    &actor,
                    caller,
                )
                .map_err(|e| CmdError {
                    exit_code: e.exit_code(),
                    code: e.code(),
                    message: e.to_string(),
                })?;
                if a.json {
                    serialize_compact(&res)
                } else {
                    Ok(format_budget_change_text(&res))
                }
            }
            FactoryRunCommand::Cancel(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let actor = Actor {
                    kind: ActorKind::User,
                    id: resolve_owner(None)?,
                };
                let res = super::runner::cancel_run(&store, &a.run, &a.confirm, &actor, caller)
                    .map_err(|e| CmdError {
                        exit_code: e.exit_code(),
                        code: e.code(),
                        message: e.to_string(),
                    })?;
                if a.json {
                    serialize_compact(&res)
                } else {
                    Ok(format_cancel_run_text(&res))
                }
            }
            FactoryRunCommand::Log(a) => read_run_job_log(&store, a),
            FactoryRunCommand::Evidence(a) => run_evidence_cmd(&store, workspace, a),
            FactoryRunCommand::Prs(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let prs = if a.refresh {
                    let actor = Actor {
                        kind: ActorKind::User,
                        id: resolve_owner(None)?,
                    };
                    super::handoff::refresh_run_prs(&store, workspace, &a.run, &actor).map_err(
                        |e| CmdError {
                            exit_code: e.exit_code(),
                            code: e.code(),
                            message: e.to_string(),
                        },
                    )?
                } else {
                    let run = super::run::get_run(&store, &a.run)
                        .map_err(|e| CmdError {
                            exit_code: 1,
                            code: e.code(),
                            message: e.to_string(),
                        })?
                        .ok_or_else(|| CmdError::not_found(format!("run {} not found", a.run)))?;
                    run.prs
                };
                if a.json {
                    serialize_compact(&prs)
                } else if prs.is_empty() {
                    Ok("No pull requests recorded.".to_string())
                } else {
                    let lines: Vec<String> = prs
                        .iter()
                        .map(|pr| {
                            let state = pr.state.as_deref().unwrap_or("unknown");
                            let checks = pr.checks_summary.as_deref().unwrap_or("not checked");
                            format!(
                                "{} #{}: {} [state: {state}, checks: {checks}]",
                                pr.repo, pr.number, pr.url
                            )
                        })
                        .collect();
                    Ok(lines.join("\n"))
                }
            }
            FactoryRunCommand::ExportSpecs(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let res = super::handoff::export_specs(&store, workspace, &a.run, caller).map_err(
                    |e| CmdError {
                        exit_code: e.exit_code(),
                        code: e.code(),
                        message: e.to_string(),
                    },
                )?;
                if a.json {
                    serialize_compact(&res)
                } else {
                    Ok(format!(
                        "Exported {} spec file(s) to {}",
                        res.files.len(),
                        res.destination.display()
                    ))
                }
            }
            FactoryRunCommand::Usage(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let report = super::telemetry::run_usage(&store, &a.run)
                    .map_err(|e| CmdError::invalid_input(e.to_string()))?;
                if a.json {
                    serialize_compact(&report)
                } else {
                    Ok(format_run_usage_text(&report))
                }
            }
            FactoryRunCommand::Data(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                if !super::ids::is_valid_run_id(&a.run) {
                    return Err(CmdError::invalid_input(format!(
                        "invalid run id: {}",
                        a.run
                    )));
                }
                let run = super::run::get_run(&store, &a.run)
                    .map_err(|e| CmdError {
                        exit_code: 1,
                        code: e.code(),
                        message: e.to_string(),
                    })?
                    .ok_or_else(|| CmdError::not_found(format!("run {} not found", a.run)))?;
                let view = super::data_policy::build_run_data(&store, &run)
                    .map_err(|e| CmdError::invalid_input(e.to_string()))?;
                if a.json {
                    serialize_compact(&view)
                } else {
                    Ok(super::data_policy::format_run_data_text(&view))
                }
            }
            FactoryRunCommand::Approve(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let action = super::protected::ProtectedAction::parse(&a.action).ok_or_else(|| {
                    CmdError::invalid_input(format!(
                        "invalid --action '{}': expected production_deploy, secrets_access, or production_access",
                        a.action
                    ))
                })?;
                let approved_by = resolve_owner(None)?;
                let outcome = super::protected::approve_with_details(
                    &store,
                    &a.run,
                    action,
                    &a.confirm,
                    a.stage.as_deref(),
                    &approved_by,
                    caller,
                )
                .map_err(|e| CmdError {
                    exit_code: e.exit_code(),
                    code: e.code(),
                    message: e.to_string(),
                })?;
                if a.json {
                    serialize_compact(&outcome)
                } else {
                    let mut lines = Vec::new();
                    if let Some(cmd) = &outcome.deploy_command {
                        lines.push(format!(
                            "This lets run {} execute the confirmed deploy command once:",
                            outcome.approval.run
                        ));
                        lines.push(format!("  {}", cmd.join(" ")));
                    } else {
                        lines.push(format!(
                            "Granted {} approval for run {} stage {}.",
                            outcome.approval.action, outcome.approval.run, outcome.approval.stage
                        ));
                    }
                    lines.push(format!(
                        "Approval recorded. It expires in {} minutes.",
                        outcome.expires_in_minutes
                    ));
                    Ok(lines.join("\n"))
                }
            }
        },
        FactoryCommand::Checks(c_args) => {
            if !enabled {
                return Err(CmdError::factory_disabled());
            }
            handle_checks_command(&c_args.command, &store, workspace, caller)
        }
        FactoryCommand::Settings(s_args) => {
            if !enabled {
                return Err(CmdError::factory_disabled());
            }
            match &s_args.command {
                FactorySettingsCommand::Get(a) => {
                    let settings = if store.exists() {
                        store.settings().map_err(map_factory_error)?
                    } else {
                        FactorySettings::default()
                    };
                    if a.json {
                        serialize_compact(&settings)
                    } else {
                        serde_json::to_string_pretty(&settings)
                            .map_err(|e| CmdError::invalid_input(e.to_string()))
                    }
                }
                FactorySettingsCommand::Set(a) => {
                    let actor = resolve_owner(None)?;
                    let res = super::settings::set(&store, &a.key, &a.value, &actor, caller)
                        .map_err(|e| CmdError {
                            exit_code: e.exit_code(),
                            code: e.code(),
                            message: e.to_string(),
                        })?;
                    if a.json {
                        serialize_compact(&res)
                    } else {
                        Ok(format!("{} = {}", res.key, res.new_value))
                    }
                }
            }
        }
        FactoryCommand::Permissions(p_args) => {
            if !enabled {
                return Err(CmdError::factory_disabled());
            }
            handle_permissions_command(&p_args.command, &store, caller)
        }
        FactoryCommand::Runner => {
            if !enabled {
                return Err(CmdError::factory_disabled());
            }
            let mut exec = super::executor::BrokerExecutor::new(workspace);
            super::runner::run_loop(workspace, &mut exec)
                .map_err(|e| CmdError::invalid_input(e.to_string()))?;
            Ok("{}".to_string())
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
        FactoryCommand::Redact(r_args) => match &r_args.command {
            FactoryRedactCommand::Test(a) => run_redact_test(a),
        },
        FactoryCommand::Audit(a) => run_audit_command(a, &store, enabled),
        FactoryCommand::Doctor(_) => unreachable!("handled in run_with_caller_io"),
        FactoryCommand::Models(a) => {
            if let Some(FactoryModelsCommand::Recommend(r)) = &a.command {
                return run_models_recommend(r, &store);
            }
            let cli = a.cli.clone().unwrap_or_default();
            if !crate::state::CLIS.contains(&cli.as_str()) {
                return Err(CmdError::invalid_input(format!(
                    "unsupported orchestrator cli: {cli} (expected claude, agy, or opencode)"
                )));
            }
            let settings = store.settings().map_err(map_factory_error)?;
            let runner = |cli: &str| -> Result<
                Vec<crate::adapters::base::Model>,
                crate::adapters::base::AdapterError,
            > {
                let adapter = crate::adapters::adapter_for(cli)
                    .map_err(|e| crate::adapters::base::AdapterError::UnknownCli(e.to_string()))?;
                adapter.list_models()
            };
            let catalog = super::model_catalog::discover(
                &cli,
                &settings,
                store.root_dir(),
                a.refresh,
                &runner,
            );
            if a.json {
                serialize_compact(&catalog)
            } else {
                Ok(catalog
                    .iter()
                    .map(|m| {
                        let tier = match m.tier {
                            super::routing::ModelTier::Economy => "economy",
                            super::routing::ModelTier::Standard => "standard",
                            super::routing::ModelTier::Premium => "premium",
                        };
                        let allowed = if m.allowed { "allowed" } else { "denied" };
                        let source = match m.source {
                            super::model_catalog::ModelSource::Live => "live",
                            super::model_catalog::ModelSource::Cache => "cache",
                            super::model_catalog::ModelSource::Fallback => "fallback",
                        };
                        format!(
                            "{}\t{}\t{}\t{:.4}\t{}\t{}",
                            m.id, m.label, tier, m.cost_rank, allowed, source
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
        }
        FactoryCommand::Report(a) => {
            let now = super::store::current_time();
            let report = super::telemetry::pilot_report(&store, now).map_err(map_factory_error)?;
            if a.json {
                serialize_compact(&report)
            } else if a.markdown {
                Ok(super::telemetry::format_pilot_report_markdown(&report))
            } else {
                Ok(super::telemetry::format_pilot_report_text(&report))
            }
        }
        FactoryCommand::Login(l_args) => match &l_args.command {
            FactoryLoginCommand::Status(a) => {
                let items = super::login::all_statuses(probe);
                let today = super::dates::today_iso_date();
                let risks = super::auth_risk::risk_report(
                    &store,
                    &items,
                    &today,
                    super::broker::terms::PROVIDER_TERMS,
                );
                if a.json {
                    #[derive(Serialize)]
                    struct LoginStatusRow {
                        #[serde(flatten)]
                        item: super::login::LoginStatusItem,
                        #[serde(skip_serializing_if = "Option::is_none")]
                        risk: Option<String>,
                        #[serde(skip_serializing_if = "Option::is_none")]
                        verdict: Option<String>,
                    }
                    #[derive(Serialize)]
                    struct LoginStatusEnvelope {
                        clis: Vec<LoginStatusRow>,
                    }
                    let clis = items
                        .into_iter()
                        .zip(risks)
                        .map(|(item, r)| LoginStatusRow {
                            item,
                            risk: r.risk,
                            verdict: r.verdict,
                        })
                        .collect();
                    serialize_compact(&LoginStatusEnvelope { clis })
                } else {
                    Ok(items
                        .iter()
                        .map(|item| {
                            let ver = if item.installed && !item.version.is_empty() {
                                item.version.as_str()
                            } else if item.installed {
                                "installed"
                            } else {
                                "not-installed"
                            };
                            let state = if !item.installed {
                                "Not installed"
                            } else if item.signed_in {
                                "Signed in"
                            } else {
                                "Not signed in"
                            };
                            let suffix = item
                                .method
                                .as_deref()
                                .map(|m| format!(" ({m})"))
                                .unwrap_or_default();
                            format!("{}\t{}\t{}{}", item.cli, ver, state, suffix)
                        })
                        .collect::<Vec<_>>()
                        .join("\n"))
                }
            }
            FactoryLoginCommand::Risk(a) => {
                let items = super::login::all_statuses(probe);
                let today = super::dates::today_iso_date();
                let risks = super::auth_risk::risk_report(
                    &store,
                    &items,
                    &today,
                    super::broker::terms::PROVIDER_TERMS,
                );
                if a.json {
                    serialize_compact(&risks)
                } else {
                    Ok(risks
                        .iter()
                        .map(|r| {
                            let state = match r.risk.as_deref() {
                                None => "not signed in".to_string(),
                                Some("ok") => "ok".to_string(),
                                Some("acknowledged") => "acknowledged".to_string(),
                                Some(_) => "RISKY".to_string(),
                            };
                            let verdict = r.verdict.as_deref().unwrap_or("no verified terms");
                            let mut line =
                                format!("{}\t{}\t{}\t{}", r.cli, r.method, verdict, state);
                            if let Some(cmd) = &r.acknowledge_command {
                                line.push_str(&format!("\t{cmd}"));
                            }
                            line
                        })
                        .collect::<Vec<_>>()
                        .join("\n"))
                }
            }
            FactoryLoginCommand::AcknowledgeRisk(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                super::approval::require_user(caller).map_err(|e| CmdError {
                    exit_code: e.exit_code(),
                    code: e.code(),
                    message: e.to_string(),
                })?;
                let detected = if super::auth_risk::KNOWN_CLIS.contains(&a.cli.as_str()) {
                    let item = super::login::check_status(&a.cli, probe);
                    if item.installed && item.signed_in {
                        super::auth_risk::detected_method(&item)
                    } else {
                        return Err(CmdError::invalid_input(format!(
                            "{} is not signed in; sign in first with `tandem factory login open {}`",
                            a.cli, a.cli
                        )));
                    }
                } else {
                    None
                };
                let by = resolve_owner(None)?;
                let ack = super::auth_risk::acknowledge(
                    &store, &a.cli, detected, &a.method, &a.confirm, &by, caller,
                )
                .map_err(|e| CmdError {
                    exit_code: e.exit_code(),
                    code: e.code(),
                    message: e.to_string(),
                })?;
                if a.json {
                    serialize_compact(&ack)
                } else {
                    Ok(format!(
                        "Acknowledged: {} sign-in via {} (terms verdict {}). Asked again if the terms are re-verified, expire, or the sign-in method changes.",
                        ack.cli, ack.method, ack.verdict
                    ))
                }
            }
            FactoryLoginCommand::Open(a) => {
                if !enabled {
                    return Err(CmdError::factory_disabled());
                }
                let socket = crate::tmux::socket_for(workspace);
                let client = crate::tmux::TmuxClient::new(socket, None);
                let res = super::login::open_login_pane(
                    &client,
                    workspace,
                    &a.cli,
                    &super::login::HostLoginProbe,
                )
                .map_err(|err| match err {
                    super::login::LoginError::UnknownCli(_)
                    | super::login::LoginError::NoLoginSubcommand { .. } => {
                        CmdError::invalid_input(err.to_string())
                    }
                    super::login::LoginError::NotInstalled(_) => CmdError {
                        exit_code: 3,
                        code: "cli_not_installed",
                        message: err.to_string(),
                    },
                    super::login::LoginError::Tmux(t) => CmdError {
                        exit_code: 1,
                        code: "tmux_error",
                        message: t.to_string(),
                    },
                })?;
                if a.json {
                    serialize_compact(&res)
                } else {
                    Ok(format!(
                        "Opened {} login pane {} (sign in there; tandem does not read credentials)",
                        res.cli, res.pane_id
                    ))
                }
            }
        },
        FactoryCommand::Agent(a_args) => {
            if !enabled {
                return Err(CmdError::factory_disabled());
            }
            handle_agent_command(&a_args.command, &store, workspace, caller, probe)
        }
    }
}

fn ensure_cli_risk_acknowledged(
    store: &FactoryStore,
    cli: &str,
    probe: &dyn super::login::LoginProbe,
) -> Result<(), CmdError> {
    let item = super::login::check_status(cli, probe);
    if !(item.installed && item.signed_in) {
        return Ok(());
    }
    let method = super::auth_risk::detected_method(&item);
    let method_label = super::auth_risk::method_name(method);
    match super::auth_risk::assess(
        cli,
        method,
        &super::dates::today_iso_date(),
        super::broker::terms::PROVIDER_TERMS,
        &super::auth_risk::read_acks(store),
    ) {
        super::auth_risk::AuthRisk::Risky { reason, .. } => Err(CmdError {
            exit_code: 3,
            code: "auth_risk_not_acknowledged",
            message: format!(
                "auth risk not acknowledged for {cli} ({method_label}): {reason}. Run: {}",
                super::auth_risk::acknowledge_command(cli, method_label)
            ),
        }),
        _ => Ok(()),
    }
}

fn map_team_error(err: super::team::TeamError) -> CmdError {
    match err {
        super::team::TeamError::InvalidInput(m) => CmdError::invalid_input(m),
        super::team::TeamError::Forbidden(m) => CmdError {
            exit_code: 4,
            code: "forbidden",
            message: m,
        },
        super::team::TeamError::NotFound(m) => CmdError::not_found(m),
        other => CmdError::invalid_input(other.to_string()),
    }
}

fn handle_agent_command(
    cmd: &FactoryAgentCommand,
    store: &FactoryStore,
    workspace: &Path,
    caller: &super::approval::Caller<'_>,
    probe: &dyn super::login::LoginProbe,
) -> Result<String, CmdError> {
    let socket = crate::tmux::socket_for(workspace);
    let tmux = crate::tmux::TmuxClient::new(socket, None);
    match cmd {
        FactoryAgentCommand::Assign(a) => {
            let caller_check = super::approval::require_user(caller);
            let is_agent = matches!(
                caller_check,
                Err(super::approval::ApprovalError::AgentCaller)
            );
            let source = match a.by.as_deref() {
                Some("user") => {
                    if is_agent {
                        return Err(CmdError {
                            exit_code: 4,
                            code: "agent_caller_forbidden",
                            message: "an AI agent cannot pass --by user".to_string(),
                        });
                    }
                    if let Err(err) = caller_check {
                        return Err(CmdError {
                            exit_code: err.exit_code(),
                            code: err.code(),
                            message: err.to_string(),
                        });
                    }
                    super::team::AssignmentSource::User
                }
                Some("orchestrator") => super::team::AssignmentSource::Orchestrator,
                Some(other) => {
                    return Err(CmdError::invalid_input(format!(
                        "invalid --by source '{other}' (expected user or orchestrator)"
                    )));
                }
                None => {
                    if is_agent {
                        super::team::AssignmentSource::Orchestrator
                    } else if let Err(err) = caller_check {
                        return Err(CmdError {
                            exit_code: err.exit_code(),
                            code: err.code(),
                            message: err.to_string(),
                        });
                    } else {
                        super::team::AssignmentSource::User
                    }
                }
            };
            let actor_id = if source == super::team::AssignmentSource::User {
                resolve_owner(None)?
            } else {
                "orchestrator".to_string()
            };
            let res = super::team::assign_stage(
                store,
                super::team::AssignStageInput {
                    run_id: &a.run,
                    stage: &a.stage,
                    cli: &a.cli,
                    model: &a.model,
                    role: a.role.as_deref(),
                    by: source,
                    actor_id: &actor_id,
                },
            )
            .map_err(map_team_error)?;
            if a.json {
                serialize_compact(&res)
            } else {
                Ok(format!(
                    "Assigned {} -> {}/{} ({})",
                    res.stage,
                    res.cli,
                    res.model,
                    res.source.as_str()
                ))
            }
        }
        FactoryAgentCommand::List(a) if a.all => {
            let list =
                super::team::list_all_agents(store, &tmux, a.active).map_err(map_team_error)?;
            if a.json {
                serialize_compact(&list)
            } else if list.is_empty() {
                Ok("No worker agents.".to_string())
            } else {
                Ok(list
                    .iter()
                    .map(|item| {
                        format!(
                            "{}\t{}\t{}/{}\t{}\t{}",
                            item.idea_id,
                            item.agent.job_id,
                            item.agent.cli,
                            item.agent.model,
                            item.agent.job_state,
                            item.agent.pane_id.as_deref().unwrap_or("-")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
        }
        FactoryAgentCommand::List(a) => {
            let run = a.run.as_deref().unwrap_or_default();
            let list = super::team::list_agents(store, &tmux, run).map_err(map_team_error)?;
            if a.json {
                serialize_compact(&list)
            } else if list.is_empty() {
                Ok("No worker jobs recorded for this run.".to_string())
            } else {
                Ok(list
                    .iter()
                    .map(|item| {
                        format!(
                            "{}\t{}/{}\t{}\t{}",
                            item.job_id,
                            item.cli,
                            item.model,
                            item.job_state,
                            item.pane_id.as_deref().unwrap_or("-")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
        }
        FactoryAgentCommand::Send(a) => {
            super::team::send_agent_message(store, &tmux, &a.run, &a.job, Path::new(&a.file))
                .map_err(map_team_error)?;
            if a.json {
                serialize_compact(&serde_json::json!({
                    "ok": true,
                    "run": a.run,
                    "job": a.job,
                }))
            } else {
                Ok(format!("Sent message to worker {}", a.job))
            }
        }
        FactoryAgentCommand::Capture(a) => {
            let output = super::team::capture_agent_output(store, &tmux, &a.run, &a.job, a.lines)
                .map_err(map_team_error)?;
            if a.json {
                serialize_compact(&serde_json::json!({
                    "run": a.run,
                    "job": a.job,
                    "output": output,
                }))
            } else {
                Ok(output)
            }
        }
        FactoryAgentCommand::Terminate(a) => {
            super::team::terminate_agent(store, &tmux, &a.run, &a.job, &a.confirm)
                .map_err(map_team_error)?;
            if a.json {
                serialize_compact(&serde_json::json!({
                    "ok": true,
                    "run": a.run,
                    "job": a.job,
                    "terminated": true,
                }))
            } else {
                Ok(format!("Terminated worker {}", a.job))
            }
        }
        FactoryAgentCommand::Relaunch(a) => {
            let target_cli =
                super::team::relaunch_target_cli(store, &a.run, &a.job).map_err(map_team_error)?;
            ensure_cli_risk_acknowledged(store, &target_cli, probe)?;
            let launched = super::team::relaunch_agent(store, &tmux, &a.run, &a.job)
                .map_err(map_team_error)?;
            if a.json {
                serialize_compact(&serde_json::json!({
                    "ok": true,
                    "run": a.run,
                    "job": a.job,
                    "pane_id": launched.pane_id,
                    "cli": launched.cli,
                    "model": launched.model,
                }))
            } else {
                Ok(format!(
                    "Relaunched worker {} in pane {} ({}/{})",
                    a.job, launched.pane_id, launched.cli, launched.model
                ))
            }
        }
    }
}

fn run_models_recommend(
    args: &FactoryModelsRecommendArgs,
    store: &FactoryStore,
) -> Result<String, CmdError> {
    let complexity = match args.complexity.as_deref() {
        None => None,
        Some(raw) => Some(super::routing::Complexity::parse(raw).ok_or_else(|| {
            CmdError::invalid_input(format!(
                "invalid complexity '{raw}' (expected low, medium, or high)"
            ))
        })?),
    };
    let required_tier = match args.required_tier.as_deref() {
        None => None,
        Some("economy") => Some(super::routing::ModelTier::Economy),
        Some("standard") => Some(super::routing::ModelTier::Standard),
        Some("premium") => Some(super::routing::ModelTier::Premium),
        Some(other) => {
            return Err(CmdError::invalid_input(format!(
                "invalid required tier '{other}' (expected economy, standard, or premium)"
            )));
        }
    };
    let probe = super::login::HostLoginProbe;
    let ready: Vec<&str> = super::login::LOGIN_SPECS
        .iter()
        .map(|spec| spec.cli)
        .filter(|cli| super::login::interactive_ready(cli, &probe).ready)
        .collect();
    if ready.is_empty() {
        return Err(CmdError {
            exit_code: 1,
            code: "no_cli_ready",
            message: "no CLI is installed and signed in (run: tandem factory login status)"
                .to_string(),
        });
    }
    let recommendation = super::team::recommend_stage(
        store,
        super::team::RecommendInput {
            run_id: &args.run,
            stage: &args.stage,
            complexity,
            required_tier,
            ready_clis: &ready,
        },
        &super::team::adapter_model_runner,
    )
    .map_err(map_team_error)?;
    if args.json {
        return serialize_compact(&recommendation);
    }
    let detail = recommendation
        .selection
        .as_ref()
        .map(|s| s.rationale.clone())
        .unwrap_or_else(|| "static tier (live catalog unavailable)".to_string());
    Ok(format!(
        "{}\t{}/{}\t{:?}\t{}",
        recommendation.stage, recommendation.cli, recommendation.model, recommendation.tier, detail
    ))
}

fn run_doctor_command(
    args: &FactoryDoctorArgs,
    workspace: &Path,
    caller: &super::approval::Caller<'_>,
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let store = FactoryStore::new(workspace);
    let result = if args.fix {
        let mut sink = io::sink();
        let prompt_out: &mut dyn Write = if args.json { &mut sink } else { stdout };
        super::recovery::doctor_fix(&store, workspace, caller, stdin, prompt_out)
    } else {
        super::recovery::diagnose(&store, workspace)
    };

    match result {
        Ok(report) => {
            let output = if args.json {
                match serialize_compact(&report) {
                    Ok(s) => s,
                    Err(err) => {
                        let _ = writeln!(stderr, "{}", err.message);
                        let _ = stderr.flush();
                        return err.exit_code;
                    }
                }
            } else {
                super::recovery::format_doctor_report(&report)
            };
            if !output.is_empty() {
                let _ = writeln!(stdout, "{output}");
            }
            let _ = stdout.flush();
            0
        }
        Err(err) => {
            let (exit_code, code, message) = match &err {
                super::recovery::RecoveryError::Approval(e) => {
                    (e.exit_code(), e.code(), e.to_string())
                }
                _ => (1, "recovery_error", err.to_string()),
            };
            let clean_msg = sanitize::clean(&message).replace('\n', " ");
            let _ = writeln!(stderr, "{clean_msg}");
            let _ = stderr.flush();
            if args.json {
                let envelope = ErrorEnvelope {
                    error: ErrorBody {
                        code,
                        message: &clean_msg,
                    },
                };
                if let Ok(line) = serde_json::to_string(&envelope) {
                    let _ = writeln!(stdout, "{line}");
                    let _ = stdout.flush();
                }
            }
            exit_code
        }
    }
}

fn run_audit_command(
    args: &FactoryAuditArgs,
    store: &FactoryStore,
    enabled: bool,
) -> Result<String, CmdError> {
    if !enabled {
        return Err(CmdError::factory_disabled());
    }
    match &args.command {
        Some(FactoryAuditCommand::Verify(v)) => {
            let report = super::audit::verify(&store.events_path()).map_err(map_factory_error)?;
            if v.json {
                serialize_compact(&report)
            } else {
                Ok(super::audit::format_verify_summary(&report))
            }
        }
        None => {
            let Some(ref idea_id) = args.idea else {
                return Err(CmdError::invalid_input(
                    "missing IDEA id (or try `tandem factory audit verify`)",
                ));
            };
            let kind = match &args.kind {
                Some(raw) => Some(super::audit::AuditKind::parse(raw).ok_or_else(|| {
                    CmdError::invalid_input(format!("invalid audit kind: {raw}"))
                })?),
                None => None,
            };
            let filter = super::audit::AuditFilter {
                run: args.run.clone(),
                kind,
                actor: args.actor.clone(),
            };
            let entries =
                super::audit::entries(store, idea_id, &filter).map_err(map_factory_error)?;
            let verify_report =
                super::audit::verify(&store.events_path()).map_err(map_factory_error)?;

            if args.json {
                return super::audit::export_json(&entries, Some(&verify_report))
                    .map_err(map_factory_error);
            }
            if args.markdown {
                return Ok(super::audit::export_markdown(
                    idea_id,
                    &entries,
                    Some(&verify_report),
                ));
            }

            let mut out = format!(
                "Audit · {idea_id}\n{}\n\n",
                super::audit::format_verify_summary(&verify_report)
            );
            if entries.is_empty() {
                out.push_str("No audit entries match the current filter.");
                return Ok(out);
            }
            for e in &entries {
                out.push_str(&format!(
                    "{}  {:<12}  {}\n",
                    super::audit::format_time_short(e.ts),
                    e.actor,
                    e.summary
                ));
                for d in &e.details {
                    out.push_str(&format!("  {:<10} {}\n", d.label, d.value));
                }
            }
            Ok(out.trim_end().to_string())
        }
    }
}

fn run_redact_test(args: &FactoryRedactTestArgs) -> Result<String, CmdError> {
    let content = fs::read_to_string(&args.file).map_err(|e| {
        CmdError::invalid_input(format!("cannot read {}: {e}", args.file.display()))
    })?;
    let ctx = super::redact::RedactContext::from_env();
    let (redacted, matches) = super::redact::inspect(&content, &ctx);

    if args.json {
        return serialize_compact(&serde_json::json!({
            "file": args.file.display().to_string(),
            "replacements": redacted.replacements(),
            "matches": matches,
        }));
    }

    if matches.is_empty() {
        return Ok("No secrets detected (0 replacements).".to_string());
    }
    let mut lines: Vec<String> = matches
        .iter()
        .map(|m| format!("line {}: {}", m.line, m.kind.as_str()))
        .collect();
    lines.push(format!("Total replacements: {}", redacted.replacements()));
    Ok(lines.join("\n"))
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
    start_idea_with_text(workspace, config, cli, model, title, "")
}

pub fn start_idea_with_text(
    workspace: &Path,
    config: &Config,
    cli: &str,
    model: &str,
    title: Option<&str>,
    text: &str,
) -> Result<Idea, String> {
    start_idea_cmd(workspace, config, cli, model, title, text).map_err(|e| e.message)
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
    text: &str,
) -> Result<Idea, CmdError> {
    if !crate::state::CLIS.contains(&cli) {
        return Err(CmdError::invalid_input(format!(
            "unsupported orchestrator cli: {cli} (expected claude, agy, or opencode)"
        )));
    }
    let validated_model = crate::adapters::base::validate_model_for_cli(cli, model)
        .map_err(|e| CmdError::invalid_input(e.to_string()))?;
    let raw_title = title.unwrap_or("New idea");
    let validated_title = validate_title(raw_title).map_err(|e| CmdError::invalid_input(e.0))?;
    let validated_text = validate_text(text).map_err(|e| CmdError::invalid_input(e.0))?;
    let owner = resolve_owner(None)?;
    let actor = resolve_actor(config);
    let store = FactoryStore::new(workspace);
    let idea = store
        .create_idea(&validated_title, &validated_text, &owner, &actor)
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

fn format_ready_text(res: &super::approval::ReadyResult) -> String {
    let mut lines = vec![format!("{} ({})", res.message, res.idea.id)];
    if let Some(budget) = &res.budget {
        let cost = res.cost_cap_note.clone().unwrap_or_else(|| {
            budget
                .max_cost_usd
                .map(|c| format!("${c:.2}"))
                .unwrap_or_else(|| "unlimited".to_string())
        });
        let dur = budget
            .max_duration_secs
            .map(super::budget::format_duration_minutes_or_secs)
            .unwrap_or_else(|| "unlimited".to_string());
        let retries = budget
            .max_retries
            .map(|r| format!("{r} retries"))
            .unwrap_or_else(|| "unlimited retries".to_string());
        lines.push(format!("Budget: {cost} · {dur} · {retries}"));
    }
    if let Some(est) = &res.estimate {
        let dur = match &est.duration_secs {
            super::budget::EstimateRange::Known { min, max, .. } => {
                format!("{min} to {max}s")
            }
            super::budget::EstimateRange::Unknown { reason } => {
                format!("duration unknown ({reason})")
            }
        };
        let cost = match &est.cost_usd {
            super::budget::EstimateRange::Known { min, max, .. } => {
                format!("${min:.2} to ${max:.2}")
            }
            super::budget::EstimateRange::Unknown { reason } => {
                format!("cost unknown ({reason})")
            }
        };
        lines.push(format!("Estimate: {dur} · {cost}"));
    }
    if let Some(run) = &res.run {
        lines.push(format!("Run: {} ({})", run.id, run.workflow));
    } else if let Some(err) = &res.run_error {
        lines.push(format!("Run error: {err}"));
    }
    lines.join("\n")
}

fn format_budget_change_text(res: &super::runner::ChangeBudgetResult) -> String {
    let mut lines = Vec::new();
    if res.old.max_cost_usd != res.new.max_cost_usd {
        let old_s = res
            .old
            .max_cost_usd
            .map(|c| format!("${c:.2}"))
            .unwrap_or_else(|| "unlimited".to_string());
        let new_s = res
            .new
            .max_cost_usd
            .map(|c| format!("${c:.2}"))
            .unwrap_or_else(|| "unlimited".to_string());
        lines.push(format!("Cost cap for {}: {old_s} -> {new_s}.", res.run.id));
    }
    if res.old.max_duration_secs != res.new.max_duration_secs {
        let old_s = res
            .old
            .max_duration_secs
            .map(|d| d.to_string())
            .unwrap_or_else(|| "unlimited".to_string());
        let new_s = res
            .new
            .max_duration_secs
            .map(|d| d.to_string())
            .unwrap_or_else(|| "unlimited".to_string());
        lines.push(format!(
            "Duration cap for {}: {old_s} -> {new_s} seconds.",
            res.run.id
        ));
    }
    if res.old.max_retries != res.new.max_retries {
        let old_s = res
            .old
            .max_retries
            .map(|r| r.to_string())
            .unwrap_or_else(|| "unlimited".to_string());
        let new_s = res
            .new
            .max_retries
            .map(|r| r.to_string())
            .unwrap_or_else(|| "unlimited".to_string());
        lines.push(format!("Retry cap for {}: {old_s} -> {new_s}.", res.run.id));
    }
    if res.run.state == super::model::RunState::Paused {
        lines.push(format!(
            "Resume with: tandem factory run resume {}",
            res.run.id
        ));
    }
    lines.join(" ")
}

fn format_run_usage_text(report: &super::telemetry::RunUsageReport) -> String {
    let mut lines = vec![
        format!("Run: {}", report.run),
        format!("Total duration: {:.1}s", report.total_duration_secs),
        format!("Total cost: {}", report.total_cost_display),
    ];
    if let (Some(i), Some(o)) = (report.total_input_tokens, report.total_output_tokens) {
        lines.push(format!("Tokens: {i} in / {o} out"));
    }
    if !report.stages.is_empty() {
        lines.push("Stages:".to_string());
        for st in &report.stages {
            let model = st.model.as_deref().unwrap_or("-");
            let tier = st.tier.as_deref().unwrap_or("-");
            lines.push(format!(
                "  {}  {} job(s)  {}/{}  {:.1}s  {}",
                st.stage, st.jobs, model, tier, st.duration_secs, st.cost_display
            ));
        }
    }
    lines.push(format!(
        "Rework: {} retries ({} repeated rationales), develop reopened {} by test / {} by review, {} review rounds, {} model upgrades, {} interrupted",
        report.rework.total_retries,
        report.rework.repeated_rationales,
        report.rework.develop_reopened_by_test,
        report.rework.develop_reopened_by_review,
        report.rework.review_rounds,
        report.rework.model_upgrades,
        report.rework.interrupted_jobs,
    ));
    lines.join("\n")
}

fn format_stopped_jobs_line(jobs: &[super::runner::StoppedJobInfo]) -> Option<String> {
    if jobs.is_empty() {
        return None;
    }
    let details: Vec<String> = jobs
        .iter()
        .map(|j| format!("{}, attempt {}", j.stage, j.attempt))
        .collect();
    let noun = if jobs.len() == 1 { "job" } else { "jobs" };
    Some(format!(
        "Stopped {} running {noun} ({}).",
        jobs.len(),
        details.join("; ")
    ))
}

fn format_pause_run_text(res: &super::runner::PauseRunResult, now: bool) -> String {
    let mut lines = Vec::new();
    if let Some(stopped) = format_stopped_jobs_line(&res.stopped_jobs) {
        lines.push(stopped);
    }
    if now {
        lines.push(format!("Run {} paused (--now).", res.run.id));
    } else {
        lines.push(format!(
            "Run {} paused. Running jobs will finish; no new stage will start.",
            res.run.id
        ));
    }
    lines.push(format!(
        "Resume with: tandem factory run resume {}",
        res.run.id
    ));
    lines.join("\n")
}

fn format_cancel_run_text(res: &super::runner::CancelRunResult) -> String {
    let mut lines = Vec::new();
    if let Some(stopped) = format_stopped_jobs_line(&res.stopped_jobs) {
        lines.push(stopped);
    }
    lines.push(format!(
        "Run {} cancelled. Idea {} is Ready for development again.",
        res.run.id, res.idea.id
    ));
    if let (Some(wt), Some(branch)) = (&res.run.worktree_path, &res.run.branch) {
        lines.push(format!("The work is kept: {wt} on branch {branch}."));
        lines.push(format!(
            "Remove it with: tandem factory run cleanup {} --confirm {}",
            res.run.id, res.run.id
        ));
    }
    lines.join("\n")
}

fn format_cancel_idea_text(res: &super::runner::CancelIdeaResult) -> String {
    let mut lines = Vec::new();
    if let Some(stopped) = format_stopped_jobs_line(&res.stopped_jobs) {
        lines.push(stopped);
    }
    lines.push(format!("Idea {} cancelled.", res.idea.id));
    for run in &res.cancelled_runs {
        if let (Some(wt), Some(branch)) = (&run.worktree_path, &run.branch) {
            lines.push(format!(
                "The work for {} is kept: {wt} on branch {branch}.",
                run.id
            ));
            lines.push(format!(
                "Remove it with: tandem factory run cleanup {} --confirm {}",
                run.id, run.id
            ));
        }
    }
    lines.join("\n")
}

const MAX_RUN_LOG_TAIL_BYTES: usize = 262_144;

fn is_valid_correlation_id_for_run(run_id: &str, job_id: &str) -> bool {
    let Some(rest) = job_id
        .strip_prefix(run_id)
        .and_then(|s| s.strip_prefix('.'))
    else {
        return false;
    };
    let Some((stage, attempt)) = rest.split_once('.') else {
        return false;
    };
    let mut stage_bytes = stage.bytes();
    let stage_ok = matches!(stage_bytes.next(), Some(b) if b.is_ascii_lowercase())
        && stage.len() <= 48
        && stage_bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    let attempt_ok =
        (1..=2).contains(&attempt.len()) && attempt.bytes().all(|b| b.is_ascii_digit());
    stage_ok && attempt_ok
}

fn read_run_job_log(store: &FactoryStore, args: &FactoryRunLogArgs) -> Result<String, CmdError> {
    use std::io::{Seek, SeekFrom};

    if !super::ids::is_valid_run_id(&args.run) {
        return Err(CmdError::invalid_input(format!(
            "invalid run id {}",
            args.run
        )));
    }
    if args.tail == 0 || args.tail > MAX_RUN_LOG_TAIL_BYTES {
        return Err(CmdError::invalid_input(format!(
            "tail bytes must be between 1 and {MAX_RUN_LOG_TAIL_BYTES}"
        )));
    }
    if !is_valid_correlation_id_for_run(&args.run, &args.job) {
        return Err(CmdError::invalid_input(format!(
            "invalid job id {}",
            args.job
        )));
    }

    super::run::get_run(store, &args.run)
        .map_err(|e| CmdError {
            exit_code: 1,
            code: e.code(),
            message: e.to_string(),
        })?
        .ok_or_else(|| CmdError::not_found(format!("run {}", args.run)))?;

    let job_dir = store
        .root
        .join("runs")
        .join(&args.run)
        .join("jobs")
        .join(&args.job);
    super::events::reject_if_symlink(&job_dir)
        .map_err(|e| CmdError::invalid_input(e.to_string()))?;
    if !job_dir.exists() {
        return Err(CmdError::not_found(format!("job {}", args.job)));
    }

    let log_path = job_dir.join("log.txt");
    super::events::reject_if_symlink(&log_path)
        .map_err(|e| CmdError::invalid_input(e.to_string()))?;

    let cleaned = if !log_path.exists() {
        String::new()
    } else {
        let mut file = File::open(&log_path)
            .map_err(|e| CmdError::invalid_input(format!("cannot open log: {e}")))?;
        let len = file
            .metadata()
            .map_err(|e| CmdError::invalid_input(format!("cannot stat log: {e}")))?
            .len();
        let start = len.saturating_sub(args.tail as u64);
        if start > 0 {
            file.seek(SeekFrom::Start(start))
                .map_err(|e| CmdError::invalid_input(format!("cannot seek log: {e}")))?;
        }
        let mut buf = Vec::with_capacity(args.tail.min(len as usize));
        file.take(args.tail as u64)
            .read_to_end(&mut buf)
            .map_err(|e| CmdError::invalid_input(format!("cannot read log: {e}")))?;

        let raw = String::from_utf8_lossy(&buf);
        let aligned = if start > 0 {
            raw.trim_start_matches('\u{FFFD}')
        } else {
            raw.as_ref()
        };
        let sanitized = sanitize::clean(aligned);
        super::redact::redact(&sanitized, &super::redact::RedactContext::from_env()).into_string()
    };

    if args.json {
        serialize_compact(&serde_json::json!({
            "run": args.run,
            "job": args.job,
            "log": cleaned
        }))
    } else {
        Ok(cleaned)
    }
}

fn load_run_workflow(
    store: &FactoryStore,
    workspace: &Path,
    run_id: &str,
) -> Result<super::workflow::schema::Workflow, CmdError> {
    let run_wf_path = store.root.join("runs").join(run_id).join("workflow.yaml");
    let raw = if run_wf_path.exists() {
        super::workflow::parse::load_workflow_file(&run_wf_path)
            .map_err(|e| CmdError::invalid_input(e.to_string()))?
    } else {
        super::workflow::load_default_workflow(workspace)
            .map_err(|e| CmdError::invalid_input(e.to_string()))?
            .0
    };
    let settings = if store.exists() {
        store.settings().map_err(map_factory_error)?
    } else {
        FactorySettings::default()
    };
    super::workflow::validate::validate(&raw, &settings)
        .map_err(|diags| CmdError::invalid_input(format!("workflow validation failed: {diags:?}")))
}

fn run_evidence_cmd(
    store: &FactoryStore,
    workspace: &Path,
    args: &FactoryRunEvidenceArgs,
) -> Result<String, CmdError> {
    if !super::ids::is_valid_run_id(&args.run) {
        return Err(CmdError::invalid_input(format!(
            "invalid run id {}",
            args.run
        )));
    }
    let run = super::run::get_run(store, &args.run)
        .map_err(|e| CmdError {
            exit_code: 1,
            code: e.code(),
            message: e.to_string(),
        })?
        .ok_or_else(|| CmdError::not_found(format!("run {}", args.run)))?;

    if let Some(ref item_id) = args.show {
        let items = super::evidence::list(store, &args.run).map_err(map_factory_error)?;
        let item = items
            .into_iter()
            .find(|i| i.id == *item_id)
            .ok_or_else(|| CmdError::not_found(format!("evidence item {item_id}")))?;
        let raw =
            super::evidence::read_item(store, &args.run, item_id).map_err(map_factory_error)?;
        let cleaned = sanitize::clean(&raw);
        if args.json {
            return serialize_compact(&serde_json::json!({
                "run": args.run,
                "item": item,
                "content": cleaned,
            }));
        }
        return Ok(cleaned);
    }

    let workflow = load_run_workflow(store, workspace, &args.run)?;
    let items = if !run.evidence.is_empty() {
        run.evidence.clone()
    } else {
        super::evidence::list(store, &args.run).map_err(map_factory_error)?
    };
    let missing = super::evidence::missing(&run, &workflow);
    let summary = super::evidence::bundle_summary(&items);
    let complete = missing.is_empty();
    let review_rounds = super::evidence::load_review_rounds(store, &args.run);

    if args.json {
        return serialize_compact(&serde_json::json!({
            "run": args.run,
            "items": items,
            "missing": missing,
            "complete": complete,
            "summary": summary,
            "review_rounds": review_rounds.rounds,
            "prs": run.prs,
        }));
    }

    let mut lines = Vec::new();
    lines.push(format!("Evidence ({}) — {summary}", args.run));
    for item in &items {
        let hash_short: String = item.sha256.chars().take(12).collect();
        lines.push(format!(
            "  {}  {:<16}  {:<12}  a{}  {}  {}",
            item.id,
            item.kind.as_str(),
            item.stage,
            item.attempt,
            hash_short,
            item.summary
        ));
    }
    if !missing.is_empty() {
        let missing_names: Vec<&str> = missing.iter().map(|k| k.as_str()).collect();
        lines.push(format!(
            "Missing required evidence: {}",
            missing_names.join(", ")
        ));
    }
    Ok(lines.join("\n"))
}

fn map_checks_error(err: super::checks::ChecksError) -> CmdError {
    CmdError {
        exit_code: err.exit_code(),
        code: err.code(),
        message: err.to_string(),
    }
}

fn resolve_checks_target(
    workspace: &Path,
    explicit: Option<&str>,
) -> Result<(String, std::path::PathBuf), CmdError> {
    if let Some(name) = explicit {
        super::checks::validate_target_name(name).map_err(map_checks_error)?;
        let path = super::workspace::target_path_for_name(workspace, name)
            .map_err(|e| CmdError::invalid_input(e.to_string()))?;
        return Ok((name.to_string(), path));
    }
    match super::workspace::resolve_target(workspace) {
        super::workspace::TargetResolution::Resolved { name, path } => Ok((name, path)),
        super::workspace::TargetResolution::Ambiguous { choices } => {
            Err(CmdError::invalid_input(format!(
                "multiple targets found ({}); pass --target <name>",
                choices.join(", ")
            )))
        }
    }
}

fn format_discover_text(report: &super::checks::DiscoverReport) -> String {
    let mut lines = vec![format!("target {}", report.target)];
    for p in &report.proposals {
        let kind = match p.kind {
            super::checks::CheckKind::Test => "test",
            super::checks::CheckKind::Lint => "lint",
            super::checks::CheckKind::Deploy => "deploy",
        };
        let note = p
            .note
            .as_deref()
            .map(|n| format!(", {n}"))
            .unwrap_or_default();
        lines.push(format!(
            "  {kind:<6} {}  ({}: line {}{note})",
            p.argv.join(" "),
            p.source_file,
            p.source_line
        ));
    }
    if report.proposals.is_empty() {
        lines.push("  (no check commands discovered)".to_string());
    }
    if let Some(ref cfg) = report.confirmed {
        let by = cfg.confirmed_by.as_deref().unwrap_or("unknown");
        lines.push(format!(
            "Confirmed by {by} (timeout {}s).",
            cfg.timeout_secs
        ));
    } else {
        lines.push(format!(
            "Nothing is confirmed yet. Confirm with: {}",
            report.confirm_command
        ));
    }
    lines.join("\n")
}

fn format_show_checks_text(checks: &BTreeMap<String, super::checks::ChecksConfig>) -> String {
    if checks.is_empty() {
        return "No check commands confirmed yet.".to_string();
    }
    let mut lines = Vec::new();
    for (target, cfg) in checks {
        lines.push(format!(
            "target {target} (timeout {}s, confirmed_by={})",
            cfg.timeout_secs,
            cfg.confirmed_by.as_deref().unwrap_or("none")
        ));
        for cmd in &cfg.test {
            lines.push(format!("  test   {}", cmd.join(" ")));
        }
        for cmd in &cfg.lint {
            lines.push(format!("  lint   {}", cmd.join(" ")));
        }
    }
    lines.join("\n")
}

fn handle_permissions_command(
    cmd: &FactoryPermissionsCommand,
    store: &FactoryStore,
    caller: &super::approval::Caller<'_>,
) -> Result<String, CmdError> {
    let map_approval_err = |e: super::approval::ApprovalError| CmdError {
        exit_code: e.exit_code(),
        code: e.code(),
        message: e.to_string(),
    };
    match cmd {
        FactoryPermissionsCommand::Show(a) => {
            let settings = if store.exists() {
                store.settings().map_err(map_factory_error)?
            } else {
                FactorySettings::default()
            };
            let view = super::permissions::show(&settings);
            if a.json {
                serialize_compact(&view)
            } else {
                serde_json::to_string_pretty(&view)
                    .map_err(|e| CmdError::invalid_input(e.to_string()))
            }
        }
        FactoryPermissionsCommand::Allow(a) => {
            if a.agent && a.target {
                return Err(CmdError::invalid_input(
                    "pass at most one of --agent or --target".to_string(),
                ));
            }
            let tool = super::workflow::schema::WorkflowTool::parse(&a.tool)
                .ok_or_else(|| CmdError::invalid_input(format!("unknown tool: {}", a.tool)))?;
            let actor = resolve_owner(None)?;
            let res = if a.target {
                super::permissions::allow_target_tool(store, &a.subject, tool, &actor, caller)
                    .map_err(map_approval_err)?
            } else {
                let agent =
                    super::workflow::schema::WorkflowAgent::parse(&a.subject).ok_or_else(|| {
                        CmdError::invalid_input(format!("unknown agent: {}", a.subject))
                    })?;
                super::permissions::allow_agent_tool(store, agent, tool, &actor, caller)
                    .map_err(map_approval_err)?
            };
            if a.json {
                serialize_compact(&res)
            } else {
                Ok(format!(
                    "allowed {} for {} {}",
                    res.tool, res.scope, res.subject
                ))
            }
        }
        FactoryPermissionsCommand::Deny(a) => {
            if a.agent && a.target {
                return Err(CmdError::invalid_input(
                    "pass at most one of --agent or --target".to_string(),
                ));
            }
            let tool = super::workflow::schema::WorkflowTool::parse(&a.tool)
                .ok_or_else(|| CmdError::invalid_input(format!("unknown tool: {}", a.tool)))?;
            let actor = resolve_owner(None)?;
            let res = if a.agent {
                let agent =
                    super::workflow::schema::WorkflowAgent::parse(&a.subject).ok_or_else(|| {
                        CmdError::invalid_input(format!("unknown agent: {}", a.subject))
                    })?;
                super::permissions::deny_agent_tool(store, agent, tool, &actor, caller)
                    .map_err(map_approval_err)?
            } else {
                super::permissions::deny_target_tool(store, &a.subject, tool, &actor, caller)
                    .map_err(map_approval_err)?
            };
            if a.json {
                serialize_compact(&res)
            } else {
                Ok(format!(
                    "denied {} for {} {}",
                    res.tool, res.scope, res.subject
                ))
            }
        }
    }
}

fn handle_checks_command(
    cmd: &FactoryChecksCommand,
    store: &FactoryStore,
    workspace: &Path,
    caller: &super::approval::Caller<'_>,
) -> Result<String, CmdError> {
    match cmd {
        FactoryChecksCommand::Discover(a) => {
            let (target_name, target_path) = resolve_checks_target(workspace, a.target.as_deref())?;
            let settings = if store.exists() {
                store.settings().map_err(map_factory_error)?
            } else {
                FactorySettings::default()
            };
            let confirmed = settings.checks.get(&target_name).cloned();
            let report = super::checks::discover_checks(&target_name, &target_path, confirmed)
                .map_err(map_checks_error)?;
            if a.json {
                serialize_compact(&report)
            } else {
                Ok(format_discover_text(&report))
            }
        }
        FactoryChecksCommand::Set(a) => {
            let confirmed_by = resolve_owner(None)?;
            let has_checks = a.test.is_some() || a.lint.is_some() || a.ui_paths.is_some();
            let deploy_cmd: Option<Vec<String>> = match a.deploy.as_deref() {
                Some(raw) => Some(serde_json::from_str(raw).map_err(|e| {
                    CmdError::invalid_input(format!("invalid --deploy JSON argv list: {e}"))
                })?),
                None => None,
            };

            let mut deploy_cfg = None;
            if let Some(cmd) = deploy_cmd {
                let dcfg = super::checks::set_deploy_command_with_timeout(
                    store,
                    &a.target,
                    cmd,
                    a.timeout_secs,
                    &a.confirm,
                    &confirmed_by,
                    caller,
                )
                .map_err(map_checks_error)?;
                deploy_cfg = Some(dcfg);
            }

            if has_checks || deploy_cfg.is_none() {
                let test_cmds: Vec<Vec<String>> = match a.test.as_deref() {
                    Some(raw) => serde_json::from_str(raw).map_err(|e| {
                        CmdError::invalid_input(format!("invalid --test JSON argv list: {e}"))
                    })?,
                    None => Vec::new(),
                };
                let lint_cmds: Vec<Vec<String>> = match a.lint.as_deref() {
                    Some(raw) => serde_json::from_str(raw).map_err(|e| {
                        CmdError::invalid_input(format!("invalid --lint JSON argv list: {e}"))
                    })?,
                    None => Vec::new(),
                };
                let ui_paths: Vec<String> = match a.ui_paths.as_deref() {
                    Some(raw) => serde_json::from_str(raw).map_err(|e| {
                        CmdError::invalid_input(format!("invalid --ui-paths JSON list: {e}"))
                    })?,
                    None => Vec::new(),
                };
                let cfg = super::checks::set_checks(
                    store,
                    &a.target,
                    test_cmds,
                    lint_cmds,
                    a.timeout_secs,
                    ui_paths,
                    &a.confirm,
                    &confirmed_by,
                    caller,
                )
                .map_err(map_checks_error)?;
                if a.json {
                    serialize_compact(&cfg)
                } else {
                    let mut map = BTreeMap::new();
                    map.insert(a.target.clone(), cfg);
                    Ok(format_show_checks_text(&map))
                }
            } else if a.json {
                serialize_compact(&deploy_cfg)
            } else {
                let cmd_str = deploy_cfg
                    .as_ref()
                    .map(|d| d.command.join(" "))
                    .unwrap_or_default();
                Ok(format!(
                    "target {}: deploy command confirmed: {}",
                    a.target, cmd_str
                ))
            }
        }
        FactoryChecksCommand::Show(a) => {
            let map =
                super::checks::show_checks(store, a.target.as_deref()).map_err(map_checks_error)?;
            if a.json {
                serialize_compact(&map)
            } else {
                Ok(format_show_checks_text(&map))
            }
        }
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
