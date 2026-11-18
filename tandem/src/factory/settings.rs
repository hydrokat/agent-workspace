use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};

use serde::Serialize;
use serde_json::Value;

use super::FactoryError;
use super::approval::{ApprovalError, ensure_user_caller};
use super::events::{Event, append_locked, reject_if_symlink, set_private_file_mode};
use super::machine::EventData;
use super::model::{Actor, ActorKind, Budget, FactorySettings, ModelPrice};
use super::routing::ProviderModelTiers;
use super::store::{FactoryStore, current_time, write_atomic};
use super::workflow::schema::Policy;
use super::workflow::validate::is_valid_workflow_identifier;
use crate::adapters::base::validate_model_id;
use crate::coordination::filelock::exclusive_lock;

pub const SETTINGS_VERSION: u32 = 1;
pub const MIN_PARALLEL_AGENTS: usize = 1;
pub const MAX_PARALLEL_AGENTS: usize = 8;
pub const MIN_TEMPERATURE: f64 = 0.0;
pub const MAX_TEMPERATURE: f64 = 1.0;
pub const MAX_REVIEW_ROUNDS: u32 = 5;
pub const MAX_APPROVAL_GRACE_SECS: u64 = 600;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SettingsChangeResult {
    pub key: String,
    pub old_value: Value,
    pub new_value: Value,
    pub actor: String,
    pub settings: FactorySettings,
}

pub fn load(store: &FactoryStore) -> Result<FactorySettings, FactoryError> {
    reject_if_symlink(store.root_dir())?;
    let path = store.settings_path();
    reject_if_symlink(&path)?;

    match fs::read_to_string(&path) {
        Ok(content) => Ok(serde_json::from_str(&content)?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(FactorySettings::default()),
        Err(err) => Err(FactoryError::Io(err)),
    }
}

pub fn save(store: &FactoryStore, settings: &FactorySettings) -> Result<(), FactoryError> {
    validate(settings)?;
    store.ensure()?;
    save_unlocked(store, settings)
}

fn save_unlocked(store: &FactoryStore, settings: &FactorySettings) -> Result<(), FactoryError> {
    let path = store.settings_path();
    reject_if_symlink(&path)?;
    let mut normalized = settings.clone();
    if normalized.version == 0 {
        normalized.version = SETTINGS_VERSION;
    }
    let bytes = serde_json::to_vec_pretty(&normalized)?;
    write_atomic(&path, &bytes)?;
    set_private_file_mode(&path)?;
    Ok(())
}

pub fn validate(settings: &FactorySettings) -> Result<(), FactoryError> {
    if !(MIN_PARALLEL_AGENTS..=MAX_PARALLEL_AGENTS).contains(&settings.max_parallel_agents) {
        return Err(FactoryError::InvalidInput(format!(
            "max_parallel_agents must be between {MIN_PARALLEL_AGENTS} and {MAX_PARALLEL_AGENTS} (got {})",
            settings.max_parallel_agents
        )));
    }
    if settings.default_temperature.is_nan()
        || !(MIN_TEMPERATURE..=MAX_TEMPERATURE).contains(&settings.default_temperature)
    {
        return Err(FactoryError::InvalidInput(format!(
            "default_temperature must be between {MIN_TEMPERATURE:.1} and {MAX_TEMPERATURE:.1} (got {})",
            settings.default_temperature
        )));
    }
    if !is_valid_workflow_identifier(&settings.default_workflow) {
        return Err(FactoryError::InvalidInput(format!(
            "default_workflow \"{}\" must match ^[a-z][a-z0-9-]{{0,47}}$",
            settings.default_workflow
        )));
    }
    if settings.max_review_rounds > MAX_REVIEW_ROUNDS {
        return Err(FactoryError::InvalidInput(format!(
            "max_review_rounds must be between 0 and {MAX_REVIEW_ROUNDS} (got {})",
            settings.max_review_rounds
        )));
    }
    if let Some(cost) = settings.default_budget.max_cost_usd
        && (!cost.is_finite() || cost < 0.0)
    {
        return Err(FactoryError::InvalidInput(format!(
            "default_budget.max_cost_usd must be >= 0.0 (got {cost})"
        )));
    }
    for (provider, tiers) in &settings.model_tiers {
        if provider.trim().is_empty() {
            return Err(FactoryError::InvalidInput(
                "model_tiers provider name must not be empty".to_string(),
            ));
        }
        validate_provider_tiers(provider, tiers)?;
    }
    for (model, price) in &settings.price_table {
        validate_model_id(model).map_err(|reason| {
            FactoryError::InvalidInput(format!("price_table model \"{model}\" invalid: {reason}"))
        })?;
        validate_model_price(model, price)?;
    }
    if settings.approval_grace_secs > MAX_APPROVAL_GRACE_SECS {
        return Err(FactoryError::InvalidInput(format!(
            "approval_grace_secs must be between 0 and {MAX_APPROVAL_GRACE_SECS} (got {})",
            settings.approval_grace_secs
        )));
    }
    Ok(())
}

fn validate_provider_tiers(provider: &str, tiers: &ProviderModelTiers) -> Result<(), FactoryError> {
    for (tier_name, model_id) in [
        ("economy", &tiers.economy),
        ("standard", &tiers.standard),
        ("premium", &tiers.premium),
    ] {
        validate_model_id(model_id).map_err(|reason| {
            FactoryError::InvalidInput(format!(
                "model_tiers.{provider}.{tier_name} \"{model_id}\" invalid: {reason}"
            ))
        })?;
    }
    Ok(())
}

fn validate_model_price(model: &str, price: &ModelPrice) -> Result<(), FactoryError> {
    if !price.input.is_finite() || price.input < 0.0 {
        return Err(FactoryError::InvalidInput(format!(
            "price_table.{model}.input must be >= 0.0 (got {})",
            price.input
        )));
    }
    if !price.output.is_finite() || price.output < 0.0 {
        return Err(FactoryError::InvalidInput(format!(
            "price_table.{model}.output must be >= 0.0 (got {})",
            price.output
        )));
    }
    if price.as_of.trim().is_empty() {
        return Err(FactoryError::InvalidInput(format!(
            "price_table.{model}.as_of must not be empty"
        )));
    }
    Ok(())
}

pub fn effective(policy: &Policy, settings: &FactorySettings) -> Policy {
    let max_parallel = policy
        .max_parallel_agents
        .min(settings.max_parallel_agents.min(u32::MAX as usize) as u32);
    let default_temperature = if policy.default_temperature.is_nan() {
        settings.default_temperature
    } else if settings.default_temperature.is_nan() {
        policy.default_temperature
    } else {
        policy.default_temperature.min(settings.default_temperature)
    };
    Policy {
        max_parallel_agents: max_parallel,
        default_temperature,
        require_human_approval: policy.require_human_approval.clone(),
    }
}

fn is_protected_key(key: &str) -> bool {
    let root = key.split('.').next().unwrap_or(key);
    matches!(
        root,
        "provider_gates" | "providers" | "checks" | "deploy" | "deploy_command" | "version"
    )
}

fn parse_string_value(raw: &str) -> String {
    if let Ok(Value::String(s)) = serde_json::from_str::<Value>(raw) {
        s
    } else {
        raw.to_string()
    }
}

fn parse_optional<T: serde::de::DeserializeOwned>(raw: &str) -> Result<Option<T>, FactoryError> {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("null") || trimmed.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    serde_json::from_str::<T>(trimmed)
        .map(Some)
        .map_err(|e| FactoryError::InvalidInput(format!("invalid value \"{raw}\": {e}")))
}

fn apply_setting_change(
    settings: &mut FactorySettings,
    key: &str,
    raw_value: &str,
) -> Result<(Value, Value), FactoryError> {
    match key {
        "max_parallel_agents" => {
            let old = serde_json::to_value(settings.max_parallel_agents)?;
            let parsed: usize = raw_value.trim().parse().map_err(|_| {
                FactoryError::InvalidInput(format!(
                    "max_parallel_agents must be an integer between {MIN_PARALLEL_AGENTS} and {MAX_PARALLEL_AGENTS}"
                ))
            })?;
            settings.max_parallel_agents = parsed;
            let new = serde_json::to_value(settings.max_parallel_agents)?;
            Ok((old, new))
        }
        "default_temperature" => {
            let old = serde_json::to_value(settings.default_temperature)?;
            let parsed: f64 = raw_value.trim().parse().map_err(|_| {
                FactoryError::InvalidInput(format!(
                    "default_temperature must be a number between {MIN_TEMPERATURE:.1} and {MAX_TEMPERATURE:.1}"
                ))
            })?;
            settings.default_temperature = parsed;
            let new = serde_json::to_value(settings.default_temperature)?;
            Ok((old, new))
        }
        "default_workflow" => {
            let old = serde_json::to_value(&settings.default_workflow)?;
            settings.default_workflow = parse_string_value(raw_value.trim());
            let new = serde_json::to_value(&settings.default_workflow)?;
            Ok((old, new))
        }
        "max_review_rounds" => {
            let old = serde_json::to_value(settings.max_review_rounds)?;
            let parsed: u32 = raw_value.trim().parse().map_err(|_| {
                FactoryError::InvalidInput(format!(
                    "max_review_rounds must be an integer between 0 and {MAX_REVIEW_ROUNDS}"
                ))
            })?;
            settings.max_review_rounds = parsed;
            let new = serde_json::to_value(settings.max_review_rounds)?;
            Ok((old, new))
        }
        "approval_grace_secs" => {
            let old = serde_json::to_value(settings.approval_grace_secs)?;
            let parsed: u64 = raw_value.trim().parse().map_err(|_| {
                FactoryError::InvalidInput(format!(
                    "approval_grace_secs must be an integer between 0 and {MAX_APPROVAL_GRACE_SECS}"
                ))
            })?;
            settings.approval_grace_secs = parsed;
            let new = serde_json::to_value(settings.approval_grace_secs)?;
            Ok((old, new))
        }
        "default_budget" => {
            let old = serde_json::to_value(&settings.default_budget)?;
            let parsed: Budget = serde_json::from_str(raw_value).map_err(|e| {
                FactoryError::InvalidInput(format!("default_budget must be JSON object: {e}"))
            })?;
            settings.default_budget = parsed;
            let new = serde_json::to_value(&settings.default_budget)?;
            Ok((old, new))
        }
        "default_budget.max_cost_usd" => {
            let old = serde_json::to_value(settings.default_budget.max_cost_usd)?;
            settings.default_budget.max_cost_usd = parse_optional(raw_value)?;
            let new = serde_json::to_value(settings.default_budget.max_cost_usd)?;
            Ok((old, new))
        }
        "default_budget.max_duration_secs" => {
            let old = serde_json::to_value(settings.default_budget.max_duration_secs)?;
            settings.default_budget.max_duration_secs = parse_optional(raw_value)?;
            let new = serde_json::to_value(settings.default_budget.max_duration_secs)?;
            Ok((old, new))
        }
        "default_budget.max_retries" => {
            let old = serde_json::to_value(settings.default_budget.max_retries)?;
            settings.default_budget.max_retries = parse_optional(raw_value)?;
            let new = serde_json::to_value(settings.default_budget.max_retries)?;
            Ok((old, new))
        }
        "model_tiers" => {
            let old = serde_json::to_value(&settings.model_tiers)?;
            let parsed: BTreeMap<String, ProviderModelTiers> = serde_json::from_str(raw_value)
                .map_err(|e| {
                    FactoryError::InvalidInput(format!("model_tiers must be JSON map: {e}"))
                })?;
            settings.model_tiers = parsed;
            let new = serde_json::to_value(&settings.model_tiers)?;
            Ok((old, new))
        }
        "price_table" => {
            let old = serde_json::to_value(&settings.price_table)?;
            let parsed: BTreeMap<String, ModelPrice> =
                serde_json::from_str(raw_value).map_err(|e| {
                    FactoryError::InvalidInput(format!("price_table must be JSON map: {e}"))
                })?;
            settings.price_table = parsed;
            let new = serde_json::to_value(&settings.price_table)?;
            Ok((old, new))
        }
        _ => {
            if let Some(rest) = key.strip_prefix("model_tiers.") {
                if let Some((provider, tier)) = rest.split_once('.') {
                    let entry = settings
                        .model_tiers
                        .entry(provider.to_string())
                        .or_insert_with(|| ProviderModelTiers {
                            economy: String::new(),
                            standard: String::new(),
                            premium: String::new(),
                        });
                    let val = parse_string_value(raw_value.trim());
                    let old_val = match tier {
                        "economy" => {
                            let old = entry.economy.clone();
                            entry.economy = val.clone();
                            old
                        }
                        "standard" => {
                            let old = entry.standard.clone();
                            entry.standard = val.clone();
                            old
                        }
                        "premium" => {
                            let old = entry.premium.clone();
                            entry.premium = val.clone();
                            old
                        }
                        _ => {
                            return Err(FactoryError::InvalidInput(format!(
                                "unknown model tier \"{tier}\" (expected economy, standard, or premium)"
                            )));
                        }
                    };
                    return Ok((Value::String(old_val), Value::String(val)));
                }
                let old = serde_json::to_value(settings.model_tiers.get(rest))?;
                let parsed: ProviderModelTiers = serde_json::from_str(raw_value).map_err(|e| {
                    FactoryError::InvalidInput(format!(
                        "model_tiers.{rest} must be JSON object: {e}"
                    ))
                })?;
                settings.model_tiers.insert(rest.to_string(), parsed);
                let new = serde_json::to_value(settings.model_tiers.get(rest))?;
                return Ok((old, new));
            }
            if let Some(model) = key.strip_prefix("price_table.") {
                let old = serde_json::to_value(settings.price_table.get(model))?;
                let parsed: ModelPrice = serde_json::from_str(raw_value).map_err(|e| {
                    FactoryError::InvalidInput(format!(
                        "price_table.{model} must be JSON object: {e}"
                    ))
                })?;
                settings.price_table.insert(model.to_string(), parsed);
                let new = serde_json::to_value(settings.price_table.get(model))?;
                return Ok((old, new));
            }
            Err(FactoryError::InvalidInput(format!(
                "unknown setting key \"{key}\""
            )))
        }
    }
}

pub fn set(
    store: &FactoryStore,
    key: &str,
    value: &str,
    actor_id: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<SettingsChangeResult, ApprovalError> {
    ensure_user_caller(env)?;

    if is_protected_key(key) {
        return Err(ApprovalError::InvalidInput(format!(
            "setting \"{key}\" cannot be changed with `settings set`; use its dedicated user-only command"
        )));
    }

    store.ensure()?;
    let lock_path = store.lock_path();
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(FactoryError::Io)?;
    let _guard = exclusive_lock(&lock_file).map_err(FactoryError::Io)?;

    let mut settings = load(store)?;
    let (old_value, new_value) = apply_setting_change(&mut settings, key, value)?;
    validate(&settings)?;

    let ts = current_time();
    let actor = Actor {
        kind: ActorKind::User,
        id: actor_id.to_string(),
    };
    let ev = Event::from_data(
        ts,
        actor,
        "settings".to_string(),
        None,
        &EventData::SettingsChanged {
            key: key.to_string(),
            old_value: old_value.clone(),
            new_value: new_value.clone(),
            actor: actor_id.to_string(),
        },
    )?;
    append_locked(&store.events_path(), &[ev])?;
    save_unlocked(store, &settings)?;

    Ok(SettingsChangeResult {
        key: key.to_string(),
        old_value,
        new_value,
        actor: actor_id.to_string(),
        settings,
    })
}
