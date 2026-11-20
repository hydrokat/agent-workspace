use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::adapters::base::{AdapterError, Model, validate_model_for_cli};
use crate::factory::events::{reject_if_symlink, set_private_file_mode};
use crate::factory::model::FactorySettings;
use crate::factory::routing::{ModelTier, default_model_tiers};
use crate::factory::store::{current_time, write_atomic};

const CACHE_TTL_SECS: f64 = 600.0;

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
    models: Vec<CachedModel>,
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
    if !refresh && let Some(cached) = load_cache(cache_dir, cli) {
        let mut result = enrich(cli, &cached, settings, ModelSource::Cache);
        sort_by_cost(&mut result);
        return result;
    }

    match runner(cli) {
        Ok(raw) => {
            let valid: Vec<Model> = raw
                .into_iter()
                .filter(|m| validate_model_for_cli(cli, &m.id).is_ok())
                .collect();
            let mut result = enrich(cli, &valid, settings, ModelSource::Live);
            sort_by_cost(&mut result);
            save_cache(cache_dir, cli, &valid);
            result
        }
        Err(_) => fallback(cli, settings),
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
    sort_by_cost(&mut models);
    models
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

fn load_cache(cache_dir: &Path, cli: &str) -> Option<Vec<Model>> {
    let path = cache_path(cache_dir, cli);
    if path.is_symlink() {
        return None;
    }
    let content = fs::read_to_string(&path).ok()?;
    let entry: CacheEntry = serde_json::from_str(&content).ok()?;
    let age = current_time() - entry.ts;
    if !(0.0..=CACHE_TTL_SECS).contains(&age) {
        return None;
    }
    Some(
        entry
            .models
            .into_iter()
            .map(|m| Model {
                id: m.id,
                label: m.label,
            })
            .filter(|m| validate_model_for_cli(cli, &m.id).is_ok())
            .collect(),
    )
}

fn save_cache(cache_dir: &Path, cli: &str, models: &[Model]) {
    let path = cache_path(cache_dir, cli);
    if reject_if_symlink(&path).is_err() {
        return;
    }
    let entry = CacheEntry {
        ts: current_time(),
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
