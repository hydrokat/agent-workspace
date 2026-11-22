use serde::Serialize;

use super::approval::{ApprovalError, Caller, require_user};
use super::broker::terms::{AuthType, PROVIDER_TERMS, ProviderTermsRow, TermsVerdict};
use super::events::{Event, append_locked, read_all, redact_event};
use super::login::{LoginProbe, LoginStatusItem};
use super::machine::EventData;
use super::model::{Actor, ActorKind};
use super::redact::RedactContext;
use super::store::{FactoryStore, current_time};

pub const KNOWN_CLIS: &[&str] = &["claude", "agy", "opencode"];
pub const UNKNOWN_METHOD: &str = "unknown";
const ACK_ENTITY: &str = "auth";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthRisk {
    Ok {
        verdict: TermsVerdict,
    },
    Risky {
        verdict: Option<TermsVerdict>,
        reason: String,
        conditions: Option<String>,
    },
    Acknowledged {
        verdict: Option<TermsVerdict>,
        acknowledged_on: String,
    },
}

impl AuthRisk {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Ok { .. } => "ok",
            Self::Risky { .. } => "risky",
            Self::Acknowledged { .. } => "acknowledged",
        }
    }

    pub fn blocks_launch(&self) -> bool {
        matches!(self, Self::Risky { .. })
    }

    pub fn verdict(&self) -> Option<TermsVerdict> {
        match self {
            Self::Ok { verdict } => Some(*verdict),
            Self::Risky { verdict, .. } | Self::Acknowledged { verdict, .. } => *verdict,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Acknowledgement {
    pub cli: String,
    pub method: String,
    pub verdict: String,
    pub terms_verified_on: String,
    pub acknowledged_by: String,
    pub acknowledged_at: f64,
}

pub fn method_name(method: Option<AuthType>) -> &'static str {
    method.map_or(UNKNOWN_METHOD, AuthType::as_str)
}

pub fn method_from_login(_cli: &str, reported: Option<&str>) -> Option<AuthType> {
    let raw = reported?.trim().to_ascii_lowercase();
    if raw.is_empty() {
        return None;
    }
    if raw == "subscription" || raw == "claude.ai" || raw == "oauth" {
        return Some(AuthType::Subscription);
    }
    if raw.contains("bedrock") || raw.contains("vertex") || raw == "cloud" {
        return Some(AuthType::Cloud);
    }
    if raw.contains("api") {
        return Some(AuthType::ApiKey);
    }
    None
}

pub fn assess(
    cli: &str,
    method: Option<AuthType>,
    today: &str,
    terms: &[ProviderTermsRow],
    acks: &[Acknowledgement],
) -> AuthRisk {
    let row = method.and_then(|m| terms.iter().find(|r| r.provider == cli && r.auth_type == m));
    let method_str = method_name(method);

    let risk_reason = match (method, row) {
        (None, _) => Some(format!("the {cli} sign-in method could not be detected")),
        (Some(m), None) => Some(format!(
            "no verified terms for {cli} with {} sign-in",
            m.as_str()
        )),
        (Some(_), Some(r)) if today > r.reverify_by => Some(format!(
            "the terms verdict for {cli} expired on {}; re-verify first",
            r.reverify_by
        )),
        (Some(_), Some(r)) if r.verdict.needs_acknowledgement() => Some(format!(
            "the terms verdict for {cli} with {method_str} sign-in is {}",
            r.verdict.as_str()
        )),
        (Some(_), Some(_)) => None,
    };

    let verdict = row.map(|r| r.verdict);
    let Some(reason) = risk_reason else {
        return AuthRisk::Ok {
            verdict: verdict.unwrap_or(TermsVerdict::Allowed),
        };
    };

    let current_terms_date = row.map_or("", |r| r.verified_on);
    let still_in_date = row.is_none_or(|r| today <= r.reverify_by);
    if still_in_date
        && let Some(ack) = acks.iter().find(|a| {
            a.cli == cli && a.method == method_str && a.terms_verified_on == current_terms_date
        })
    {
        return AuthRisk::Acknowledged {
            verdict,
            acknowledged_on: iso_date_of(ack.acknowledged_at),
        };
    }

    AuthRisk::Risky {
        verdict,
        reason,
        conditions: row.map(|r| r.conditions.to_string()),
    }
}

pub fn current_acks(events: &[Event]) -> Vec<Acknowledgement> {
    let mut out: Vec<Acknowledgement> = Vec::new();
    for ev in events
        .iter()
        .filter(|e| e.r#type == "auth.risk_acknowledged")
    {
        let field = |name: &str| {
            ev.data
                .get(name)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let ack = Acknowledgement {
            cli: field("cli"),
            method: field("method"),
            verdict: field("verdict"),
            terms_verified_on: field("terms_verified_on"),
            acknowledged_by: field("acknowledged_by"),
            acknowledged_at: ev.ts,
        };
        out.retain(|a| !(a.cli == ack.cli && a.method == ack.method));
        out.push(ack);
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub fn acknowledge_with_terms(
    store: &FactoryStore,
    cli: &str,
    detected: Option<AuthType>,
    requested_method: &str,
    confirm: &str,
    acknowledged_by: &str,
    caller: &Caller<'_>,
    terms: &[ProviderTermsRow],
) -> Result<Acknowledgement, ApprovalError> {
    require_user(caller)?;

    if !KNOWN_CLIS.contains(&cli) {
        return Err(ApprovalError::InvalidInput(format!("unknown CLI: {cli}")));
    }
    if confirm != cli {
        return Err(ApprovalError::InvalidInput(format!(
            "confirmation mismatch: pass --confirm {cli}"
        )));
    }
    let detected_name = method_name(detected);
    if requested_method != detected_name {
        return Err(ApprovalError::InvalidInput(format!(
            "{cli} is signed in with {detected_name}, not {requested_method}; pass --method {detected_name}"
        )));
    }

    let row = detected.and_then(|m| terms.iter().find(|r| r.provider == cli && r.auth_type == m));
    let verdict = row.map_or("none", |r| r.verdict.as_str());
    let terms_verified_on = row.map_or("", |r| r.verified_on);

    store.ensure()?;
    let ts = current_time();
    let actor = Actor {
        kind: ActorKind::User,
        id: acknowledged_by.to_string(),
    };
    let data = EventData::AuthRiskAcknowledged {
        cli: cli.to_string(),
        method: detected_name.to_string(),
        verdict: verdict.to_string(),
        terms_verified_on: terms_verified_on.to_string(),
        acknowledged_by: acknowledged_by.to_string(),
    };
    let ev = Event::from_data(ts, actor, ACK_ENTITY.to_string(), None, &data)?;
    append_locked(
        &store.events_path(),
        &[redact_event(ev, &RedactContext::from_env())],
    )?;

    Ok(Acknowledgement {
        cli: cli.to_string(),
        method: detected_name.to_string(),
        verdict: verdict.to_string(),
        terms_verified_on: terms_verified_on.to_string(),
        acknowledged_by: acknowledged_by.to_string(),
        acknowledged_at: ts,
    })
}

pub fn acknowledge(
    store: &FactoryStore,
    cli: &str,
    detected: Option<AuthType>,
    requested_method: &str,
    confirm: &str,
    acknowledged_by: &str,
    caller: &Caller<'_>,
) -> Result<Acknowledgement, ApprovalError> {
    acknowledge_with_terms(
        store,
        cli,
        detected,
        requested_method,
        confirm,
        acknowledged_by,
        caller,
        PROVIDER_TERMS,
    )
}

fn iso_date_of(ts: f64) -> String {
    let days = (ts / 86_400.0).floor() as i64;
    let (y, m, d) = super::dates::civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RiskItem {
    pub cli: String,
    pub signed_in: bool,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acknowledged_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reverify_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acknowledge_command: Option<String>,
}

pub fn detected_method(item: &LoginStatusItem) -> Option<AuthType> {
    method_from_login(&item.cli, item.method.as_deref())
}

pub fn risk_item(
    item: &LoginStatusItem,
    acks: &[Acknowledgement],
    today: &str,
    terms: &[ProviderTermsRow],
) -> RiskItem {
    let method = detected_method(item);
    let mut out = RiskItem {
        cli: item.cli.clone(),
        signed_in: item.installed && item.signed_in,
        method: method_name(method).to_string(),
        verdict: None,
        risk: None,
        reason: None,
        conditions: None,
        acknowledged_on: None,
        verified_on: None,
        reverify_by: None,
        acknowledge_command: None,
    };
    if !out.signed_in {
        return out;
    }
    let row = method.and_then(|m| {
        terms
            .iter()
            .find(|r| r.provider == item.cli && r.auth_type == m)
    });
    out.verified_on = row.map(|r| r.verified_on.to_string());
    out.reverify_by = row.map(|r| r.reverify_by.to_string());
    let risk = assess(&item.cli, method, today, terms, acks);
    out.risk = Some(risk.label().to_string());
    out.verdict = risk.verdict().map(|v| v.as_str().to_string());
    match risk {
        AuthRisk::Ok { .. } => {}
        AuthRisk::Risky {
            reason, conditions, ..
        } => {
            out.reason = Some(reason);
            out.conditions = conditions;
            out.acknowledge_command = Some(acknowledge_command(&item.cli, &out.method));
        }
        AuthRisk::Acknowledged {
            acknowledged_on, ..
        } => {
            out.acknowledged_on = Some(acknowledged_on);
        }
    }
    out
}

pub fn acknowledge_command(cli: &str, method: &str) -> String {
    format!("tandem factory login acknowledge-risk {cli} --method {method} --confirm {cli}")
}

pub fn read_acks(store: &FactoryStore) -> Vec<Acknowledgement> {
    if !store.exists() {
        return Vec::new();
    }
    read_all(&store.events_path())
        .map(|r| current_acks(&r.events))
        .unwrap_or_default()
}

pub fn risk_report(
    store: &FactoryStore,
    items: &[LoginStatusItem],
    today: &str,
    terms: &[ProviderTermsRow],
) -> Vec<RiskItem> {
    let acks = read_acks(store);
    items
        .iter()
        .map(|item| risk_item(item, &acks, today, terms))
        .collect()
}

pub fn assess_cli(store: &FactoryStore, cli: &str, probe: &dyn LoginProbe) -> AuthRisk {
    let item = super::login::check_status(cli, probe);
    assess(
        cli,
        detected_method(&item),
        &super::dates::today_iso_date(),
        PROVIDER_TERMS,
        &read_acks(store),
    )
}
