use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const KNOWN_PROVIDERS: &[&str] = &["claude", "agy", "opencode"];
pub const TITLE_MAX_LEN: usize = 120;
pub const TEXT_MAX_LEN: usize = 8000;
pub const OWNER_MAX_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ValidationError(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdeaState {
    Inbox,
    Ideation,
    Planning,
    ReadyForDevelopment,
    Queued,
    InDevelopment,
    Review,
    Delivered,
    Cancelled,
}

impl IdeaState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Inbox => "Inbox",
            Self::Ideation => "Ideation",
            Self::Planning => "Planning",
            Self::ReadyForDevelopment => "Ready for development",
            Self::Queued => "Queued",
            Self::InDevelopment => "In development",
            Self::Review => "Review",
            Self::Delivered => "Delivered",
            Self::Cancelled => "Cancelled",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Delivered | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    User,
    Orchestrator,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub kind: ActorKind,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrchestratorRef {
    pub cli: String,
    pub model: String,
    #[serde(default)]
    pub conversation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub kind: String,
    pub version: u32,
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub created: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FeasibilityVerdict {
    Feasible,
    FeasibleWithRisks,
    Infeasible,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub artifact: ArtifactRef,
    pub workflow: String,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub missing_sections: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalGate {
    Feasibility,
    ReadyForDevelopment,
    ProtectedAction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Approval {
    pub gate: ApprovalGate,
    pub actor: Actor,
    pub via: String,
    pub at: f64,
    #[serde(default)]
    pub subject: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    #[serde(default)]
    pub max_cost_usd: Option<f64>,
    #[serde(default)]
    pub max_duration_secs: Option<u64>,
    #[serde(default)]
    pub max_retries: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Skipped,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    pub name: String,
    pub agent: String,
    pub state: StageState,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub interrupted_attempts: u32,
    #[serde(default)]
    pub started: Option<f64>,
    #[serde(default)]
    pub finished: Option<f64>,
    #[serde(default)]
    pub artifacts: Vec<ArtifactRef>,
    #[serde(default)]
    pub rationale: Option<String>,
    #[serde(default)]
    pub retry_hypothesis: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Pending,
    Queued,
    Running,
    Paused,
    Review,
    Succeeded,
    Delivered,
    Failed,
    Cancelled,
    Withdrawn,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub idea: String,
    pub workflow: String,
    pub workflow_version: String,
    pub workflow_sha256: String,
    #[serde(default)]
    pub workflow_source: String,
    #[serde(default)]
    pub plan_sha256: String,
    #[serde(default)]
    pub deploy_requested: bool,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub base_commit: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub pause_reason: Option<String>,
    #[serde(default)]
    pub action_commands: Vec<String>,
    #[serde(default)]
    pub baseline_passed: Option<bool>,
    pub state: RunState,
    #[serde(default)]
    pub stages: Vec<Stage>,
    #[serde(default)]
    pub budget: Option<Budget>,
    pub created: f64,
    pub updated: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrLink {
    pub repo: String,
    pub number: u64,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Idea {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub text: String,
    pub state: IdeaState,
    pub owner: String,
    pub created: f64,
    pub updated: f64,
    #[serde(default)]
    pub risk: Option<Risk>,
    #[serde(default)]
    pub orchestrator: Option<OrchestratorRef>,
    #[serde(default)]
    pub feasibility: Option<ArtifactRef>,
    #[serde(default)]
    pub feasibility_verdict: Option<FeasibilityVerdict>,
    #[serde(default)]
    pub plan: Option<Plan>,
    #[serde(default)]
    pub plan_snapshot: Option<ArtifactRef>,
    #[serde(default)]
    pub artifacts: Vec<ArtifactRef>,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub runs: Vec<String>,
    #[serde(default)]
    pub prs: Vec<PrLink>,
    #[serde(default)]
    pub cancelled_from: Option<IdeaState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderGateState {
    Closed,
    Open,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderGate {
    pub provider: String,
    pub state: ProviderGateState,
    #[serde(default)]
    pub auth_type: Option<String>,
    #[serde(default)]
    pub verified_on: Option<String>,
    #[serde(default)]
    pub terms_record: Option<String>,
    #[serde(default)]
    pub consented_at: Option<f64>,
    #[serde(default)]
    pub consented_by: Option<String>,
}

fn default_max_parallel_agents() -> usize {
    3
}

fn default_temperature() -> f64 {
    0.4
}

fn default_provider_gates() -> Vec<ProviderGate> {
    KNOWN_PROVIDERS
        .iter()
        .map(|provider| ProviderGate {
            provider: (*provider).to_string(),
            state: ProviderGateState::Closed,
            auth_type: None,
            verified_on: None,
            terms_record: None,
            consented_at: None,
            consented_by: None,
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactorySettings {
    #[serde(default = "default_max_parallel_agents")]
    pub max_parallel_agents: usize,
    #[serde(default = "default_temperature")]
    pub default_temperature: f64,
    #[serde(default = "default_provider_gates")]
    pub provider_gates: Vec<ProviderGate>,
    #[serde(default)]
    pub default_budget: Budget,
    #[serde(default)]
    pub checks: BTreeMap<String, super::checks::ChecksConfig>,
}

impl Default for FactorySettings {
    fn default() -> Self {
        Self {
            max_parallel_agents: default_max_parallel_agents(),
            default_temperature: default_temperature(),
            provider_gates: default_provider_gates(),
            default_budget: Budget::default(),
            checks: BTreeMap::new(),
        }
    }
}

pub fn validate_title(title: &str) -> Result<String, ValidationError> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(ValidationError("title must not be empty".to_string()));
    }
    if trimmed.chars().count() > TITLE_MAX_LEN {
        return Err(ValidationError(format!(
            "title must be at most {TITLE_MAX_LEN} characters"
        )));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(ValidationError(
            "title must be a single line without control characters".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

pub fn validate_text(text: &str) -> Result<String, ValidationError> {
    if text.chars().count() > TEXT_MAX_LEN {
        return Err(ValidationError(format!(
            "text must be at most {TEXT_MAX_LEN} characters"
        )));
    }
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(ValidationError(
            "text must not contain control characters other than newline and tab".to_string(),
        ));
    }
    Ok(text.to_string())
}

pub fn validate_owner(owner: &str) -> Result<String, ValidationError> {
    if owner.is_empty() || owner.len() > OWNER_MAX_LEN {
        return Err(ValidationError(format!(
            "owner must be 1 to {OWNER_MAX_LEN} characters"
        )));
    }
    if !owner
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(ValidationError(
            "owner may only contain A-Z, a-z, 0-9, '.', '_', and '-'".to_string(),
        ));
    }
    Ok(owner.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_title_boundaries() {
        assert_eq!(validate_title("  a  ").unwrap(), "a");
        assert!(validate_title("").is_err());
        assert!(validate_title("a\nb").is_err());
        assert!(validate_title(&"a".repeat(TITLE_MAX_LEN)).is_ok());
        assert!(validate_title(&"a".repeat(TITLE_MAX_LEN + 1)).is_err());
    }

    #[test]
    fn test_validate_text_boundaries() {
        assert!(validate_text("ok\n\tok").is_ok());
        assert!(validate_text("bad\r").is_err());
        assert!(validate_text(&"a".repeat(TEXT_MAX_LEN)).is_ok());
        assert!(validate_text(&"a".repeat(TEXT_MAX_LEN + 1)).is_err());
    }

    #[test]
    fn test_validate_owner_boundaries() {
        assert!(validate_owner("u-1.a_b").is_ok());
        assert!(validate_owner("").is_err());
        assert!(validate_owner("a b").is_err());
        assert!(validate_owner(&"a".repeat(OWNER_MAX_LEN)).is_ok());
        assert!(validate_owner(&"a".repeat(OWNER_MAX_LEN + 1)).is_err());
    }
}
