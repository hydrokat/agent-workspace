use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkflowTrigger {
    ReadyForDevelopment,
}

impl WorkflowTrigger {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ready-for-development" => Some(Self::ReadyForDevelopment),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkflowAgent {
    Orchestrator,
    TestEngineer,
    Implementer,
    Verifier,
    Reviewer,
    DemoMaker,
    ReleaseEngineer,
}

impl WorkflowAgent {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "orchestrator" => Some(Self::Orchestrator),
            "test-engineer" => Some(Self::TestEngineer),
            "implementer" => Some(Self::Implementer),
            "verifier" => Some(Self::Verifier),
            "reviewer" => Some(Self::Reviewer),
            "demo-maker" => Some(Self::DemoMaker),
            "release-engineer" => Some(Self::ReleaseEngineer),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Orchestrator => "orchestrator",
            Self::TestEngineer => "test-engineer",
            Self::Implementer => "implementer",
            Self::Verifier => "verifier",
            Self::Reviewer => "reviewer",
            Self::DemoMaker => "demo-maker",
            Self::ReleaseEngineer => "release-engineer",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolClass {
    Read,
    Write,
    Execute,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTool {
    RepoRead,
    IssueContext,
    DiffReview,
    RepoWrite,
    Shell,
    TestRunner,
    LintRunner,
    Browser,
    Screenshot,
    Ci,
    Deployment,
}

impl WorkflowTool {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "repo_read" => Some(Self::RepoRead),
            "issue_context" => Some(Self::IssueContext),
            "diff_review" => Some(Self::DiffReview),
            "repo_write" => Some(Self::RepoWrite),
            "shell" => Some(Self::Shell),
            "test_runner" => Some(Self::TestRunner),
            "lint_runner" => Some(Self::LintRunner),
            "browser" => Some(Self::Browser),
            "screenshot" => Some(Self::Screenshot),
            "ci" => Some(Self::Ci),
            "deployment" => Some(Self::Deployment),
            _ => None,
        }
    }

    pub fn class(&self) -> ToolClass {
        match self {
            Self::RepoRead | Self::IssueContext | Self::DiffReview => ToolClass::Read,
            Self::RepoWrite => ToolClass::Write,
            Self::Shell | Self::TestRunner | Self::LintRunner => ToolClass::Execute,
            Self::Browser | Self::Screenshot | Self::Ci | Self::Deployment => ToolClass::External,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageCondition {
    UiChanged,
    DeployRequested,
}

impl StageCondition {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ui_changed" => Some(Self::UiChanged),
            "deploy_requested" => Some(Self::DeployRequested),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageRequirement {
    HumanApproval,
}

impl StageRequirement {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "human_approval" => Some(Self::HumanApproval),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtectedAction {
    ProductionDeploy,
}

impl ProtectedAction {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "production_deploy" => Some(Self::ProductionDeploy),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    TestReport,
    LintReport,
    ReviewFindings,
    Screenshots,
    Handoff,
}

impl EvidenceKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "test_report" => Some(Self::TestReport),
            "lint_report" => Some(Self::LintReport),
            "review_findings" => Some(Self::ReviewFindings),
            "screenshots" => Some(Self::Screenshots),
            "handoff" => Some(Self::Handoff),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StageBudget {
    pub max_retries: Option<u32>,
    pub max_duration_secs: Option<u64>,
    pub max_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    pub max_parallel_agents: u32,
    pub default_temperature: f64,
    pub require_human_approval: Vec<ProtectedAction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageDef {
    pub name: String,
    pub agent: WorkflowAgent,
    pub depends_on: Vec<String>,
    pub tools: Vec<WorkflowTool>,
    pub when: Option<StageCondition>,
    pub requires: Vec<StageRequirement>,
    pub budget: StageBudget,
    pub evidence: Vec<EvidenceKind>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workflow {
    pub name: String,
    pub version: u32,
    pub trigger: WorkflowTrigger,
    pub policy: Policy,
    pub stages: Vec<StageDef>,
    pub sha256: String,
    pub raw_text: String,
}
