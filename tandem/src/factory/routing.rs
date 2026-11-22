use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::adapters::base::ToolFlag;
use crate::factory::model::FactorySettings;
use crate::factory::model_catalog::{CatalogModel, CostBasis, ModelSource};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Complexity {
    Low,
    Medium,
    High,
}

impl Complexity {
    pub fn min_tier(self) -> ModelTier {
        match self {
            Self::Low => ModelTier::Economy,
            Self::Medium => ModelTier::Standard,
            Self::High => ModelTier::Premium,
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CliCatalog {
    pub cli: String,
    pub models: Vec<CatalogModel>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Selection {
    pub required_tier: ModelTier,
    pub selected_tier: ModelTier,
    pub cost_basis: CostBasis,
    pub candidates: usize,
    pub rationale: String,
    pub source: ModelSource,
}

#[derive(Debug, Clone, Default)]
pub struct RoutingContext {
    pub preferred_provider: Option<String>,
    pub is_ui_work: bool,
    pub avoid_provider: Option<String>,
    pub avoid_model: Option<String>,
    pub complexity: Option<Complexity>,
    pub required_tier: Option<ModelTier>,
    pub catalog: Vec<CliCatalog>,
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
    #[error("no signed-in CLI is ready")]
    NoOpenProvider,
    #[error("provider {provider} has no interactive worker adapter")]
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

pub fn stage_floor(stage: &StageDef) -> ModelTier {
    match stage.agent {
        WorkflowAgent::Reviewer => ModelTier::Standard,
        _ => base_tier_for_stage(stage),
    }
}

pub fn required_tier(stage: &StageDef, attempt: u32, ctx: &RoutingContext) -> ModelTier {
    let mut tier = stage_floor(stage);
    if let Some(complexity) = ctx.complexity {
        tier = tier.max(complexity.min_tier());
    }
    if let Some(hint) = ctx.required_tier {
        tier = tier.max(hint);
    }
    for _ in 1..attempt {
        if let Some(next_tier) = tier.upgrade() {
            tier = next_tier;
        }
    }
    tier
}

fn cheapest_order(a: &CatalogModel, b: &CatalogModel) -> std::cmp::Ordering {
    a.cost_rank
        .total_cmp(&b.cost_rank)
        .then(a.unclassified.cmp(&b.unclassified))
        .then(a.tier.cmp(&b.tier))
        .then_with(|| a.id.cmp(&b.id))
}

fn capable(models: &[CatalogModel], required: ModelTier) -> Vec<&CatalogModel> {
    models
        .iter()
        .filter(|m| m.allowed && m.tier >= required)
        .collect()
}

fn pick_cheapest<'a>(
    models: &'a [CatalogModel],
    required: ModelTier,
    avoid_model: Option<&str>,
) -> Option<&'a CatalogModel> {
    let all = capable(models, required);
    let independent: Vec<&CatalogModel> = all
        .iter()
        .copied()
        .filter(|m| avoid_model != Some(m.id.as_str()))
        .collect();
    let pool = if independent.is_empty() {
        all
    } else {
        independent
    };
    pool.into_iter().min_by(|a, b| cheapest_order(a, b))
}

fn catalog_for<'a>(ctx: &'a RoutingContext, cli: &str) -> Option<&'a CliCatalog> {
    ctx.catalog
        .iter()
        .find(|c| c.cli == cli && !c.models.is_empty())
}

pub fn escalate(models: &[CatalogModel], failed_model: &str) -> Option<(CatalogModel, Selection)> {
    let current = models.iter().find(|m| m.id == failed_model)?;
    let next_tier = models
        .iter()
        .filter(|m| m.allowed && m.tier > current.tier)
        .map(|m| m.tier)
        .min()?;
    let pool: Vec<&CatalogModel> = models
        .iter()
        .filter(|m| m.allowed && m.tier == next_tier && m.id != failed_model)
        .collect();
    let chosen = pool.iter().copied().min_by(|a, b| cheapest_order(a, b))?;
    let selection = Selection {
        required_tier: next_tier,
        selected_tier: chosen.tier,
        cost_basis: chosen.cost_basis,
        candidates: pool.len(),
        rationale: format!(
            "escalated from {failed_model} ({:?} tier) after a failed attempt: cheapest of {} {:?}-tier candidates",
            current.tier,
            pool.len(),
            next_tier
        ),
        source: chosen.source,
    };
    Some((chosen.clone(), selection))
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
        _ => Err(Unroutable::UnsupportedProvider {
            provider: provider.to_string(),
        }),
    }
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
    workflow: &Workflow,
    settings: &FactorySettings,
    ready_clis: &[&str],
    attempt: u32,
    ctx: &RoutingContext,
) -> Result<Route, Unroutable> {
    route_interactive_selected(stage, grant, workflow, settings, ready_clis, attempt, ctx)
        .map(|(route, _)| route)
}

pub fn route_interactive_selected(
    stage: &StageDef,
    grant: &ToolGrant,
    _workflow: &Workflow,
    settings: &FactorySettings,
    ready_clis: &[&str],
    attempt: u32,
    ctx: &RoutingContext,
) -> Result<(Route, Option<Selection>), Unroutable> {
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
    let required = required_tier(stage, attempt, ctx);
    let mut rest: Vec<&str> = ready_clis
        .iter()
        .copied()
        .filter(|&cli| ctx.avoid_provider.as_deref() != Some(cli) && !ordered.contains(&cli))
        .collect();
    rest.sort_by(|&a, &b| {
        let rank = |cli: &str| {
            catalog_for(ctx, cli)
                .and_then(|c| pick_cheapest(&c.models, required, ctx.avoid_model.as_deref()))
                .map(|m| m.cost_rank)
        };
        match (rank(a), rank(b)) {
            (Some(x), Some(y)) => x.total_cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
    ordered.extend(rest);
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
        if let Some(cat) = catalog_for(ctx, provider)
            && let Some(chosen) = pick_cheapest(&cat.models, required, ctx.avoid_model.as_deref())
        {
            let candidates: usize = ready_clis
                .iter()
                .filter_map(|&cli| catalog_for(ctx, cli))
                .map(|c| capable(&c.models, required).len())
                .sum();
            let selection = Selection {
                required_tier: required,
                selected_tier: chosen.tier,
                cost_basis: chosen.cost_basis,
                candidates,
                rationale: rationale_for(chosen, required, candidates, ctx, provider),
                source: chosen.source,
            };
            let route = Route {
                agent_definition: map_agent_definition(stage.agent, ctx.is_ui_work),
                provider: provider.to_string(),
                tier: chosen.tier,
                upgraded_from: (chosen.tier != base_tier).then_some(base_tier),
                model: chosen.id.clone(),
                tool_flags,
                temperature: TemperatureSupport::Unsupported,
            };
            return Ok((route, Some(selection)));
        }
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
                        last_err = Some(Unroutable::NoAllowedModel {
                            provider: provider.to_string(),
                        });
                        continue;
                    }
                }
            }
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

        return Ok((
            Route {
                agent_definition: map_agent_definition(stage.agent, ctx.is_ui_work),
                provider: provider.to_string(),
                tier,
                upgraded_from,
                model,
                tool_flags,
                temperature: TemperatureSupport::Unsupported,
            },
            None,
        ));
    }

    Err(last_err.unwrap_or(Unroutable::NoOpenProvider))
}

fn rationale_for(
    chosen: &CatalogModel,
    required: ModelTier,
    candidates: usize,
    ctx: &RoutingContext,
    provider: &str,
) -> String {
    let mut text = format!(
        "cheapest {required:?}-or-higher model ({}/{}) of {candidates} candidates",
        provider, chosen.id
    );
    if let Some(complexity) = ctx.complexity {
        text.push_str(&format!("; complexity {}", complexity.as_str()));
    }
    if let Some(avoid) = ctx.avoid_model.as_deref() {
        text.push_str(&format!("; avoiding {avoid} for independence"));
    }
    text
}
