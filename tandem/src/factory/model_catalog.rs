use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::adapters::base::{AdapterError, Model, validate_model_for_cli};
use crate::adapters::claude::CLAUDE_ALIASES;
use crate::factory::events::{reject_if_symlink, set_private_file_mode};
use crate::factory::model::FactorySettings;
use crate::factory::routing::{ModelTier, default_model_tiers};
use crate::factory::store::{current_time, write_atomic};

const CACHE_TTL_SECS: f64 = 600.0;
const FAILURE_TTL_SECS: f64 = 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostBasis {
    PriceTable,
    FreeTier,
    Heuristic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSource {
    Live,
    Cache,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogModel {
    pub id: String,
    pub label: String,
    pub tier: ModelTier,
    pub cost_rank: f64,
    pub cost_basis: CostBasis,
    pub allowed: bool,
    pub source: ModelSource,
    #[serde(default)]
    pub unclassified: bool,
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    ts: f64,
    #[serde(default)]
    failed: bool,
    models: Vec<CachedModel>,
}

enum Cached {
    Models(Vec<Model>),
    Failed,
}

#[derive(Serialize, Deserialize)]
struct CachedModel {
    id: String,
    label: String,
}

pub fn discover(
    cli: &str,
    settings: &FactorySettings,
    cache_dir: &Path,
    refresh: bool,
    runner: &dyn Fn(&str) -> Result<Vec<Model>, AdapterError>,
) -> Vec<CatalogModel> {
    if !refresh {
        match load_cache(cache_dir, cli) {
            Some(Cached::Models(cached)) => {
                let mut result = enrich(cli, &cached, settings, ModelSource::Cache);
                sort_by_cost(&mut result);
                return result;
            }
            Some(Cached::Failed) => return fallback(cli, settings),
            None => {}
        }
    }

    match runner(cli) {
        Ok(raw) => {
            let valid: Vec<Model> = raw
                .into_iter()
                .filter(|m| validate_model_for_cli(cli, &m.id).is_ok())
                .collect();
            let mut result = enrich(cli, &valid, settings, ModelSource::Live);
            sort_by_cost(&mut result);
            save_cache(cache_dir, cli, &valid, false);
            result
        }
        Err(_) => {
            if !refresh {
                save_cache(cache_dir, cli, &[], true);
            }
            fallback(cli, settings)
        }
    }
}

fn enrich(
    cli: &str,
    raw: &[Model],
    settings: &FactorySettings,
    source: ModelSource,
) -> Vec<CatalogModel> {
    let default_tiers = default_model_tiers();
    let pinned = settings
        .model_tiers
        .get(cli)
        .or_else(|| default_tiers.get(cli));

    let allowed_list = settings.allowed_models.get(cli);

    raw.iter()
        .map(|m| {
            let (tier, unclassified) = classify_tier(&m.id, pinned);
            let (cost_rank, cost_basis) = compute_cost(&m.id, settings, tier);
            let allowed = match allowed_list {
                None => true,
                Some(list) if list.is_empty() => true,
                Some(list) => list.iter().any(|a| a == &m.id),
            };
            CatalogModel {
                id: m.id.clone(),
                label: m.label.clone(),
                tier,
                cost_rank,
                cost_basis,
                allowed,
                source,
                unclassified,
            }
        })
        .collect()
}

fn fallback(cli: &str, settings: &FactorySettings) -> Vec<CatalogModel> {
    let default_tiers = default_model_tiers();
    let Some(tiers) = settings
        .model_tiers
        .get(cli)
        .or_else(|| default_tiers.get(cli))
    else {
        return Vec::new();
    };

    let raw: Vec<Model> = [
        (tiers.economy.as_str(), "Economy"),
        (tiers.standard.as_str(), "Standard"),
        (tiers.premium.as_str(), "Premium"),
    ]
    .into_iter()
    .filter(|(id, _)| !id.is_empty() && validate_model_for_cli(cli, id).is_ok())
    .map(|(id, _)| Model {
        id: id.to_string(),
        label: id.to_string(),
    })
    .collect();

    let mut result = enrich(cli, &raw, settings, ModelSource::Fallback);
    sort_by_cost(&mut result);
    result
}

fn source_rank(source: ModelSource) -> u8 {
    match source {
        ModelSource::Live => 0,
        ModelSource::Cache => 1,
        ModelSource::Fallback => 2,
    }
}

pub fn with_pinned(
    cli: &str,
    mut models: Vec<CatalogModel>,
    settings: &FactorySettings,
) -> Vec<CatalogModel> {
    for pinned in fallback(cli, settings) {
        if !models.iter().any(|m| m.id == pinned.id) {
            models.push(pinned);
        }
    }
    models.sort_by(|a, b| {
        a.cost_rank
            .total_cmp(&b.cost_rank)
            .then_with(|| source_rank(a.source).cmp(&source_rank(b.source)))
    });
    models
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ModelFreshnessStatus {
    Current,
    MissingFromCatalog,
    NewerAvailable { recommended: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelDriftItem {
    pub cli: String,
    pub tier: ModelTier,
    pub configured: String,
    pub status: ModelFreshnessStatus,
    pub available_models: Vec<CatalogModel>,
    pub available_signature: String,
    pub source: ModelSource,
    pub needs_attention: bool,
}

impl ModelDriftItem {
    pub fn recommended_model(&self) -> Option<&str> {
        match &self.status {
            ModelFreshnessStatus::NewerAvailable { recommended } => Some(recommended.as_str()),
            ModelFreshnessStatus::MissingFromCatalog => self
                .available_models
                .iter()
                .find(|m| m.allowed)
                .map(|m| m.id.as_str()),
            ModelFreshnessStatus::Current => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelVerificationReport {
    pub items: Vec<ModelDriftItem>,
}

impl ModelVerificationReport {
    pub fn needs_attention_items(&self) -> Vec<&ModelDriftItem> {
        self.items.iter().filter(|i| i.needs_attention).collect()
    }

    pub fn needs_attention_count(&self) -> usize {
        self.items.iter().filter(|i| i.needs_attention).count()
    }
}

pub fn catalog_signature(models: &[CatalogModel]) -> String {
    let mut ids: Vec<&str> = models
        .iter()
        .filter(|m| m.source != ModelSource::Fallback)
        .map(|m| m.id.as_str())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return String::new();
    }
    let mut hasher = Sha256::new();
    hasher.update(ids.join("\n").as_bytes());
    let digest = hasher.finalize();
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

fn extract_version_tuple(id: &str) -> Vec<u64> {
    let part = name_part(id);
    let mut nums = Vec::new();
    let mut cur: Option<u64> = None;
    for ch in part.chars() {
        if let Some(d) = ch.to_digit(10) {
            cur = Some(cur.unwrap_or(0).saturating_mul(10).saturating_add(d as u64));
        } else if let Some(n) = cur.take() {
            nums.push(n);
        }
    }
    if let Some(n) = cur {
        nums.push(n);
    }
    nums
}

fn is_claude_alias_only(catalog: &[CatalogModel]) -> bool {
    !catalog.is_empty()
        && catalog
            .iter()
            .all(|m| CLAUDE_ALIASES.contains(&m.id.as_str()))
}

fn claude_alias_covers(configured: &str, catalog: &[CatalogModel]) -> bool {
    let lower = name_part(configured).to_ascii_lowercase();
    if catalog.iter().any(|m| m.id == configured) {
        return true;
    }
    if lower.contains("sonnet") {
        return catalog.iter().any(|m| m.id == "sonnet");
    }
    if lower.contains("opus") {
        return catalog.iter().any(|m| m.id == "opus");
    }
    if lower.contains("haiku") || lower.contains("fable") {
        return catalog
            .iter()
            .any(|m| matches!(m.id.as_str(), "fable" | "sonnet" | "opus"));
    }
    false
}

fn ordered_candidates_for_tier(catalog: &[CatalogModel], tier: ModelTier) -> Vec<CatalogModel> {
    let mut exact_tier: Vec<CatalogModel> = catalog
        .iter()
        .filter(|m| m.allowed && classify_tier(&m.id, None).0 == tier)
        .cloned()
        .collect();
    let mut rest: Vec<CatalogModel> = catalog
        .iter()
        .filter(|m| m.allowed && classify_tier(&m.id, None).0 != tier)
        .cloned()
        .collect();

    let cmp_desc = |a: &CatalogModel, b: &CatalogModel| {
        extract_version_tuple(&b.id)
            .cmp(&extract_version_tuple(&a.id))
            .then_with(|| a.cost_rank.total_cmp(&b.cost_rank))
            .then_with(|| a.id.cmp(&b.id))
    };
    exact_tier.sort_by(cmp_desc);
    rest.sort_by(cmp_desc);
    exact_tier.extend(rest);
    exact_tier
}

pub fn verify_configured_models(
    clis: &[&str],
    settings: &FactorySettings,
    cache_dir: &Path,
    refresh: bool,
    runner: &dyn Fn(&str) -> Result<Vec<Model>, AdapterError>,
) -> ModelVerificationReport {
    let default_tiers = default_model_tiers();
    let mut items = Vec::new();

    for &cli in clis {
        let Some(tiers) = settings
            .model_tiers
            .get(cli)
            .or_else(|| default_tiers.get(cli))
        else {
            continue;
        };

        let catalog = discover(cli, settings, cache_dir, refresh, runner);
        let source = catalog
            .first()
            .map(|m| m.source)
            .unwrap_or(ModelSource::Fallback);
        let has_live_catalog =
            !catalog.is_empty() && catalog.iter().any(|m| m.source != ModelSource::Fallback);
        let signature = if has_live_catalog {
            catalog_signature(&catalog)
        } else {
            String::new()
        };
        let alias_only = cli == "claude" && is_claude_alias_only(&catalog);

        for (tier, tier_name, configured) in [
            (ModelTier::Economy, "economy", tiers.economy.as_str()),
            (ModelTier::Standard, "standard", tiers.standard.as_str()),
            (ModelTier::Premium, "premium", tiers.premium.as_str()),
        ] {
            if !has_live_catalog {
                items.push(ModelDriftItem {
                    cli: cli.to_string(),
                    tier,
                    configured: configured.to_string(),
                    status: ModelFreshnessStatus::Current,
                    available_models: Vec::new(),
                    available_signature: String::new(),
                    source: ModelSource::Fallback,
                    needs_attention: false,
                });
                continue;
            }

            let candidates = ordered_candidates_for_tier(&catalog, tier);
            let status = if alias_only {
                if claude_alias_covers(configured, &catalog) {
                    ModelFreshnessStatus::Current
                } else {
                    ModelFreshnessStatus::MissingFromCatalog
                }
            } else {
                let in_catalog = catalog.iter().any(|m| m.id == configured);
                if !in_catalog {
                    ModelFreshnessStatus::MissingFromCatalog
                } else {
                    let configured_ver = extract_version_tuple(configured);
                    let newer = candidates.iter().find(|c| {
                        c.id != configured
                            && classify_tier(&c.id, None).0 == tier
                            && extract_version_tuple(&c.id) > configured_ver
                    });
                    if let Some(best) = newer {
                        ModelFreshnessStatus::NewerAvailable {
                            recommended: best.id.clone(),
                        }
                    } else {
                        ModelFreshnessStatus::Current
                    }
                }
            };

            let actionable = !matches!(status, ModelFreshnessStatus::Current);
            let suppressed = settings
                .model_decisions
                .get(cli)
                .and_then(|tiers_map| tiers_map.get(tier_name))
                .is_some_and(|dec| {
                    dec.chosen == configured && dec.available_signature == signature
                });

            items.push(ModelDriftItem {
                cli: cli.to_string(),
                tier,
                configured: configured.to_string(),
                status,
                available_models: candidates,
                available_signature: signature.clone(),
                source,
                needs_attention: actionable && !suppressed,
            });
        }
    }

    ModelVerificationReport { items }
}

fn sort_by_cost(models: &mut [CatalogModel]) {
    models.sort_by(|a, b| a.cost_rank.total_cmp(&b.cost_rank));
}

fn name_part(id: &str) -> &str {
    id.rfind('/').map(|i| &id[i + 1..]).unwrap_or(id)
}

fn classify_tier(
    id: &str,
    pinned: Option<&crate::factory::routing::ProviderModelTiers>,
) -> (ModelTier, bool) {
    if let Some(pins) = pinned {
        if id == pins.economy {
            return (ModelTier::Economy, false);
        }
        if id == pins.standard {
            return (ModelTier::Standard, false);
        }
        if id == pins.premium {
            return (ModelTier::Premium, false);
        }
    }

    let lower = name_part(id).to_lowercase();

    let is_economy = lower.contains("flash-lite")
        || lower.contains("flash_lite")
        || lower.contains("haiku")
        || lower.contains("-mini")
        || lower.starts_with("mini-")
        || lower == "mini"
        || lower.ends_with("-low")
        || lower.ends_with("-free");

    if is_economy {
        return (ModelTier::Economy, false);
    }

    let is_standard = lower.contains("sonnet")
        || lower.contains("-flash")
        || lower.starts_with("flash")
        || lower.ends_with("-medium");

    if is_standard {
        return (ModelTier::Standard, false);
    }

    let is_premium = lower.contains("opus")
        || lower.ends_with("-pro")
        || lower.contains("-pro-")
        || lower == "pro"
        || lower.ends_with("-high")
        || lower.ends_with("-xhigh");

    if is_premium {
        return (ModelTier::Premium, false);
    }

    (ModelTier::Standard, true)
}

fn compute_cost(id: &str, settings: &FactorySettings, tier: ModelTier) -> (f64, CostBasis) {
    if let Some(price) = settings.price_table.get(id) {
        let blended = (3.0 * price.input + price.output) / 4.0;
        return (blended, CostBasis::PriceTable);
    }

    if let Some(name) = id.split_once('/').map(|(_, n)| n)
        && let Some(price) = settings.price_table.get(name)
    {
        let blended = (3.0 * price.input + price.output) / 4.0;
        return (blended, CostBasis::PriceTable);
    }

    let lower = name_part(id).to_lowercase();
    if lower.ends_with("-free") {
        return (0.0, CostBasis::FreeTier);
    }

    let rank = match tier {
        ModelTier::Economy => 1.0,
        ModelTier::Standard => 10.0,
        ModelTier::Premium => 100.0,
    };
    (rank, CostBasis::Heuristic)
}

fn cache_path(cache_dir: &Path, cli: &str) -> std::path::PathBuf {
    cache_dir.join(format!("model-cache-{cli}.json"))
}

fn load_cache(cache_dir: &Path, cli: &str) -> Option<Cached> {
    let path = cache_path(cache_dir, cli);
    if path.is_symlink() {
        return None;
    }
    let content = fs::read_to_string(&path).ok()?;
    let entry: CacheEntry = serde_json::from_str(&content).ok()?;
    let age = current_time() - entry.ts;
    if entry.failed {
        return (0.0..=FAILURE_TTL_SECS)
            .contains(&age)
            .then_some(Cached::Failed);
    }
    if !(0.0..=CACHE_TTL_SECS).contains(&age) {
        return None;
    }
    Some(Cached::Models(
        entry
            .models
            .into_iter()
            .map(|m| Model {
                id: m.id,
                label: m.label,
            })
            .filter(|m| validate_model_for_cli(cli, &m.id).is_ok())
            .collect(),
    ))
}

fn save_cache(cache_dir: &Path, cli: &str, models: &[Model], failed: bool) {
    let path = cache_path(cache_dir, cli);
    if reject_if_symlink(&path).is_err() {
        return;
    }
    let entry = CacheEntry {
        ts: current_time(),
        failed,
        models: models
            .iter()
            .map(|m| CachedModel {
                id: m.id.clone(),
                label: m.label.clone(),
            })
            .collect(),
    };
    let Ok(bytes) = serde_json::to_vec_pretty(&entry) else {
        return;
    };
    if write_atomic(&path, &bytes).is_ok() {
        let _ = set_private_file_mode(&path);
    }
}
