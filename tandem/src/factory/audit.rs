use std::collections::HashMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};

use super::FactoryError;
use super::broker::job::{JobInput, job_dir};
use super::events::{Event, read_all, reject_if_symlink, sha256_hex};
use super::ids::is_valid_idea_id;
use super::machine::EventData;
use super::model::{ActorKind, Budget};
use super::redact::{RedactContext, redact};
use super::store::FactoryStore;
use super::telemetry::{Cost, CostBasis};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditKind {
    JobStart,
    JobEnd,
    Approval,
    Decision,
    Lifecycle,
}

impl AuditKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::JobStart => "job_start",
            Self::JobEnd => "job_end",
            Self::Approval => "approval",
            Self::Decision => "decision",
            Self::Lifecycle => "lifecycle",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::JobStart => "Job start",
            Self::JobEnd => "Job end",
            Self::Approval => "Approval",
            Self::Decision => "Decision",
            Self::Lifecycle => "Lifecycle",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "job_start" | "job-start" | "start" => Some(Self::JobStart),
            "job_end" | "job-end" | "end" => Some(Self::JobEnd),
            "approval" | "approvals" => Some(Self::Approval),
            "decision" | "decisions" => Some(Self::Decision),
            "lifecycle" => Some(Self::Lifecycle),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditDetailItem {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub seq: u64,
    pub ts: f64,
    pub actor: String,
    pub actor_kind: ActorKind,
    pub kind: AuditKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    pub summary: String,
    pub details: Vec<AuditDetailItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditFilter {
    pub run: Option<String>,
    pub kind: Option<AuditKind>,
    pub actor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditBreak {
    pub seq: u64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditVerifyReport {
    pub total_events: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain_start_seq: Option<u64>,
    pub verified_events: usize,
    pub intact: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_break: Option<AuditBreak>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditPayload {
    pub entries: Vec<AuditEntry>,
    pub verify: AuditVerifyReport,
}

pub fn verify(events_path: &Path) -> Result<AuditVerifyReport, FactoryError> {
    reject_if_symlink(events_path)?;

    let bytes = match fs::read(events_path) {
        Ok(b) => b,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AuditVerifyReport {
                total_events: 0,
                chain_start_seq: None,
                verified_events: 0,
                intact: true,
                first_break: None,
            });
        }
        Err(err) => return Err(FactoryError::Io(err)),
    };

    if bytes.is_empty() {
        return Ok(AuditVerifyReport {
            total_events: 0,
            chain_start_seq: None,
            verified_events: 0,
            intact: true,
            first_break: None,
        });
    }

    let valid_bytes = if bytes.ends_with(b"\n") {
        bytes.as_slice()
    } else {
        let cut = bytes
            .iter()
            .rposition(|&b| b == b'\n')
            .map(|idx| idx + 1)
            .unwrap_or(0);
        &bytes[..cut]
    };

    let mut total_events = 0usize;
    let mut chain_start_seq: Option<u64> = None;
    let mut verified_events = 0usize;
    let mut first_break: Option<AuditBreak> = None;
    let mut prev_line_hash: Option<String> = None;

    for raw_line in valid_bytes.split(|&b| b == b'\n') {
        let trimmed = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        if trimmed.iter().all(|b| b.is_ascii_whitespace()) {
            continue;
        }

        let current_hash = sha256_hex(trimmed);
        match serde_json::from_slice::<Event>(trimmed) {
            Ok(ev) => {
                total_events += 1;
                if let Some(ref prev) = ev.prev {
                    if chain_start_seq.is_none() {
                        chain_start_seq = Some(ev.seq);
                    }
                    match prev_line_hash.as_deref() {
                        Some(expected) if expected == prev => {
                            if first_break.is_none() {
                                verified_events += 1;
                            }
                        }
                        Some(_) => {
                            if first_break.is_none() {
                                first_break = Some(AuditBreak {
                                    seq: ev.seq,
                                    reason: "prev hash mismatch".to_string(),
                                });
                            }
                        }
                        None => {
                            if first_break.is_none() {
                                first_break = Some(AuditBreak {
                                    seq: ev.seq,
                                    reason: "prev hash present on first event".to_string(),
                                });
                            }
                        }
                    }
                } else if chain_start_seq.is_some() && first_break.is_none() {
                    first_break = Some(AuditBreak {
                        seq: ev.seq,
                        reason: "missing prev hash after chain start".to_string(),
                    });
                }
            }
            Err(_) => {
                if chain_start_seq.is_some() && first_break.is_none() {
                    first_break = Some(AuditBreak {
                        seq: total_events as u64 + 1,
                        reason: "corrupt event line".to_string(),
                    });
                }
            }
        }
        prev_line_hash = Some(current_hash);
    }

    Ok(AuditVerifyReport {
        total_events,
        chain_start_seq,
        verified_events,
        intact: first_break.is_none(),
        first_break,
    })
}

pub fn format_verify_summary(report: &AuditVerifyReport) -> String {
    match (report.chain_start_seq, &report.first_break) {
        (None, None) if report.total_events == 0 => "No events recorded yet.".to_string(),
        (None, None) => format!(
            "No hash chain started yet ({} legacy events).",
            report.total_events
        ),
        (Some(start), None) => format!(
            "Chain starts at event {}. {} events verified. No break found.",
            start, report.verified_events
        ),
        (Some(start), Some(brk)) => format!(
            "Chain break at event {} ({}); chain started at event {}, {} events verified before break.",
            brk.seq, brk.reason, start, report.verified_events
        ),
        (None, Some(brk)) => format!("Chain break at event {} ({}).", brk.seq, brk.reason),
    }
}

fn redact_text(raw: &str, ctx: &RedactContext) -> String {
    redact(raw, ctx).into_string()
}

fn detail(
    label: impl Into<String>,
    value: impl Into<String>,
    ctx: &RedactContext,
) -> AuditDetailItem {
    AuditDetailItem {
        label: label.into(),
        value: redact_text(&value.into(), ctx),
    }
}

fn format_tool_grant(g: &super::permissions::ToolGrant) -> String {
    let names: Vec<&str> = g.tools.iter().map(|t| t.as_str()).collect();
    let mode = if g.read_only {
        "read-only"
    } else {
        "read-write"
    };
    format!("{} ({mode})", names.join(", "))
}

fn format_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.0} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

pub fn format_duration(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    let mins = total / 60;
    let rem = total % 60;
    if mins > 0 {
        format!("{mins}m {rem}s")
    } else {
        format!("{rem}s")
    }
}

fn format_cost_with_basis(cost: &Cost) -> String {
    match (cost.basis, cost.value) {
        (CostBasis::Actual, Some(v)) if v.is_finite() => format!("${v:.2} (actual)"),
        (CostBasis::Estimated, Some(v)) if v.is_finite() => {
            format!("about ${v:.2} (estimated)")
        }
        _ => "unknown (unknown)".to_string(),
    }
}

fn format_budget(b: &Budget) -> String {
    let mut parts = Vec::new();
    if let Some(v) = b.max_cost_usd {
        parts.push(format!("cost=${v:.2}"));
    }
    if let Some(v) = b.max_duration_secs {
        parts.push(format!("duration={v}s"));
    }
    if let Some(v) = b.max_retries {
        parts.push(format!("retries={v}"));
    }
    if parts.is_empty() {
        "unlimited".to_string()
    } else {
        parts.join(", ")
    }
}

fn load_job_meta(
    store: &FactoryStore,
    run_id: &str,
    stage: &str,
    attempt: u32,
) -> (Option<JobInput>, usize) {
    let dir = job_dir(store, run_id, stage, attempt);
    let input_path = dir.join("input.json");
    let input = if reject_if_symlink(&input_path).is_ok() {
        fs::read(&input_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<JobInput>(&bytes).ok())
    } else {
        None
    };

    let prompt_path = dir.join("prompt.md");
    let redactions = if reject_if_symlink(&prompt_path).is_ok() {
        fs::read_to_string(&prompt_path)
            .map(|text| text.matches("«redacted:").count())
            .unwrap_or(0)
    } else {
        0
    };

    (input, redactions)
}

#[derive(Debug, Clone)]
struct UsageMeta {
    provider: String,
    model: String,
    tier: String,
    duration_secs: f64,
    cost: Cost,
}

pub fn entries(
    store: &FactoryStore,
    idea_id: &str,
    filter: &AuditFilter,
) -> Result<Vec<AuditEntry>, FactoryError> {
    if !is_valid_idea_id(idea_id) {
        return Err(FactoryError::InvalidInput(format!(
            "invalid idea id: {idea_id}"
        )));
    }

    let read = read_all(&store.events_path())?;
    let ctx = RedactContext::from_env();

    let mut run_workflows: HashMap<String, String> = HashMap::new();
    let mut active_attempt: HashMap<(String, String), u32> = HashMap::new();
    let mut job_usages: HashMap<(String, String, u32), UsageMeta> = HashMap::new();
    let mut plan_versions: HashMap<String, u32> = HashMap::new();

    for ev in &read.events {
        if ev.idea != idea_id {
            continue;
        }
        if let Ok(data) = ev.to_event_data() {
            match data {
                EventData::RunCreated {
                    run,
                    workflow,
                    version,
                    ..
                } => {
                    let wf = if version.is_empty() {
                        workflow
                    } else if version.starts_with('v') {
                        format!("{workflow} {version}")
                    } else {
                        format!("{workflow} v{version}")
                    };
                    run_workflows.insert(run, wf);
                }
                EventData::JobUsage {
                    run,
                    stage,
                    attempt,
                    provider,
                    model,
                    tier,
                    duration_secs,
                    cost,
                    ..
                } => {
                    job_usages.insert(
                        (run, stage, attempt),
                        UsageMeta {
                            provider,
                            model,
                            tier,
                            duration_secs,
                            cost,
                        },
                    );
                }
                EventData::ArtifactStored { kind, artifact, .. } if kind == "plan" => {
                    plan_versions.insert(artifact.path.clone(), artifact.version);
                }
                _ => {}
            }
        }
    }

    let mut out = Vec::new();
    for ev in &read.events {
        let is_idea_event = ev.idea == idea_id;
        let is_workspace_decision = matches!(ev.idea.as_str(), "settings" | "provider");
        if !is_idea_event && !is_workspace_decision {
            continue;
        }

        let Ok(data) = ev.to_event_data() else {
            continue;
        };

        let actor_str = redact_text(&ev.actor.id, &ctx);
        let mut entry: Option<AuditEntry> = None;

        match data {
            EventData::StageStarted {
                run,
                stage,
                attempt,
                grant,
            } => {
                active_attempt.insert((run.clone(), stage.clone()), attempt);
                let (job_input, redactions) = load_job_meta(store, &run, &stage, attempt);
                let usage = job_usages.get(&(run.clone(), stage.clone(), attempt));

                let provider = job_input
                    .as_ref()
                    .map(|j| j.provider.clone())
                    .or_else(|| usage.map(|u| u.provider.clone()))
                    .unwrap_or_else(|| "unknown".to_string());
                let model = job_input
                    .as_ref()
                    .map(|j| j.model.clone())
                    .or_else(|| usage.map(|u| u.model.clone()))
                    .unwrap_or_else(|| "unknown".to_string());
                let tier = usage
                    .map(|u| u.tier.clone())
                    .unwrap_or_else(|| "standard".to_string());

                let mut details = Vec::new();
                if let Some(wf) = run_workflows.get(&run) {
                    details.push(detail("workflow", wf, &ctx));
                }
                details.push(detail(
                    "model",
                    format!("{provider} / {model} ({tier})"),
                    &ctx,
                ));

                let tools = if let Some(ref g) = grant {
                    format_tool_grant(g)
                } else if let Some(ref ji) = job_input {
                    if let Some(ref g) = ji.context_manifest.tool_grant {
                        format_tool_grant(g)
                    } else if !ji.tool_grants.is_empty() {
                        ji.tool_grants.join(", ")
                    } else {
                        "none".to_string()
                    }
                } else {
                    "none".to_string()
                };
                details.push(detail("tools", tools, &ctx));

                if let Some(ref ji) = job_input {
                    let files = ji.context_manifest.items.len();
                    let bytes = format_bytes(ji.context_manifest.total_bytes);
                    details.push(detail(
                        "context",
                        format!("{files} files · {bytes} · {redactions} redactions"),
                        &ctx,
                    ));
                }

                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: actor_str.clone(),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::JobStart,
                    run: Some(run.clone()),
                    stage: Some(stage.clone()),
                    attempt: Some(attempt),
                    correlation_id: Some(format!("{run}.{stage}.{attempt}")),
                    summary: redact_text(
                        &format!("{stage} started · {provider} / {model} ({tier})"),
                        &ctx,
                    ),
                    details,
                });
            }
            EventData::StageSucceeded { run, stage } => {
                let attempt = active_attempt
                    .get(&(run.clone(), stage.clone()))
                    .copied()
                    .unwrap_or(1);
                let usage = job_usages.get(&(run.clone(), stage.clone(), attempt));
                let mut details = vec![detail("result", "succeeded", &ctx)];
                if let Some(u) = usage {
                    details.push(detail(
                        "cost",
                        format!(
                            "{} · {}",
                            format_cost_with_basis(&u.cost),
                            format_duration(u.duration_secs)
                        ),
                        &ctx,
                    ));
                }
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: actor_str.clone(),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::JobEnd,
                    run: Some(run.clone()),
                    stage: Some(stage.clone()),
                    attempt: Some(attempt),
                    correlation_id: Some(format!("{run}.{stage}.{attempt}")),
                    summary: redact_text(&format!("{stage} succeeded"), &ctx),
                    details,
                });
            }
            EventData::StageFailed { run, stage, reason } => {
                let attempt = active_attempt
                    .get(&(run.clone(), stage.clone()))
                    .copied()
                    .unwrap_or(1);
                let usage = job_usages.get(&(run.clone(), stage.clone(), attempt));
                let mut details = vec![detail("result", format!("failed: {reason}"), &ctx)];
                if let Some(u) = usage {
                    details.push(detail(
                        "cost",
                        format!(
                            "{} · {}",
                            format_cost_with_basis(&u.cost),
                            format_duration(u.duration_secs)
                        ),
                        &ctx,
                    ));
                }
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: actor_str.clone(),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::JobEnd,
                    run: Some(run.clone()),
                    stage: Some(stage.clone()),
                    attempt: Some(attempt),
                    correlation_id: Some(format!("{run}.{stage}.{attempt}")),
                    summary: redact_text(&format!("{stage} failed: {reason}"), &ctx),
                    details,
                });
            }
            EventData::StageBlocked { run, stage, reason } => {
                let attempt = active_attempt
                    .get(&(run.clone(), stage.clone()))
                    .copied()
                    .unwrap_or(1);
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: actor_str.clone(),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::JobEnd,
                    run: Some(run.clone()),
                    stage: Some(stage.clone()),
                    attempt: Some(attempt),
                    correlation_id: Some(format!("{run}.{stage}.{attempt}")),
                    summary: redact_text(&format!("{stage} blocked: {reason}"), &ctx),
                    details: vec![detail("result", format!("blocked: {reason}"), &ctx)],
                });
            }
            EventData::StageSkipped { run, stage, reason } => {
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: actor_str.clone(),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::JobEnd,
                    run: Some(run),
                    stage: Some(stage.clone()),
                    attempt: None,
                    correlation_id: None,
                    summary: redact_text(&format!("{stage} skipped: {reason}"), &ctx),
                    details: vec![detail("result", format!("skipped: {reason}"), &ctx)],
                });
            }
            EventData::StageInterrupted { run, stage } => {
                let attempt = active_attempt
                    .get(&(run.clone(), stage.clone()))
                    .copied()
                    .unwrap_or(1);
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: actor_str.clone(),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::JobEnd,
                    run: Some(run.clone()),
                    stage: Some(stage.clone()),
                    attempt: Some(attempt),
                    correlation_id: Some(format!("{run}.{stage}.{attempt}")),
                    summary: redact_text(&format!("{stage} interrupted"), &ctx),
                    details: vec![detail("result", "interrupted", &ctx)],
                });
            }
            EventData::IdeaPlanSnapshotFrozen {
                plan_snapshot,
                approved_by,
                via,
            } => {
                let ver = plan_versions
                    .get(&plan_snapshot.path)
                    .copied()
                    .unwrap_or(plan_snapshot.version);
                let what = format!("plan v{ver}");
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: redact_text(&approved_by, &ctx),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::Approval,
                    run: None,
                    stage: None,
                    attempt: None,
                    correlation_id: None,
                    summary: redact_text(
                        &format!("approved ready-for-development · via {via} · {what}"),
                        &ctx,
                    ),
                    details: vec![
                        detail("gate", "ready-for-development", &ctx),
                        detail("who", approved_by, &ctx),
                        detail("surface", via, &ctx),
                        detail("what", what, &ctx),
                    ],
                });
            }
            EventData::ApprovalGranted {
                gate,
                action,
                run,
                stage,
                actor: approver,
                surface,
                ..
            } => {
                let what = format!("{action:?} on {stage}");
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: redact_text(&approver.id, &ctx),
                    actor_kind: approver.kind,
                    kind: AuditKind::Approval,
                    run: Some(run),
                    stage: Some(stage.clone()),
                    attempt: None,
                    correlation_id: None,
                    summary: redact_text(
                        &format!("approved gate {gate} ({what}) · via {surface}"),
                        &ctx,
                    ),
                    details: vec![
                        detail("gate", format!("gate {gate}"), &ctx),
                        detail("who", approver.id, &ctx),
                        detail("surface", surface, &ctx),
                        detail("what", what, &ctx),
                    ],
                });
            }
            EventData::StageModelUpgraded {
                run,
                stage,
                from,
                to,
            } => {
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: actor_str.clone(),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::Decision,
                    run: Some(run),
                    stage: Some(stage.clone()),
                    attempt: None,
                    correlation_id: None,
                    summary: redact_text(&format!("{stage} model upgraded · {from} → {to}"), &ctx),
                    details: vec![
                        detail("decision", "model upgrade", &ctx),
                        detail("change", format!("{from} → {to}"), &ctx),
                    ],
                });
            }
            EventData::StageRetryRequested {
                run,
                stage,
                hypothesis,
            } => {
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: actor_str.clone(),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::Decision,
                    run: Some(run),
                    stage: Some(stage.clone()),
                    attempt: None,
                    correlation_id: None,
                    summary: redact_text(&format!("{stage} retry requested · {hypothesis}"), &ctx),
                    details: vec![
                        detail("decision", "retry", &ctx),
                        detail("hypothesis", hypothesis, &ctx),
                    ],
                });
            }
            EventData::StageReopened { run, stage, reason } => {
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: actor_str.clone(),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::Decision,
                    run: Some(run),
                    stage: Some(stage.clone()),
                    attempt: None,
                    correlation_id: None,
                    summary: redact_text(&format!("{stage} reopened · {reason}"), &ctx),
                    details: vec![
                        detail("decision", "reopened stage", &ctx),
                        detail("reason", reason, &ctx),
                    ],
                });
            }
            EventData::BudgetChanged { run, old, new } => {
                let change = format!("{} → {}", format_budget(&old), format_budget(&new));
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: actor_str.clone(),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::Decision,
                    run: Some(run),
                    stage: None,
                    attempt: None,
                    correlation_id: None,
                    summary: redact_text(&format!("budget changed · {change}"), &ctx),
                    details: vec![
                        detail("decision", "budget change", &ctx),
                        detail("change", change, &ctx),
                    ],
                });
            }
            EventData::SettingsChanged {
                key,
                old_value,
                new_value,
                actor: who,
            } => {
                let change = format!("{key}: {old_value} → {new_value}");
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: redact_text(&who, &ctx),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::Decision,
                    run: None,
                    stage: None,
                    attempt: None,
                    correlation_id: None,
                    summary: redact_text(&format!("settings changed · {change}"), &ctx),
                    details: vec![
                        detail("decision", "settings change", &ctx),
                        detail("change", change, &ctx),
                    ],
                });
            }
            EventData::ProviderConsented {
                provider,
                auth_type,
                verified_on,
                terms_record,
                consented_by,
            } => {
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: redact_text(&consented_by, &ctx),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::Decision,
                    run: None,
                    stage: None,
                    attempt: None,
                    correlation_id: None,
                    summary: redact_text(
                        &format!("provider consented · {provider} ({auth_type})"),
                        &ctx,
                    ),
                    details: vec![
                        detail("decision", "provider consent", &ctx),
                        detail("provider", provider, &ctx),
                        detail("verified_on", verified_on, &ctx),
                        detail("terms", terms_record, &ctx),
                    ],
                });
            }
            EventData::ProviderRevoked { provider } => {
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: actor_str.clone(),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::Decision,
                    run: None,
                    stage: None,
                    attempt: None,
                    correlation_id: None,
                    summary: redact_text(&format!("provider revoked · {provider}"), &ctx),
                    details: vec![
                        detail("decision", "provider revoked", &ctx),
                        detail("provider", provider, &ctx),
                    ],
                });
            }
            EventData::RunWithdrawn { run } => {
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: actor_str.clone(),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::Decision,
                    run: Some(run.clone()),
                    stage: None,
                    attempt: None,
                    correlation_id: None,
                    summary: redact_text(&format!("run {run} withdrawn"), &ctx),
                    details: vec![detail("decision", "run withdrawn", &ctx)],
                });
            }
            EventData::RunCancelled { run } => {
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: actor_str.clone(),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::Decision,
                    run: Some(run.clone()),
                    stage: None,
                    attempt: None,
                    correlation_id: None,
                    summary: redact_text(&format!("run {run} cancelled"), &ctx),
                    details: vec![detail("decision", "run cancelled", &ctx)],
                });
            }
            EventData::IdeaCancelled { reason } => {
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: actor_str.clone(),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::Decision,
                    run: None,
                    stage: None,
                    attempt: None,
                    correlation_id: None,
                    summary: redact_text(&format!("idea cancelled · {reason}"), &ctx),
                    details: vec![
                        detail("decision", "idea cancelled", &ctx),
                        detail("reason", reason, &ctx),
                    ],
                });
            }
            EventData::RunCreated {
                run,
                workflow,
                version,
                ..
            } => {
                let wf = if version.is_empty() {
                    workflow
                } else if version.starts_with('v') {
                    format!("{workflow} {version}")
                } else {
                    format!("{workflow} v{version}")
                };
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: actor_str.clone(),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::Lifecycle,
                    run: Some(run.clone()),
                    stage: None,
                    attempt: None,
                    correlation_id: None,
                    summary: redact_text(&format!("run {run} created · {wf}"), &ctx),
                    details: vec![detail("workflow", wf, &ctx)],
                });
            }
            EventData::IdeaDelivered { run, prs } => {
                entry = Some(AuditEntry {
                    seq: ev.seq,
                    ts: ev.ts,
                    actor: actor_str.clone(),
                    actor_kind: ev.actor.kind,
                    kind: AuditKind::Lifecycle,
                    run: Some(run),
                    stage: None,
                    attempt: None,
                    correlation_id: None,
                    summary: redact_text(&format!("idea delivered · {} PR(s)", prs.len()), &ctx),
                    details: prs
                        .into_iter()
                        .map(|p| detail("pr", format!("{}#{} ({})", p.repo, p.number, p.url), &ctx))
                        .collect(),
                });
            }
            _ => {}
        }

        if let Some(e) = entry {
            if let Some(ref run_f) = filter.run
                && e.run.as_deref() != Some(run_f.as_str())
            {
                continue;
            }
            if let Some(kind_f) = filter.kind
                && e.kind != kind_f
            {
                continue;
            }
            if let Some(ref actor_f) = filter.actor
                && !e.actor.eq_ignore_ascii_case(actor_f)
            {
                continue;
            }
            out.push(e);
        }
    }

    Ok(out)
}

pub fn format_time_short(ts: f64) -> String {
    let secs = ts.max(0.0) as i64;
    DateTime::<Utc>::from_timestamp(secs, 0)
        .map(|dt| dt.with_timezone(&Local).format("%H:%M").to_string())
        .unwrap_or_else(|| "--:--".to_string())
}

pub fn format_time_iso(ts: f64) -> String {
    let secs = ts.max(0.0) as i64;
    DateTime::<Utc>::from_timestamp(secs, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

pub fn export_json(
    entries: &[AuditEntry],
    verify_report: Option<&AuditVerifyReport>,
) -> Result<String, FactoryError> {
    let verify = verify_report.cloned().unwrap_or(AuditVerifyReport {
        total_events: 0,
        chain_start_seq: None,
        verified_events: 0,
        intact: true,
        first_break: None,
    });
    let payload = AuditPayload {
        entries: entries.to_vec(),
        verify,
    };
    Ok(serde_json::to_string_pretty(&payload)?)
}

pub fn export_markdown(
    idea_id: &str,
    entries: &[AuditEntry],
    verify_report: Option<&AuditVerifyReport>,
) -> String {
    let ctx = RedactContext::from_env();
    let mut out = format!("# Audit Timeline: {idea_id}\n\n");
    if let Some(rep) = verify_report {
        out.push_str(&format!(
            "**Log integrity:** {}\n\n",
            format_verify_summary(rep)
        ));
    }
    if entries.is_empty() {
        out.push_str("_No audit entries match the current filter._\n");
        return out;
    }

    for entry in entries {
        let time = format_time_iso(entry.ts);
        let summary = redact_text(&entry.summary, &ctx);
        let actor = redact_text(&entry.actor, &ctx);
        out.push_str(&format!(
            "## [{time}] {summary}\n\n- **Kind:** {}\n- **Actor:** {actor}\n",
            entry.kind.label()
        ));
        if let Some(ref run) = entry.run {
            out.push_str(&format!("- **Run:** `{run}`\n"));
        }
        for d in &entry.details {
            let val = redact_text(&d.value, &ctx);
            out.push_str(&format!("- **{}:** {val}\n", d.label));
        }
        out.push('\n');
    }
    out
}
