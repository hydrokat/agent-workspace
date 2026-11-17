use serde::Serialize;

// Source of truth: knowledgebase/architecture-decisions/factory-provider-terms.md.
// This table is a compiled copy; keep both synchronized whenever terms are re-verified.
pub const TERMS_RECORD_PATH: &str =
    "knowledgebase/architecture-decisions/factory-provider-terms.md";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TermsVerdict {
    Allowed,
    AllowedWithConditions,
    NotAllowed,
    Unclear,
}

impl TermsVerdict {
    pub fn allows_headless(self) -> bool {
        matches!(self, Self::Allowed | Self::AllowedWithConditions)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::AllowedWithConditions => "allowed-with-conditions",
            Self::NotAllowed => "not-allowed",
            Self::Unclear => "unclear",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthType {
    Subscription,
    ApiKey,
    Cloud,
}

impl AuthType {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "subscription" => Some(Self::Subscription),
            "api-key" => Some(Self::ApiKey),
            "cloud" => Some(Self::Cloud),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Subscription => "subscription",
            Self::ApiKey => "api-key",
            Self::Cloud => "cloud",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderTermsRow {
    pub provider: &'static str,
    pub auth_type: AuthType,
    pub verdict: TermsVerdict,
    pub conditions: &'static str,
    pub verified_on: &'static str,
    pub reverify_by: &'static str,
    pub record_ref: &'static str,
}

pub const PROVIDER_TERMS: &[ProviderTermsRow] = &[
    ProviderTermsRow {
        provider: "claude",
        auth_type: AuthType::Subscription,
        verdict: TermsVerdict::NotAllowed,
        conditions: "Consumer Terms Section 3.7 prohibit automated or non-human access using consumer subscription credentials.",
        verified_on: "2026-11-16",
        reverify_by: "2027-02-14",
        record_ref: TERMS_RECORD_PATH,
    },
    ProviderTermsRow {
        provider: "claude",
        auth_type: AuthType::ApiKey,
        verdict: TermsVerdict::AllowedWithConditions,
        conditions: "Commercial Terms apply; user's own API key; unmodified official claude binary via argv with documented flags; no credential proxying.",
        verified_on: "2026-11-16",
        reverify_by: "2027-02-14",
        record_ref: TERMS_RECORD_PATH,
    },
    ProviderTermsRow {
        provider: "claude",
        auth_type: AuthType::Cloud,
        verdict: TermsVerdict::AllowedWithConditions,
        conditions: "Commercial/Cloud Terms (Bedrock/Vertex) apply; user's own cloud inference credential; unmodified official claude binary via argv.",
        verified_on: "2026-11-16",
        reverify_by: "2027-02-14",
        record_ref: TERMS_RECORD_PATH,
    },
    ProviderTermsRow {
        provider: "agy",
        auth_type: AuthType::Subscription,
        verdict: TermsVerdict::Unclear,
        conditions: "Antigravity Additional Terms prohibit third-party tools accessing the Service; forum clarification for local agy --print remains open.",
        verified_on: "2026-11-16",
        reverify_by: "2027-02-14",
        record_ref: TERMS_RECORD_PATH,
    },
    ProviderTermsRow {
        provider: "agy",
        auth_type: AuthType::ApiKey,
        verdict: TermsVerdict::AllowedWithConditions,
        conditions: "User's own Google AI Studio API key; unmodified official agy binary via argv; no OAuth interception or quota bypass.",
        verified_on: "2026-11-16",
        reverify_by: "2027-02-14",
        record_ref: TERMS_RECORD_PATH,
    },
    ProviderTermsRow {
        provider: "agy",
        auth_type: AuthType::Cloud,
        verdict: TermsVerdict::AllowedWithConditions,
        conditions: "User's own Vertex AI project credentials; unmodified official agy binary via argv; per-provider user consent recorded.",
        verified_on: "2026-11-16",
        reverify_by: "2027-02-14",
        record_ref: TERMS_RECORD_PATH,
    },
    ProviderTermsRow {
        provider: "opencode",
        auth_type: AuthType::Subscription,
        verdict: TermsVerdict::Unclear,
        conditions: "No verified headless terms row recorded yet.",
        verified_on: "2026-11-16",
        reverify_by: "2027-02-14",
        record_ref: TERMS_RECORD_PATH,
    },
];

pub fn lookup(provider: &str, auth_type: AuthType) -> Option<&'static ProviderTermsRow> {
    PROVIDER_TERMS
        .iter()
        .find(|row| row.provider == provider && row.auth_type == auth_type)
}
