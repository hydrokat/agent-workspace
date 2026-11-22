use std::fs::OpenOptions;

use serde::Serialize;

pub use super::terms::{AuthType, ProviderTermsRow};
use super::terms::{PROVIDER_TERMS, TermsVerdict, lookup};
use crate::coordination::filelock::exclusive_lock;
use crate::factory::FactoryError;
use crate::factory::approval::{ApprovalError, Caller, require_user};
pub use crate::factory::dates::today_iso_date;
use crate::factory::events::{Event, append_locked, redact_event, set_private_file_mode};
use crate::factory::machine::EventData;
use crate::factory::model::{
    Actor, ActorKind, FactorySettings, KNOWN_PROVIDERS, ProviderGate, ProviderGateState,
};
use crate::factory::redact::RedactContext;
use crate::factory::store::{FactoryStore, current_time};

#[derive(Debug)]
pub struct GatePass {
    _sealed: (),
    provider: String,
    auth_type: AuthType,
}

impl GatePass {
    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn auth_type(&self) -> AuthType {
        self.auth_type
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GateDenied {
    #[error("provider {provider} has no consent recorded in this workspace")]
    NoConsent { provider: String },
    #[error("provider {provider} ({auth_type}) terms verdict is {verdict}: {reason}")]
    TermsDisallowed {
        provider: String,
        auth_type: String,
        verdict: String,
        reason: String,
    },
    #[error(
        "provider {provider} terms expired on {reverify_by} (today is {today}); re-verify terms"
    )]
    TermsExpired {
        provider: String,
        reverify_by: String,
        today: String,
    },
    #[error(
        "provider {provider} consent was recorded against {consented_on}, but terms were verified on {verified_on}; re-consent required"
    )]
    ConsentStale {
        provider: String,
        consented_on: String,
        verified_on: String,
    },
    #[error("provider {provider} gate is closed in factory settings")]
    GateClosed { provider: String },
}

pub fn check(
    provider: &str,
    settings: &FactorySettings,
    today: &str,
) -> Result<GatePass, GateDenied> {
    check_with_terms(provider, settings, today, PROVIDER_TERMS)
}

pub fn check_with_terms(
    provider: &str,
    settings: &FactorySettings,
    today: &str,
    terms: &[ProviderTermsRow],
) -> Result<GatePass, GateDenied> {
    let gate = settings
        .provider_gates
        .iter()
        .find(|g| g.provider == provider)
        .ok_or_else(|| GateDenied::NoConsent {
            provider: provider.to_string(),
        })?;

    let auth_str = gate
        .auth_type
        .as_deref()
        .ok_or_else(|| GateDenied::NoConsent {
            provider: provider.to_string(),
        })?;
    let auth_type = AuthType::parse(auth_str).ok_or_else(|| GateDenied::TermsDisallowed {
        provider: provider.to_string(),
        auth_type: auth_str.to_string(),
        verdict: "unknown".to_string(),
        reason: "unknown authentication type".to_string(),
    })?;

    let row = terms
        .iter()
        .find(|r| r.provider == provider && r.auth_type == auth_type)
        .ok_or_else(|| GateDenied::TermsDisallowed {
            provider: provider.to_string(),
            auth_type: auth_type.as_str().to_string(),
            verdict: "unclear".to_string(),
            reason: "no verified terms row for provider and auth type".to_string(),
        })?;

    if !row.verdict.allows_headless() {
        return Err(GateDenied::TermsDisallowed {
            provider: provider.to_string(),
            auth_type: auth_type.as_str().to_string(),
            verdict: row.verdict.as_str().to_string(),
            reason: row.conditions.to_string(),
        });
    }

    if today > row.reverify_by {
        return Err(GateDenied::TermsExpired {
            provider: provider.to_string(),
            reverify_by: row.reverify_by.to_string(),
            today: today.to_string(),
        });
    }

    if gate.consented_at.is_none() {
        return Err(GateDenied::NoConsent {
            provider: provider.to_string(),
        });
    }

    let consented_on = gate.verified_on.as_deref().unwrap_or("");
    if consented_on != row.verified_on {
        return Err(GateDenied::ConsentStale {
            provider: provider.to_string(),
            consented_on: consented_on.to_string(),
            verified_on: row.verified_on.to_string(),
        });
    }

    if gate.state != ProviderGateState::Open {
        return Err(GateDenied::GateClosed {
            provider: provider.to_string(),
        });
    }

    Ok(GatePass {
        _sealed: (),
        provider: provider.to_string(),
        auth_type,
    })
}

pub fn consent_provider(
    store: &FactoryStore,
    provider: &str,
    auth_type: AuthType,
    confirm: &str,
    consented_by: &str,
    caller: &Caller<'_>,
) -> Result<ProviderGate, ApprovalError> {
    consent_provider_with_terms(
        store,
        provider,
        auth_type,
        confirm,
        consented_by,
        caller,
        &today_iso_date(),
        PROVIDER_TERMS,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn consent_provider_with_terms(
    store: &FactoryStore,
    provider: &str,
    auth_type: AuthType,
    confirm: &str,
    consented_by: &str,
    caller: &Caller<'_>,
    today: &str,
    terms: &[ProviderTermsRow],
) -> Result<ProviderGate, ApprovalError> {
    require_user(caller)?;

    if confirm != provider {
        return Err(ApprovalError::InvalidInput(format!(
            "confirmation mismatch: pass --confirm {provider}"
        )));
    }
    if !KNOWN_PROVIDERS.contains(&provider) {
        return Err(ApprovalError::InvalidInput(format!(
            "unknown provider: {provider}"
        )));
    }

    let row = terms
        .iter()
        .find(|r| r.provider == provider && r.auth_type == auth_type)
        .ok_or_else(|| {
            ApprovalError::Gate(format!(
                "Cannot open the {provider} gate: no verified terms row for {}.",
                auth_type.as_str()
            ))
        })?;

    if !row.verdict.allows_headless() {
        return Err(ApprovalError::Gate(format!(
            "Cannot open the {provider} gate: the terms verdict is \"{}\".",
            row.verdict.as_str()
        )));
    }

    if today > row.reverify_by {
        return Err(ApprovalError::Gate(format!(
            "Cannot open the {provider} gate: terms expired on {}; re-verify first.",
            row.reverify_by
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
        .map_err(FactoryError::from)?;
    set_private_file_mode(&lock_path)?;
    let _guard = exclusive_lock(&lock_file).map_err(FactoryError::from)?;

    let mut settings = store.settings()?;
    let ts = current_time();
    let gate = ProviderGate {
        provider: provider.to_string(),
        state: ProviderGateState::Open,
        auth_type: Some(auth_type.as_str().to_string()),
        verified_on: Some(row.verified_on.to_string()),
        terms_record: Some(row.record_ref.to_string()),
        consented_at: Some(ts),
        consented_by: Some(consented_by.to_string()),
    };

    if let Some(existing) = settings
        .provider_gates
        .iter_mut()
        .find(|g| g.provider == provider)
    {
        *existing = gate.clone();
    } else {
        settings.provider_gates.push(gate.clone());
    }

    let actor = Actor {
        kind: ActorKind::User,
        id: consented_by.to_string(),
    };
    let ev = Event::from_data(
        ts,
        actor,
        "provider".to_string(),
        None,
        &EventData::ProviderConsented {
            provider: provider.to_string(),
            auth_type: auth_type.as_str().to_string(),
            verified_on: row.verified_on.to_string(),
            terms_record: row.record_ref.to_string(),
            consented_by: consented_by.to_string(),
        },
    )?;
    append_locked(
        &store.events_path(),
        &[redact_event(ev, &RedactContext::from_env())],
    )?;
    store.save_settings(&settings)?;

    Ok(gate)
}

pub fn revoke_provider(
    store: &FactoryStore,
    provider: &str,
    revoked_by: &str,
    caller: &Caller<'_>,
) -> Result<ProviderGate, ApprovalError> {
    require_user(caller)?;

    if !KNOWN_PROVIDERS.contains(&provider) {
        return Err(ApprovalError::InvalidInput(format!(
            "unknown provider: {provider}"
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
        .map_err(FactoryError::from)?;
    set_private_file_mode(&lock_path)?;
    let _guard = exclusive_lock(&lock_file).map_err(FactoryError::from)?;

    let mut settings = store.settings()?;
    let ts = current_time();
    let updated_gate = if let Some(existing) = settings
        .provider_gates
        .iter_mut()
        .find(|g| g.provider == provider)
    {
        existing.state = ProviderGateState::Closed;
        existing.clone()
    } else {
        let g = ProviderGate {
            provider: provider.to_string(),
            state: ProviderGateState::Closed,
            auth_type: None,
            verified_on: None,
            terms_record: None,
            consented_at: None,
            consented_by: None,
        };
        settings.provider_gates.push(g.clone());
        g
    };

    let actor = Actor {
        kind: ActorKind::User,
        id: revoked_by.to_string(),
    };
    let ev = Event::from_data(
        ts,
        actor,
        "provider".to_string(),
        None,
        &EventData::ProviderRevoked {
            provider: provider.to_string(),
        },
    )?;
    append_locked(
        &store.events_path(),
        &[redact_event(ev, &RedactContext::from_env())],
    )?;
    store.save_settings(&settings)?;

    Ok(updated_gate)
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderStatusItem {
    pub provider: String,
    pub auth_type: String,
    pub verdict: TermsVerdict,
    pub verified_on: String,
    pub reverify_by: String,
    pub gate: ProviderGateState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

pub fn provider_statuses(settings: &FactorySettings, today: &str) -> Vec<ProviderStatusItem> {
    let mut out = Vec::new();
    for provider in KNOWN_PROVIDERS {
        let gate = settings
            .provider_gates
            .iter()
            .find(|g| g.provider == *provider);
        let auth_str = gate
            .and_then(|g| g.auth_type.as_deref())
            .unwrap_or("subscription");
        let auth_type = AuthType::parse(auth_str).unwrap_or(AuthType::Subscription);
        let row = lookup(provider, auth_type)
            .or_else(|| PROVIDER_TERMS.iter().find(|r| r.provider == *provider));
        let (verdict, verified_on, reverify_by) = match row {
            Some(r) => (r.verdict, r.verified_on, r.reverify_by),
            None => (TermsVerdict::Unclear, "", ""),
        };
        let reason = match check(provider, settings, today) {
            Ok(_) => None,
            Err(err) => Some(err.to_string()),
        };
        out.push(ProviderStatusItem {
            provider: (*provider).to_string(),
            auth_type: auth_str.to_string(),
            verdict,
            verified_on: verified_on.to_string(),
            reverify_by: reverify_by.to_string(),
            gate: gate.map(|g| g.state).unwrap_or(ProviderGateState::Closed),
            reason,
        });
    }
    out
}
