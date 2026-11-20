use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::adapters::agy::AgyAdapter;
use crate::adapters::base::{HeadlessAdapter, ToolFlag, validate_model_id};
use crate::adapters::claude::ClaudeAdapter;
use crate::factory::model::FactorySettings;
use crate::factory::permissions::ToolGrant;
use crate::factory::workflow::schema::{StageDef, Workflow, WorkflowAgent, WorkflowTool};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    Economy,
    Standard,
    Premium,
}

impl ModelTier {
    pub fn upgrade(self) -> Option<Self> {
        match self {
            Self::Economy => Some(Self::Standard),
            Self::Standard => Some(Self::Premium),
            Self::Premium => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderModelTiers {
    pub economy: String,
    pub standard: String,
    pub premium: String,
}

impl ProviderModelTiers {
    pub fn for_tier(&self, tier: ModelTier) -> &str {
        match tier {
            ModelTier::Economy => &self.economy,
            ModelTier::Standard => &self.standard,
            ModelTier::Premium => &self.premium,
        }
    }
}

pub fn default_model_tiers() -> BTreeMap<String, ProviderModelTiers> {
    let mut map = BTreeMap::new();
    map.insert(
        "claude".to_string(),
        ProviderModelTiers {
            economy: "claude-haiku-4-5".to_string(),
            standard: "claude-sonnet-4-6".to_string(),
            premium: "claude-opus-4-7".to_string(),
        },
    );
    map.insert(
        "agy".to_string(),
        ProviderModelTiers {
            economy: "gemini-2.0-flash-lite".to_string(),
            standard: "gemini-2.0-flash".to_string(),
            premium: "gemini-2.5-pro".to_string(),
        },
    );
    map
}

#[derive(Debug, Clone, Default)]
pub struct RoutingContext {
    pub preferred_provider: Option<String>,
    pub is_ui_work: bool,
    pub avoid_provider: Option<String>,
    pub avoid_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TemperatureSupport {
    Supported(f32),
    Unsupported,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub agent_definition: &'static str,
    pub provider: String,
    pub tier: ModelTier,
    pub upgraded_from: Option<ModelTier>,
    pub model: String,
    pub tool_flags: Vec<ToolFlag>,
    pub temperature: TemperatureSupport,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Unroutable {
    #[error("no provider gate is open")]
    NoOpenProvider,
    #[error("provider {provider} has no headless adapter")]
    UnsupportedProvider { provider: String },
    #[error("provider {provider} has no safe flag mapping for tool {tool:?}")]
    UnsupportedTool {
        provider: String,
        tool: WorkflowTool,
    },
    #[error("invalid model id {model} for provider {provider}: {reason}")]
    InvalidModel {
        provider: String,
        model: String,
        reason: String,
    },
    #[error("no allowed model left for provider {provider}")]
    NoAllowedModel { provider: String },
}

pub fn map_agent_definition(agent: WorkflowAgent, is_ui_work: bool) -> &'static str {
    match agent {
        WorkflowAgent::Orchestrator => "tech-lead-orchestrator",
        WorkflowAgent::TestEngineer | WorkflowAgent::Verifier => "qa-engineer",
        WorkflowAgent::Implementer => {
            if is_ui_work {
                "frontend-dev"
            } else {
                "senior-backend-dev"
            }
        }
        WorkflowAgent::Reviewer => "code-reviewer",
        WorkflowAgent::DemoMaker => "frontend-dev",
        WorkflowAgent::ReleaseEngineer => "terminal-agent",
    }
}

pub fn base_tier_for_stage(stage: &StageDef) -> ModelTier {
    match stage.agent {
        WorkflowAgent::Orchestrator | WorkflowAgent::Implementer => ModelTier::Standard,
        WorkflowAgent::TestEngineer
        | WorkflowAgent::Verifier
        | WorkflowAgent::Reviewer
        | WorkflowAgent::DemoMaker
        | WorkflowAgent::ReleaseEngineer => ModelTier::Economy,
    }
}

fn choose_provider<'a>(
    open_providers: &[&'a str],
    preferred: Option<&str>,
    avoid: Option<&str>,
) -> Result<&'a str, Unroutable> {
    if open_providers.is_empty() {
        return Err(Unroutable::NoOpenProvider);
    }
    let candidates: Vec<&'a str> = if let Some(av) = avoid {
        let filtered: Vec<&'a str> = open_providers
            .iter()
            .copied()
            .filter(|&p| p != av)
            .collect();
        if filtered.is_empty() {
            open_providers.to_vec()
        } else {
            filtered
        }
    } else {
        open_providers.to_vec()
    };
    if let Some(pref) = preferred
        && let Some(&found) = candidates.iter().find(|&&p| p == pref)
    {
        return Ok(found);
    }
    Ok(candidates[0])
}

fn map_tool_for_provider(
    provider: &str,
    tool: WorkflowTool,
    out: &mut Vec<ToolFlag>,
) -> Result<(), Unroutable> {
    match provider {
        "claude" => match tool {
            WorkflowTool::RepoRead | WorkflowTool::IssueContext | WorkflowTool::DiffReview => {
                for name in ["Read", "Glob", "Grep"] {
                    let flag = ToolFlag::Allow(name.to_string());
                    if !out.contains(&flag) {
                        out.push(flag);
                    }
                }
                Ok(())
            }
            WorkflowTool::RepoWrite => {
                for name in ["Edit", "Write"] {
                    let flag = ToolFlag::Allow(name.to_string());
                    if !out.contains(&flag) {
                        out.push(flag);
                    }
                }
                Ok(())
            }
            WorkflowTool::Shell | WorkflowTool::TestRunner | WorkflowTool::LintRunner => {
                let flag = ToolFlag::Allow("Bash".to_string());
                if !out.contains(&flag) {
                    out.push(flag);
                }
                Ok(())
            }
            WorkflowTool::Ci | WorkflowTool::Deployment => {
                let flag = ToolFlag::Allow("Bash(gh:*)".to_string());
                if !out.contains(&flag) {
                    out.push(flag);
                }
                Ok(())
            }
            WorkflowTool::Browser | WorkflowTool::Screenshot => Err(Unroutable::UnsupportedTool {
                provider: provider.to_string(),
                tool,
            }),
        },
        "agy" => match tool {
            WorkflowTool::RepoRead | WorkflowTool::IssueContext | WorkflowTool::DiffReview => {
                Ok(())
            }
            _ => Err(Unroutable::UnsupportedTool {
                provider: provider.to_string(),
                tool,
            }),
        },
        _ => Err(Unroutable::UnsupportedProvider {
            provider: provider.to_string(),
        }),
    }
}

pub fn route(
    stage: &StageDef,
    grant: &ToolGrant,
    workflow: &Workflow,
    settings: &FactorySettings,
    open_providers: &[&str],
    attempt: u32,
    ctx: &RoutingContext,
) -> Result<Route, Unroutable> {
    let provider = choose_provider(
        open_providers,
        ctx.preferred_provider.as_deref(),
        ctx.avoid_provider.as_deref(),
    )?;
    let caps = match provider {
        "claude" => ClaudeAdapter::default().capabilities(),
        "agy" => AgyAdapter::default().capabilities(),
        other => {
            return Err(Unroutable::UnsupportedProvider {
                provider: other.to_string(),
            });
        }
    };

    let base_tier = base_tier_for_stage(stage);
    let mut tier = base_tier;
    for _ in 1..attempt {
        if let Some(next_tier) = tier.upgrade() {
            tier = next_tier;
        }
    }

    let default_tiers = default_model_tiers();
    let provider_tiers = settings
        .model_tiers
        .get(provider)
        .or_else(|| default_tiers.get(provider))
        .ok_or_else(|| Unroutable::UnsupportedProvider {
            provider: provider.to_string(),
        })?;
    if let Some(ref avoid_model) = ctx.avoid_model
        && provider_tiers.for_tier(tier) == avoid_model.as_str()
        && let Some(next_tier) = tier.upgrade()
    {
        tier = next_tier;
    }

    if let Some(allowed) = settings.allowed_models.get(provider) {
        let tier_is_allowed = |t: ModelTier| {
            let candidate = provider_tiers.for_tier(t);
            allowed.iter().any(|m| m == candidate)
        };
        if !tier_is_allowed(tier) {
            let candidates = [
                tier.upgrade(),
                Some(ModelTier::Standard),
                Some(ModelTier::Premium),
                Some(ModelTier::Economy),
            ];
            let found = candidates
                .into_iter()
                .flatten()
                .find(|&t| {
                    tier_is_allowed(t)
                        && ctx.avoid_model.as_deref() != Some(provider_tiers.for_tier(t))
                })
                .or_else(|| {
                    [ModelTier::Economy, ModelTier::Standard, ModelTier::Premium]
                        .into_iter()
                        .find(|&t| tier_is_allowed(t))
                });
            match found {
                Some(t) => tier = t,
                None => {
                    return Err(Unroutable::NoAllowedModel {
                        provider: provider.to_string(),
                    });
                }
            }
        }
    }

    let upgraded_from = if tier != base_tier {
        Some(base_tier)
    } else {
        None
    };
    let model = provider_tiers.for_tier(tier).to_string();
    validate_model_id(&model).map_err(|reason| Unroutable::InvalidModel {
        provider: provider.to_string(),
        model: model.clone(),
        reason: reason.to_string(),
    })?;

    let mut tool_flags = Vec::new();
    for tool in &grant.tools {
        map_tool_for_provider(provider, *tool, &mut tool_flags)?;
    }

    let eff = crate::factory::settings::effective(&workflow.policy, settings);
    let temperature = if caps.temperature {
        TemperatureSupport::Supported(eff.default_temperature as f32)
    } else {
        TemperatureSupport::Unsupported
    };

    Ok(Route {
        agent_definition: map_agent_definition(stage.agent, ctx.is_ui_work),
        provider: provider.to_string(),
        tier,
        upgraded_from,
        model,
        tool_flags,
        temperature,
    })
}

pub(crate) fn interactive_map_tool(
    provider: &str,
    tool: WorkflowTool,
    out: &mut Vec<ToolFlag>,
) -> Result<(), Unroutable> {
    match provider {
        "claude" => map_tool_for_provider("claude", tool, out),
        "agy" => match tool {
            WorkflowTool::RepoRead
            | WorkflowTool::IssueContext
            | WorkflowTool::DiffReview
            | WorkflowTool::RepoWrite
            | WorkflowTool::Shell
            | WorkflowTool::TestRunner
            | WorkflowTool::LintRunner => Ok(()),
            WorkflowTool::Ci
            | WorkflowTool::Deployment
            | WorkflowTool::Browser
            | WorkflowTool::Screenshot => Err(Unroutable::UnsupportedTool {
                provider: provider.to_string(),
                tool,
            }),
        },
        "opencode" => match tool {
            WorkflowTool::RepoWrite
            | WorkflowTool::Shell
            | WorkflowTool::TestRunner
            | WorkflowTool::LintRunner
            | WorkflowTool::RepoRead => Ok(()),
            WorkflowTool::IssueContext
            | WorkflowTool::DiffReview
            | WorkflowTool::Ci
            | WorkflowTool::Deployment
            | WorkflowTool::Browser
            | WorkflowTool::Screenshot => Err(Unroutable::UnsupportedTool {
                provider: provider.to_string(),
                tool,
            }),
        },
        other => Err(Unroutable::UnsupportedProvider {
            provider: other.to_string(),
        }),
    }
}

pub fn route_interactive(
    stage: &StageDef,
    grant: &ToolGrant,
    _workflow: &Workflow,
    settings: &FactorySettings,
    ready_clis: &[&str],
    attempt: u32,
    ctx: &RoutingContext,
) -> Result<Route, Unroutable> {
    if ready_clis.is_empty() {
        return Err(Unroutable::NoOpenProvider);
    }

    let mut ordered: Vec<&str> = Vec::with_capacity(ready_clis.len());
    if let Some(ref pref) = ctx.preferred_provider
        && let Some(&found) = ready_clis.iter().find(|&&c| c == pref.as_str())
        && ctx.avoid_provider.as_deref() != Some(found)
    {
        ordered.push(found);
    }
    for &cli in ready_clis {
        if ctx.avoid_provider.as_deref() != Some(cli) && !ordered.contains(&cli) {
            ordered.push(cli);
        }
    }
    for &cli in ready_clis {
        if !ordered.contains(&cli) {
            ordered.push(cli);
        }
    }

    let mut last_err: Option<Unroutable> = None;
    for provider in ordered {
        let mut tool_flags = Vec::new();
        let mut mapped_ok = true;
        for &tool in &grant.tools {
            if let Err(err) = interactive_map_tool(provider, tool, &mut tool_flags) {
                last_err = Some(err);
                mapped_ok = false;
                break;
            }
        }
        if !mapped_ok {
            continue;
        }
        if provider == "opencode" && !grant.has_repo_write {
            last_err = Some(Unroutable::UnsupportedTool {
                provider: provider.to_string(),
                tool: WorkflowTool::DiffReview,
            });
            continue;
        }

        let base_tier = base_tier_for_stage(stage);
        let mut tier = base_tier;
        for _ in 1..attempt {
            if let Some(next_tier) = tier.upgrade() {
                tier = next_tier;
            }
        }
        let default_tiers = default_model_tiers();
        let Some(provider_tiers) = settings
            .model_tiers
            .get(provider)
            .or_else(|| default_tiers.get(provider))
        else {
            last_err = Some(Unroutable::UnsupportedProvider {
                provider: provider.to_string(),
            });
            continue;
        };
        if let Some(ref avoid_model) = ctx.avoid_model
            && provider_tiers.for_tier(tier) == avoid_model.as_str()
            && let Some(next_tier) = tier.upgrade()
        {
            tier = next_tier;
        }
        let upgraded_from = (tier != base_tier).then_some(base_tier);
        let model = provider_tiers.for_tier(tier).to_string();
        if let Err(reason) = crate::adapters::base::validate_model_for_cli(provider, &model) {
            last_err = Some(Unroutable::InvalidModel {
                provider: provider.to_string(),
                model,
                reason: reason.to_string(),
            });
            continue;
        }

        return Ok(Route {
            agent_definition: map_agent_definition(stage.agent, ctx.is_ui_work),
            provider: provider.to_string(),
            tier,
            upgraded_from,
            model,
            tool_flags,
            temperature: TemperatureSupport::Unsupported,
        });
    }

    Err(last_err.unwrap_or(Unroutable::NoOpenProvider))
}
