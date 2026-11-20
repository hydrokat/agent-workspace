mod common;

use std::sync::{Arc, Mutex};

use tandem::adapters::base::{AdapterError, Model};
use tandem::factory::model::{FactorySettings, ModelPrice};
use tandem::factory::model_catalog::{CostBasis, ModelSource, discover};
use tandem::factory::routing::{ModelTier, ProviderModelTiers};

use common::TempDir;

fn model(id: &str, label: &str) -> Model {
    Model {
        id: id.to_string(),
        label: label.to_string(),
    }
}

fn claude_runner(cli: &str) -> Result<Vec<Model>, AdapterError> {
    if cli != "claude" {
        return Err(AdapterError::ModelList("wrong cli".to_string()));
    }
    Ok(vec![
        model("claude-haiku-4-5", "Haiku 4.5"),
        model("claude-sonnet-4-6", "Sonnet 4.6"),
        model("claude-opus-4-7", "Opus 4.7"),
    ])
}

fn offline_runner(_cli: &str) -> Result<Vec<Model>, AdapterError> {
    Err(AdapterError::ModelList("offline".to_string()))
}

fn agy_runner(cli: &str) -> Result<Vec<Model>, AdapterError> {
    if cli != "agy" {
        return Err(AdapterError::ModelList("wrong cli".to_string()));
    }
    Ok(vec![
        model("gemini-2.0-flash-lite", "Flash Lite"),
        model("gemini-2.0-flash", "Flash"),
        model("gemini-2.5-pro", "Pro"),
    ])
}

#[test]
fn tier_pins_from_model_tiers_take_priority() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();
    let models = discover("claude", &settings, dir.path(), false, &claude_runner);

    let haiku = models.iter().find(|m| m.id == "claude-haiku-4-5").unwrap();
    assert_eq!(
        haiku.tier,
        ModelTier::Economy,
        "haiku should be Economy (pin)"
    );

    let sonnet = models.iter().find(|m| m.id == "claude-sonnet-4-6").unwrap();
    assert_eq!(
        sonnet.tier,
        ModelTier::Standard,
        "sonnet should be Standard (pin)"
    );

    let opus = models.iter().find(|m| m.id == "claude-opus-4-7").unwrap();
    assert_eq!(
        opus.tier,
        ModelTier::Premium,
        "opus should be Premium (pin)"
    );
}

#[test]
fn tier_heuristic_agy_family() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();
    let models = discover("agy", &settings, dir.path(), false, &agy_runner);

    let flash_lite = models
        .iter()
        .find(|m| m.id == "gemini-2.0-flash-lite")
        .unwrap();
    assert_eq!(
        flash_lite.tier,
        ModelTier::Economy,
        "flash-lite should be Economy"
    );

    let flash = models.iter().find(|m| m.id == "gemini-2.0-flash").unwrap();
    assert_eq!(flash.tier, ModelTier::Standard, "flash should be Standard");

    let pro = models.iter().find(|m| m.id == "gemini-2.5-pro").unwrap();
    assert_eq!(pro.tier, ModelTier::Premium, "pro should be Premium");
}

#[test]
fn tier_heuristic_free_suffix_economy() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();
    let runner = |cli: &str| -> Result<Vec<Model>, AdapterError> {
        if cli != "agy" {
            return Err(AdapterError::ModelList("wrong cli".to_string()));
        }
        Ok(vec![model("gemini-2.0-flash-free", "Flash Free")])
    };
    let models = discover("agy", &settings, dir.path(), false, &runner);
    let free_model = models
        .iter()
        .find(|m| m.id == "gemini-2.0-flash-free")
        .unwrap();
    assert_eq!(
        free_model.tier,
        ModelTier::Economy,
        "*-free suffix should be Economy"
    );
}

#[test]
fn tier_heuristic_low_suffix_economy() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();
    let runner = |cli: &str| -> Result<Vec<Model>, AdapterError> {
        if cli != "agy" {
            return Err(AdapterError::ModelList("wrong cli".to_string()));
        }
        Ok(vec![model("some-model-low", "Low")])
    };
    let models = discover("agy", &settings, dir.path(), false, &runner);
    let low_model = models.iter().find(|m| m.id == "some-model-low").unwrap();
    assert_eq!(
        low_model.tier,
        ModelTier::Economy,
        "*-low suffix should be Economy"
    );
}

#[test]
fn tier_heuristic_high_and_xhigh_suffix_premium() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();
    let runner = |cli: &str| -> Result<Vec<Model>, AdapterError> {
        if cli != "agy" {
            return Err(AdapterError::ModelList("wrong cli".to_string()));
        }
        Ok(vec![
            model("some-model-high", "High"),
            model("some-model-xhigh", "XHigh"),
        ])
    };
    let models = discover("agy", &settings, dir.path(), false, &runner);
    let high = models.iter().find(|m| m.id == "some-model-high").unwrap();
    assert_eq!(high.tier, ModelTier::Premium, "*-high should be Premium");
    let xhigh = models.iter().find(|m| m.id == "some-model-xhigh").unwrap();
    assert_eq!(xhigh.tier, ModelTier::Premium, "*-xhigh should be Premium");
}

#[test]
fn cost_rank_from_price_table() {
    let mut settings = FactorySettings::default();
    settings.price_table.insert(
        "claude-haiku-4-5".to_string(),
        ModelPrice {
            input: 0.25,
            output: 1.25,
            as_of: "2026-01-01".to_string(),
        },
    );
    settings.price_table.insert(
        "claude-sonnet-4-6".to_string(),
        ModelPrice {
            input: 3.0,
            output: 15.0,
            as_of: "2026-01-01".to_string(),
        },
    );
    let dir = TempDir::new();
    let models = discover("claude", &settings, dir.path(), false, &claude_runner);

    let haiku = models.iter().find(|m| m.id == "claude-haiku-4-5").unwrap();
    let sonnet = models.iter().find(|m| m.id == "claude-sonnet-4-6").unwrap();

    // blended = (3 * input + output) / 4
    // haiku: (3 * 0.25 + 1.25) / 4 = 2.0 / 4 = 0.5
    // sonnet: (3 * 3.0 + 15.0) / 4 = 24.0 / 4 = 6.0
    assert!(
        (haiku.cost_rank - 0.5).abs() < 1e-9,
        "haiku cost_rank should be 0.5, got {}",
        haiku.cost_rank
    );
    assert!(
        (sonnet.cost_rank - 6.0).abs() < 1e-9,
        "sonnet cost_rank should be 6.0, got {}",
        sonnet.cost_rank
    );
    assert!(
        haiku.cost_rank < sonnet.cost_rank,
        "haiku cheaper than sonnet"
    );
    assert_eq!(haiku.cost_basis, CostBasis::PriceTable);
    assert_eq!(sonnet.cost_basis, CostBasis::PriceTable);
}

#[test]
fn cost_rank_free_tier_is_zero() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();
    let runner = |cli: &str| -> Result<Vec<Model>, AdapterError> {
        if cli != "agy" {
            return Err(AdapterError::ModelList("wrong cli".to_string()));
        }
        Ok(vec![
            model("gemini-2.0-flash-free", "Flash Free"),
            model("gemini-2.0-flash", "Flash"),
        ])
    };
    let models = discover("agy", &settings, dir.path(), false, &runner);
    let free_model = models
        .iter()
        .find(|m| m.id == "gemini-2.0-flash-free")
        .unwrap();
    assert_eq!(
        free_model.cost_rank, 0.0,
        "free tier cost_rank should be 0.0"
    );
    assert_eq!(free_model.cost_basis, CostBasis::FreeTier);
}

#[test]
fn cost_rank_heuristic_ordered_by_tier() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();
    let models = discover("claude", &settings, dir.path(), false, &claude_runner);

    let haiku = models.iter().find(|m| m.id == "claude-haiku-4-5").unwrap();
    let sonnet = models.iter().find(|m| m.id == "claude-sonnet-4-6").unwrap();
    let opus = models.iter().find(|m| m.id == "claude-opus-4-7").unwrap();

    assert!(
        haiku.cost_rank < sonnet.cost_rank,
        "economy cost_rank < standard cost_rank"
    );
    assert!(
        sonnet.cost_rank < opus.cost_rank,
        "standard cost_rank < premium cost_rank"
    );
    assert_eq!(haiku.cost_basis, CostBasis::Heuristic);
    assert_eq!(sonnet.cost_basis, CostBasis::Heuristic);
    assert_eq!(opus.cost_basis, CostBasis::Heuristic);
}

#[test]
fn allowed_models_filter_marks_disallowed() {
    let mut settings = FactorySettings::default();
    settings
        .allowed_models
        .insert("claude".to_string(), vec!["claude-sonnet-4-6".to_string()]);
    let dir = TempDir::new();
    let models = discover("claude", &settings, dir.path(), false, &claude_runner);

    let sonnet = models.iter().find(|m| m.id == "claude-sonnet-4-6").unwrap();
    assert!(sonnet.allowed, "sonnet should be in allowlist");

    let haiku = models.iter().find(|m| m.id == "claude-haiku-4-5").unwrap();
    assert!(!haiku.allowed, "haiku not in allowlist → not allowed");

    let opus = models.iter().find(|m| m.id == "claude-opus-4-7").unwrap();
    assert!(!opus.allowed, "opus not in allowlist → not allowed");
}

#[test]
fn all_allowed_when_no_filter() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();
    let models = discover("claude", &settings, dir.path(), false, &claude_runner);
    assert!(
        models.iter().all(|m| m.allowed),
        "empty allowlist means all allowed"
    );
}

#[test]
fn fallback_to_model_tiers_when_cli_fails() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();
    let models = discover("claude", &settings, dir.path(), false, &offline_runner);
    assert!(!models.is_empty(), "should return fallback models");
    assert!(
        models.iter().all(|m| m.source == ModelSource::Fallback),
        "all models should have Fallback source"
    );
}

#[test]
fn fallback_covers_all_three_tiers() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();
    let models = discover("claude", &settings, dir.path(), false, &offline_runner);
    let tiers: Vec<_> = models.iter().map(|m| m.tier).collect();
    assert!(
        tiers.contains(&ModelTier::Economy),
        "fallback should include Economy tier"
    );
    assert!(
        tiers.contains(&ModelTier::Standard),
        "fallback should include Standard tier"
    );
    assert!(
        tiers.contains(&ModelTier::Premium),
        "fallback should include Premium tier"
    );
}

#[test]
fn source_live_on_first_discovery() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();
    let models = discover("claude", &settings, dir.path(), false, &claude_runner);
    assert!(
        models.iter().all(|m| m.source == ModelSource::Live),
        "first discovery should produce Live source"
    );
}

#[test]
fn source_cache_within_ttl() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();
    let _ = discover("claude", &settings, dir.path(), false, &claude_runner);

    let call_count = Arc::new(Mutex::new(0u32));
    let count_ref = Arc::clone(&call_count);
    let counting_runner = move |cli: &str| -> Result<Vec<Model>, AdapterError> {
        *count_ref.lock().unwrap() += 1;
        claude_runner(cli)
    };
    let models = discover("claude", &settings, dir.path(), false, &counting_runner);
    assert_eq!(
        *call_count.lock().unwrap(),
        0,
        "within TTL should use cache, not call runner"
    );
    assert!(
        models.iter().all(|m| m.source == ModelSource::Cache),
        "cached models should have Cache source"
    );
}

#[test]
fn refresh_bypasses_cache() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();
    let _ = discover("claude", &settings, dir.path(), false, &claude_runner);

    let call_count = Arc::new(Mutex::new(0u32));
    let count_ref = Arc::clone(&call_count);
    let counting_runner = move |cli: &str| -> Result<Vec<Model>, AdapterError> {
        *count_ref.lock().unwrap() += 1;
        claude_runner(cli)
    };
    let models = discover("claude", &settings, dir.path(), true, &counting_runner);
    assert_eq!(
        *call_count.lock().unwrap(),
        1,
        "refresh=true must bypass cache and call runner"
    );
    assert!(
        models.iter().all(|m| m.source == ModelSource::Live),
        "refreshed models should have Live source"
    );
}

#[test]
fn unknown_model_gets_standard_tier_and_unclassified_flag() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();
    let runner = |cli: &str| -> Result<Vec<Model>, AdapterError> {
        if cli != "agy" {
            return Err(AdapterError::ModelList("wrong cli".to_string()));
        }
        Ok(vec![model("completely-unknown-xyz", "Unknown")])
    };
    let models = discover("agy", &settings, dir.path(), false, &runner);
    let unknown = models
        .iter()
        .find(|m| m.id == "completely-unknown-xyz")
        .unwrap();
    assert_eq!(
        unknown.tier,
        ModelTier::Standard,
        "unknown model defaults to Standard"
    );
    assert!(
        unknown.unclassified,
        "unknown model must be flagged unclassified"
    );
}

#[test]
fn opencode_models_tier_classified_by_name_part() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();
    let runner = |cli: &str| -> Result<Vec<Model>, AdapterError> {
        if cli != "opencode" {
            return Err(AdapterError::ModelList("wrong cli".to_string()));
        }
        Ok(vec![
            model("anthropic/claude-sonnet-4-6", "anthropic/claude-sonnet-4-6"),
            model("anthropic/claude-haiku-4-5", "anthropic/claude-haiku-4-5"),
            model("google/gemini-2.5-pro", "google/gemini-2.5-pro"),
        ])
    };
    let models = discover("opencode", &settings, dir.path(), false, &runner);

    let sonnet = models
        .iter()
        .find(|m| m.id == "anthropic/claude-sonnet-4-6")
        .unwrap();
    assert_eq!(sonnet.tier, ModelTier::Standard);

    let haiku = models
        .iter()
        .find(|m| m.id == "anthropic/claude-haiku-4-5")
        .unwrap();
    assert_eq!(haiku.tier, ModelTier::Economy);

    let pro = models
        .iter()
        .find(|m| m.id == "google/gemini-2.5-pro")
        .unwrap();
    assert_eq!(pro.tier, ModelTier::Premium);
}

#[test]
fn sorted_by_cost_rank_ascending() {
    let mut settings = FactorySettings::default();
    settings.price_table.insert(
        "claude-haiku-4-5".to_string(),
        ModelPrice {
            input: 0.25,
            output: 1.25,
            as_of: "2026-01-01".to_string(),
        },
    );
    settings.price_table.insert(
        "claude-sonnet-4-6".to_string(),
        ModelPrice {
            input: 3.0,
            output: 15.0,
            as_of: "2026-01-01".to_string(),
        },
    );
    settings.price_table.insert(
        "claude-opus-4-7".to_string(),
        ModelPrice {
            input: 15.0,
            output: 75.0,
            as_of: "2026-01-01".to_string(),
        },
    );
    let dir = TempDir::new();
    let models = discover("claude", &settings, dir.path(), false, &claude_runner);
    let ranks: Vec<f64> = models.iter().map(|m| m.cost_rank).collect();
    let mut sorted = ranks.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(
        ranks, sorted,
        "output should be sorted by cost_rank ascending"
    );
}

#[test]
fn custom_model_tiers_pins_override_heuristic() {
    let mut settings = FactorySettings::default();
    settings.model_tiers.insert(
        "agy".to_string(),
        ProviderModelTiers {
            economy: "gemini-2.0-flash-lite".to_string(),
            standard: "gemini-2.0-flash".to_string(),
            premium: "gemini-2.5-pro".to_string(),
        },
    );
    let dir = TempDir::new();
    let models = discover("agy", &settings, dir.path(), false, &agy_runner);
    let flash_lite = models
        .iter()
        .find(|m| m.id == "gemini-2.0-flash-lite")
        .unwrap();
    assert_eq!(
        flash_lite.tier,
        ModelTier::Economy,
        "pin should take effect"
    );
}

#[test]
fn invalid_model_ids_are_excluded() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();
    let runner = |cli: &str| -> Result<Vec<Model>, AdapterError> {
        if cli != "claude" {
            return Err(AdapterError::ModelList("wrong cli".to_string()));
        }
        Ok(vec![
            model("claude-sonnet-4-6", "Good"),
            model("../path-traversal", "Bad"),
            model("", "Empty"),
        ])
    };
    let models = discover("claude", &settings, dir.path(), false, &runner);
    assert!(
        !models.iter().any(|m| m.id == "../path-traversal"),
        "invalid model ids should be filtered"
    );
    assert!(
        !models.iter().any(|m| m.id.is_empty()),
        "empty model ids should be filtered"
    );
    assert!(models.iter().any(|m| m.id == "claude-sonnet-4-6"));
}

#[test]
fn cache_isolated_per_cli() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();

    let _ = discover("claude", &settings, dir.path(), false, &claude_runner);

    let call_count = Arc::new(Mutex::new(0u32));
    let count_ref = Arc::clone(&call_count);
    let counting_agy = move |cli: &str| -> Result<Vec<Model>, AdapterError> {
        *count_ref.lock().unwrap() += 1;
        agy_runner(cli)
    };
    let _ = discover("agy", &settings, dir.path(), false, &counting_agy);
    assert_eq!(
        *call_count.lock().unwrap(),
        1,
        "agy cache miss should trigger runner (claude cache does not bleed)"
    );
}

fn cache_file(dir: &TempDir, cli: &str) -> std::path::PathBuf {
    dir.path().join(format!("model-cache-{cli}.json"))
}

#[test]
fn cache_expires_after_ttl() {
    let settings = FactorySettings::default();
    let dir = TempDir::new();
    let _ = discover("claude", &settings, dir.path(), false, &claude_runner);
    let path = cache_file(&dir, "claude");
    let mut entry: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    entry["ts"] = serde_json::json!(1.0);
    std::fs::write(&path, entry.to_string()).unwrap();

    let calls = Arc::new(Mutex::new(0u32));
    let calls_ref = Arc::clone(&calls);
    let counting = move |cli: &str| -> Result<Vec<Model>, AdapterError> {
        *calls_ref.lock().unwrap() += 1;
        claude_runner(cli)
    };
    let models = discover("claude", &settings, dir.path(), false, &counting);
    assert_eq!(
        *calls.lock().unwrap(),
        1,
        "an expired cache must re-discover"
    );
    assert!(models.iter().all(|m| m.source == ModelSource::Live));
}

#[test]
fn cached_models_follow_current_allowed_models() {
    let dir = TempDir::new();
    let _ = discover(
        "claude",
        &FactorySettings::default(),
        dir.path(),
        false,
        &claude_runner,
    );
    let mut settings = FactorySettings::default();
    settings
        .allowed_models
        .insert("claude".to_string(), vec!["claude-haiku-4-5".to_string()]);
    let models = discover("claude", &settings, dir.path(), false, &offline_runner);
    assert!(models.iter().all(|m| m.source == ModelSource::Cache));
    let allowed: Vec<&str> = models
        .iter()
        .filter(|m| m.allowed)
        .map(|m| m.id.as_str())
        .collect();
    assert_eq!(
        allowed,
        vec!["claude-haiku-4-5"],
        "a settings change must apply to cached entries immediately"
    );
}

#[cfg(unix)]
#[test]
fn cache_file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new();
    let _ = discover(
        "claude",
        &FactorySettings::default(),
        dir.path(),
        false,
        &claude_runner,
    );
    let mode = std::fs::metadata(cache_file(&dir, "claude"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[cfg(unix)]
#[test]
fn symlinked_cache_is_never_read_or_written() {
    let dir = TempDir::new();
    let target = dir.path().join("elsewhere.json");
    std::fs::write(&target, "untouched").unwrap();
    std::os::unix::fs::symlink(&target, cache_file(&dir, "claude")).unwrap();
    let models = discover(
        "claude",
        &FactorySettings::default(),
        dir.path(),
        false,
        &claude_runner,
    );
    assert!(models.iter().all(|m| m.source == ModelSource::Live));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "untouched");
}
