use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use super::FactoryError;
use super::broker::result::JobUsage;
use super::events::{Event, append, read_all, read_run_events, redact_event};
use super::machine::{self, EventData};
use super::model::{Actor, IdeaState, ModelPrice, PrLink, Run};
use super::redact::RedactContext;
use super::routing::ModelTier;
use super::run::get_run;
use super::store::{FactoryStore, current_time, write_atomic};

pub const TYPICAL_INPUT_TOKENS: u64 = 25_000;
pub const TYPICAL_OUTPUT_TOKENS: u64 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostBasis {
    Actual,
    Estimated,
    Unknown,
}

impl CostBasis {
    pub fn combine(self, other: Self) -> Self {
        self.max(other)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Actual => "actual",
            Self::Estimated => "estimated",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    pub basis: CostBasis,
}

impl Cost {
    pub fn unknown() -> Self {
        Self {
            value: None,
            basis: CostBasis::Unknown,
        }
    }

    pub fn combine(&self, other: &Self) -> Self {
        let basis = self.basis.combine(other.basis);
        if basis == CostBasis::Unknown {
            return Self::unknown();
        }
        match (self.value, other.value) {
            (Some(a), Some(b)) if a.is_finite() && b.is_finite() => Self {
                value: Some(a + b),
                basis,
            },
            _ => Self::unknown(),
        }
    }

    pub fn format(&self) -> String {
        match (self.basis, self.value) {
            (CostBasis::Actual, Some(v)) if v.is_finite() => format!("${v:.2}"),
            (CostBasis::Estimated, Some(v)) if v.is_finite() => {
                format!("about ${v:.2} (estimated)")
            }
            _ => "unknown".to_string(),
        }
    }
}

pub fn compute_cost(
    usage: Option<&JobUsage>,
    model: &str,
    price_table: &BTreeMap<String, ModelPrice>,
) -> Cost {
    if let Some(u) = usage
        && let Some(c) = u.cost_usd
        && c.is_finite()
        && c >= 0.0
    {
        return Cost {
            value: Some(c),
            basis: CostBasis::Actual,
        };
    }

    let Some(price) = price_table.get(model) else {
        return Cost::unknown();
    };

    if let Some(u) = usage
        && (u.input_tokens.is_some() || u.output_tokens.is_some())
    {
        let in_t = u.input_tokens.unwrap_or(0) as f64;
        let out_t = u.output_tokens.unwrap_or(0) as f64;
        let val = (in_t / 1_000_000.0) * price.input + (out_t / 1_000_000.0) * price.output;
        return Cost {
            value: Some(val),
            basis: CostBasis::Actual,
        };
    }

    let est = (TYPICAL_INPUT_TOKENS as f64 / 1_000_000.0) * price.input
        + (TYPICAL_OUTPUT_TOKENS as f64 / 1_000_000.0) * price.output;
    Cost {
        value: Some(est),
        basis: CostBasis::Estimated,
    }
}

pub struct RecordJobUsageInput<'a> {
    pub store: &'a FactoryStore,
    pub idea_id: &'a str,
    pub run_id: &'a str,
    pub stage: &'a str,
    pub attempt: u32,
    pub provider: &'a str,
    pub model: &'a str,
    pub tier: ModelTier,
    pub duration_secs: f64,
    pub usage: Option<&'a JobUsage>,
    pub price_table: &'a BTreeMap<String, ModelPrice>,
    pub actor: &'a Actor,
}

fn tier_label(tier: ModelTier) -> String {
    match tier {
        ModelTier::Economy => "economy",
        ModelTier::Standard => "standard",
        ModelTier::Premium => "premium",
    }
    .to_string()
}

pub fn build_job_usage_event(input: &RecordJobUsageInput<'_>) -> EventData {
    let cost = compute_cost(input.usage, input.model, input.price_table);
    let cost_usd = if cost.basis == CostBasis::Unknown {
        None
    } else {
        cost.value
    };
    EventData::JobUsage {
        run: input.run_id.to_string(),
        stage: input.stage.to_string(),
        attempt: input.attempt,
        provider: input.provider.to_string(),
        model: input.model.to_string(),
        tier: tier_label(input.tier),
        duration_secs: input.duration_secs.max(0.0),
        input_tokens: input.usage.and_then(|u| u.input_tokens),
        output_tokens: input.usage.and_then(|u| u.output_tokens),
        cost,
        cost_usd,
    }
}

pub fn record_job_usage(input: RecordJobUsageInput<'_>) -> Result<Event, FactoryError> {
    input.store.ensure()?;
    let ev_data = build_job_usage_event(&input);
    let ev = Event::from_data(
        current_time(),
        input.actor.clone(),
        input.idea_id.to_string(),
        Some(input.run_id.to_string()),
        &ev_data,
    )?;
    let redacted = redact_event(ev, &RedactContext::from_env());
    let appended = append(
        &input.store.events_path(),
        &input.store.lock_path(),
        &[redacted],
    )?;
    let recorded = appended.into_iter().next().unwrap();
    if let Ok(Some(run)) = get_run(input.store, input.run_id)
        && let Some(updated_run) =
            machine::apply_run(Some(run), &ev_data, input.idea_id, recorded.ts)
        && let Ok(bytes) = serde_json::to_vec_pretty(&updated_run)
    {
        let _ = write_atomic(&input.store.run_json_path(input.run_id), &bytes);
    }
    Ok(recorded)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageRework {
    pub retries: u32,
    pub repeated_rationales: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReworkSignals {
    pub retries_by_stage: BTreeMap<String, StageRework>,
    pub total_retries: u32,
    pub repeated_rationales: u32,
    pub develop_reopened_by_test: u32,
    pub develop_reopened_by_review: u32,
    pub review_rounds: u32,
    pub model_upgrades: u32,
    pub interrupted_jobs: u32,
}

pub fn compute_rework_signals(run_id: &str, events: &[Event]) -> ReworkSignals {
    let mut started_counts: BTreeMap<String, u32> = BTreeMap::new();
    let mut interrupted_counts: BTreeMap<String, u32> = BTreeMap::new();
    let mut seen_rationales: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    let mut repeated_by_stage: BTreeMap<String, u32> = BTreeMap::new();

    let mut develop_reopened_by_test = 0u32;
    let mut develop_reopened_by_review = 0u32;
    let mut review_rounds = 0u32;
    let mut model_upgrades = 0u32;
    let mut interrupted_jobs = 0u32;

    let mut last_failed_stage: Option<String> = None;
    let mut pending_changes_requested = false;

    for ev in events {
        if ev.run.as_deref() != Some(run_id) {
            continue;
        }
        match ev.r#type.as_str() {
            "stage.started" => {
                if let Some(stage) = ev.data.get("stage").and_then(|v| v.as_str()) {
                    *started_counts.entry(stage.to_string()).or_insert(0) += 1;
                }
            }
            "stage.succeeded" => {
                if let Some(stage) = ev.data.get("stage").and_then(|v| v.as_str())
                    && stage == "review"
                {
                    review_rounds += 1;
                }
            }
            "stage.failed" => {
                if let Some(stage) = ev.data.get("stage").and_then(|v| v.as_str()) {
                    last_failed_stage = Some(stage.to_string());
                    if let Some(reason) = ev.data.get("reason").and_then(|v| v.as_str()) {
                        let norm = reason.trim().to_lowercase();
                        if !norm.is_empty() {
                            let set = seen_rationales.entry(stage.to_string()).or_default();
                            if !set.insert(norm) {
                                *repeated_by_stage.entry(stage.to_string()).or_insert(0) += 1;
                            }
                        }
                    }
                }
            }
            "stage.interrupted" => {
                interrupted_jobs += 1;
                if let Some(stage) = ev.data.get("stage").and_then(|v| v.as_str()) {
                    *interrupted_counts.entry(stage.to_string()).or_insert(0) += 1;
                }
            }
            "stage.model_upgraded" => {
                model_upgrades += 1;
            }
            "run.changes_requested" => {
                pending_changes_requested = true;
            }
            "stage.reopened" => {
                if let Some(stage) = ev.data.get("stage").and_then(|v| v.as_str())
                    && stage == "develop"
                {
                    if pending_changes_requested {
                        develop_reopened_by_review += 1;
                        pending_changes_requested = false;
                    } else if last_failed_stage.as_deref() == Some("test") {
                        develop_reopened_by_test += 1;
                    }
                }
            }
            _ => {}
        }
    }

    let mut all_stages: HashSet<String> = HashSet::new();
    all_stages.extend(started_counts.keys().cloned());
    all_stages.extend(repeated_by_stage.keys().cloned());
    all_stages.extend(interrupted_counts.keys().cloned());

    let mut retries_by_stage = BTreeMap::new();
    let mut total_retries = 0u32;
    let mut repeated_rationales = 0u32;

    for stage in all_stages {
        let starts = started_counts.get(&stage).copied().unwrap_or(0);
        let ints = interrupted_counts.get(&stage).copied().unwrap_or(0);
        let retries = starts.saturating_sub(1 + ints);
        let rep = repeated_by_stage.get(&stage).copied().unwrap_or(0);
        total_retries += retries;
        repeated_rationales += rep;
        if retries > 0 || rep > 0 {
            retries_by_stage.insert(
                stage,
                StageRework {
                    retries,
                    repeated_rationales: rep,
                },
            );
        }
    }

    ReworkSignals {
        retries_by_stage,
        total_retries,
        repeated_rationales,
        develop_reopened_by_test,
        develop_reopened_by_review,
        review_rounds,
        model_upgrades,
        interrupted_jobs,
    }
}

fn parse_event_cost(ev: &Event) -> Option<Cost> {
    if let Some(cost_val) = ev.data.get("cost")
        && let Ok(c) = serde_json::from_value::<Cost>(cost_val.clone())
    {
        return Some(c);
    }
    if let Some(c) = ev.data.get("cost_usd").and_then(|v| v.as_f64())
        && c.is_finite()
        && c >= 0.0
    {
        return Some(Cost {
            value: Some(c),
            basis: CostBasis::Actual,
        });
    }
    if ev.r#type == "job.usage" {
        return Some(Cost::unknown());
    }
    None
}

pub fn total_cost_from_events(run_id: &str, events: &[Event]) -> Cost {
    let mut total: Option<Cost> = None;
    for ev in events {
        if ev.run.as_deref() != Some(run_id) {
            continue;
        }
        if !matches!(
            ev.r#type.as_str(),
            "job.usage" | "telemetry.usage" | "stage.usage"
        ) {
            continue;
        }
        if let Some(c) = parse_event_cost(ev) {
            total = Some(match total {
                Some(acc) => acc.combine(&c),
                None => c,
            });
        }
    }
    total.unwrap_or_else(Cost::unknown)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageUsageSummary {
    pub stage: String,
    pub jobs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    pub duration_secs: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    pub cost: Cost,
    pub cost_display: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunUsageReport {
    pub run: String,
    pub stages: Vec<StageUsageSummary>,
    pub total_duration_secs: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_output_tokens: Option<u64>,
    pub total_cost: Cost,
    pub total_cost_display: String,
    pub rework: ReworkSignals,
}

struct StageAcc {
    jobs: u32,
    provider: Option<String>,
    model: Option<String>,
    tier: Option<String>,
    duration_secs: f64,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cost: Option<Cost>,
}

pub fn run_usage_from_events(run: &Run, events: &[Event]) -> RunUsageReport {
    let mut by_stage: BTreeMap<String, StageAcc> = BTreeMap::new();
    let mut stage_order: Vec<String> = run.stages.iter().map(|s| s.name.clone()).collect();

    for ev in events {
        if ev.run.as_deref() != Some(run.id.as_str()) {
            continue;
        }
        if !matches!(
            ev.r#type.as_str(),
            "job.usage" | "telemetry.usage" | "stage.usage"
        ) {
            continue;
        }
        let stage = ev
            .data
            .get("stage")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        if !stage_order.contains(&stage) {
            stage_order.push(stage.clone());
        }
        let acc = by_stage.entry(stage).or_insert(StageAcc {
            jobs: 0,
            provider: None,
            model: None,
            tier: None,
            duration_secs: 0.0,
            input_tokens: None,
            output_tokens: None,
            cost: None,
        });
        acc.jobs += 1;
        if let Some(p) = ev.data.get("provider").and_then(|v| v.as_str()) {
            acc.provider = Some(p.to_string());
        }
        if let Some(m) = ev.data.get("model").and_then(|v| v.as_str()) {
            acc.model = Some(m.to_string());
        }
        if let Some(t) = ev.data.get("tier").and_then(|v| v.as_str()) {
            acc.tier = Some(t.to_string());
        }
        if let Some(d) = ev.data.get("duration_secs").and_then(|v| v.as_f64())
            && d.is_finite()
            && d >= 0.0
        {
            acc.duration_secs += d;
        }
        if let Some(it) = ev.data.get("input_tokens").and_then(|v| v.as_u64()) {
            acc.input_tokens = Some(acc.input_tokens.unwrap_or(0) + it);
        }
        if let Some(ot) = ev.data.get("output_tokens").and_then(|v| v.as_u64()) {
            acc.output_tokens = Some(acc.output_tokens.unwrap_or(0) + ot);
        }
        if let Some(c) = parse_event_cost(ev) {
            acc.cost = Some(match acc.cost.take() {
                Some(prev) => prev.combine(&c),
                None => c,
            });
        }
    }

    let mut stages = Vec::new();
    let mut total_duration_secs = 0.0f64;
    let mut total_input_tokens: Option<u64> = None;
    let mut total_output_tokens: Option<u64> = None;
    let mut total_cost: Option<Cost> = None;

    for stage_name in stage_order {
        let Some(acc) = by_stage.remove(&stage_name) else {
            continue;
        };
        let cost = acc.cost.unwrap_or_else(Cost::unknown);
        let cost_display = cost.format();
        total_duration_secs += acc.duration_secs;
        if let Some(it) = acc.input_tokens {
            total_input_tokens = Some(total_input_tokens.unwrap_or(0) + it);
        }
        if let Some(ot) = acc.output_tokens {
            total_output_tokens = Some(total_output_tokens.unwrap_or(0) + ot);
        }
        total_cost = Some(match total_cost {
            Some(prev) => prev.combine(&cost),
            None => cost.clone(),
        });
        stages.push(StageUsageSummary {
            stage: stage_name,
            jobs: acc.jobs,
            provider: acc.provider,
            model: acc.model,
            tier: acc.tier,
            duration_secs: acc.duration_secs,
            input_tokens: acc.input_tokens,
            output_tokens: acc.output_tokens,
            cost,
            cost_display,
        });
    }

    let total_cost = total_cost.unwrap_or_else(Cost::unknown);
    let total_cost_display = total_cost.format();
    let rework = compute_rework_signals(&run.id, events);

    RunUsageReport {
        run: run.id.clone(),
        stages,
        total_duration_secs,
        total_input_tokens,
        total_output_tokens,
        total_cost,
        total_cost_display,
        rework,
    }
}

pub fn run_usage(store: &FactoryStore, run_id: &str) -> Result<RunUsageReport, FactoryError> {
    let run = get_run(store, run_id)
        .map_err(|e| FactoryError::InvalidInput(e.to_string()))?
        .ok_or_else(|| FactoryError::InvalidInput(format!("run {run_id} not found")))?;
    let events = read_run_events(&store.events_path(), run_id)?;
    Ok(run_usage_from_events(&run, &events))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdeaReport {
    pub id: String,
    pub title: String,
    pub state: IdeaState,
    pub state_times_secs: BTreeMap<String, f64>,
    pub stage_durations_secs: BTreeMap<String, f64>,
    pub rework: ReworkSignals,
    pub cost: Cost,
    pub cost_display: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_complete: Option<bool>,
    #[serde(default)]
    pub evidence_missing: Vec<String>,
    #[serde(default)]
    pub prs: Vec<PrLink>,
    pub recoveries: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PilotReportTotals {
    pub total_ideas: usize,
    pub delivered_ideas: usize,
    pub cancelled_ideas: usize,
    pub by_state: BTreeMap<String, usize>,
    pub median_state_times_secs: BTreeMap<String, f64>,
    pub stage_durations_secs: BTreeMap<String, f64>,
    pub rework: ReworkSignals,
    pub cost: Cost,
    pub cost_display: String,
    pub unknown_cost_runs: usize,
    pub evidence_complete_count: usize,
    pub evidence_checked_count: usize,
    pub pr_counts: BTreeMap<String, usize>,
    pub recoveries_performed: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PilotReport {
    pub ideas: Vec<IdeaReport>,
    pub totals: PilotReportTotals,
}

fn state_key(state: IdeaState) -> &'static str {
    match state {
        IdeaState::Inbox => "inbox",
        IdeaState::Ideation => "ideation",
        IdeaState::Planning => "planning",
        IdeaState::ReadyForDevelopment => "ready_for_development",
        IdeaState::Queued => "queued",
        IdeaState::InDevelopment => "in_development",
        IdeaState::Review => "review",
        IdeaState::Delivered => "delivered",
        IdeaState::Cancelled => "cancelled",
    }
}

fn state_label(key: &str) -> &str {
    match key {
        "inbox" => "inbox",
        "ideation" => "ideation",
        "planning" => "planning",
        "ready_for_development" => "ready",
        "queued" => "queued",
        "in_development" => "in development",
        "review" => "review",
        "delivered" => "delivered",
        "cancelled" => "cancelled",
        other => other,
    }
}

fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        values[mid]
    } else {
        (values[mid - 1] + values[mid]) / 2.0
    }
}

fn merge_rework(dst: &mut ReworkSignals, src: &ReworkSignals) {
    for (stage, sr) in &src.retries_by_stage {
        let entry = dst.retries_by_stage.entry(stage.clone()).or_default();
        entry.retries += sr.retries;
        entry.repeated_rationales += sr.repeated_rationales;
    }
    dst.total_retries += src.total_retries;
    dst.repeated_rationales += src.repeated_rationales;
    dst.develop_reopened_by_test += src.develop_reopened_by_test;
    dst.develop_reopened_by_review += src.develop_reopened_by_review;
    dst.review_rounds += src.review_rounds;
    dst.model_upgrades += src.model_upgrades;
    dst.interrupted_jobs += src.interrupted_jobs;
}

pub fn pilot_report(store: &FactoryStore, now: f64) -> Result<PilotReport, FactoryError> {
    let ideas = store.list_ideas()?;
    let log = if store.exists() {
        read_all(&store.events_path())?.events
    } else {
        Vec::new()
    };

    let mut events_by_idea: BTreeMap<String, Vec<&Event>> = BTreeMap::new();
    for ev in &log {
        events_by_idea.entry(ev.idea.clone()).or_default().push(ev);
    }

    let settings = store.settings().unwrap_or_default();
    let default_wf = store
        .root
        .parent()
        .and_then(|p| p.parent())
        .and_then(|ws| super::workflow::load_default_workflow(ws).ok())
        .map(|(wf, _)| wf);

    let mut by_state: BTreeMap<String, usize> = BTreeMap::new();
    let mut state_samples: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut total_stage_durations: BTreeMap<String, f64> = BTreeMap::new();
    let mut total_rework = ReworkSignals::default();
    let mut total_cost: Option<Cost> = None;
    let mut unknown_cost_runs = 0usize;
    let mut evidence_complete_count = 0usize;
    let mut evidence_checked_count = 0usize;
    let mut pr_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut total_recoveries = 0u32;
    let mut delivered_ideas = 0usize;
    let mut cancelled_ideas = 0usize;

    let mut idea_reports = Vec::with_capacity(ideas.len());

    for idea in ideas {
        *by_state
            .entry(state_key(idea.state).to_string())
            .or_insert(0) += 1;
        if idea.state == IdeaState::Delivered {
            delivered_ideas += 1;
        } else if idea.state == IdeaState::Cancelled {
            cancelled_ideas += 1;
        }

        let idea_events: Vec<Event> = events_by_idea
            .get(&idea.id)
            .map(|list| list.iter().map(|e| (*e).clone()).collect())
            .unwrap_or_default();

        let mut state_times_secs: BTreeMap<String, f64> = BTreeMap::new();
        let mut acc_idea = None;
        let mut cur_state: Option<IdeaState> = None;
        let mut entered_ts: Option<f64> = None;
        for ev in &idea_events {
            if let Ok(ev_data) = ev.to_event_data() {
                acc_idea = machine::apply(acc_idea, &ev_data, ev.ts);
                if let Some(ref next_idea) = acc_idea {
                    match cur_state {
                        None => {
                            cur_state = Some(next_idea.state);
                            entered_ts = Some(ev.ts);
                        }
                        Some(prev) if prev != next_idea.state => {
                            if let Some(start) = entered_ts {
                                let dt = (ev.ts - start).max(0.0);
                                *state_times_secs
                                    .entry(state_key(prev).to_string())
                                    .or_insert(0.0) += dt;
                            }
                            cur_state = Some(next_idea.state);
                            entered_ts = Some(ev.ts);
                        }
                        _ => {}
                    }
                }
            }
        }
        if let (Some(st), Some(start)) = (cur_state, entered_ts)
            && !st.is_terminal()
            && now > start
        {
            *state_times_secs
                .entry(state_key(st).to_string())
                .or_insert(0.0) += now - start;
        }
        for (k, &v) in &state_times_secs {
            state_samples.entry(k.clone()).or_default().push(v);
        }

        let mut stage_durations_secs: BTreeMap<String, f64> = BTreeMap::new();
        let mut idea_rework = ReworkSignals::default();
        let mut idea_cost: Option<Cost> = None;
        let mut evidence_complete: Option<bool> = None;
        let mut evidence_missing = Vec::new();
        let mut prs = idea.prs.clone();

        for run_id in &idea.runs {
            let rw = compute_rework_signals(run_id, &idea_events);
            merge_rework(&mut idea_rework, &rw);

            let run_cost = total_cost_from_events(run_id, &idea_events);
            if run_cost.basis == CostBasis::Unknown {
                unknown_cost_runs += 1;
            } else {
                total_cost = Some(match total_cost {
                    Some(prev) => prev.combine(&run_cost),
                    None => run_cost.clone(),
                });
            }
            idea_cost = Some(match idea_cost {
                Some(prev) => prev.combine(&run_cost),
                None => run_cost,
            });

            if let Ok(Some(run)) = get_run(store, run_id) {
                let usage = run_usage_from_events(&run, &idea_events);
                for st in usage.stages {
                    *stage_durations_secs.entry(st.stage.clone()).or_insert(0.0) +=
                        st.duration_secs;
                    *total_stage_durations.entry(st.stage).or_insert(0.0) += st.duration_secs;
                }
                for pr in &run.prs {
                    if !prs
                        .iter()
                        .any(|p| p.url == pr.url || (p.repo == pr.repo && p.number == pr.number))
                    {
                        prs.push(pr.clone());
                    }
                }
                let run_wf_path = store.run_dir(run_id).join("workflow.yaml");
                let raw_wf = if run_wf_path.exists() {
                    super::workflow::parse::load_workflow_file(&run_wf_path)
                        .ok()
                        .or_else(|| default_wf.clone())
                } else {
                    default_wf.clone()
                };
                if let Some(raw) = raw_wf
                    && let Ok(wf) = super::workflow::validate::validate(&raw, &settings)
                {
                    let missing = super::evidence::missing(&run, &wf);
                    let complete = missing.is_empty();
                    evidence_complete = Some(match evidence_complete {
                        Some(prev) => prev && complete,
                        None => complete,
                    });
                    for m in missing {
                        let s = m.as_str().to_string();
                        if !evidence_missing.contains(&s) {
                            evidence_missing.push(s);
                        }
                    }
                }
            }
        }

        if let Some(complete) = evidence_complete {
            evidence_checked_count += 1;
            if complete {
                evidence_complete_count += 1;
            }
        }

        for pr in &prs {
            let key = pr
                .state
                .as_deref()
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "unknown".to_string());
            *pr_counts.entry(key).or_insert(0) += 1;
        }

        let recoveries = idea_rework.interrupted_jobs;
        total_recoveries += recoveries;
        merge_rework(&mut total_rework, &idea_rework);

        let cost = idea_cost.unwrap_or_else(Cost::unknown);
        let cost_display = cost.format();

        idea_reports.push(IdeaReport {
            id: idea.id,
            title: idea.title,
            state: idea.state,
            state_times_secs,
            stage_durations_secs,
            rework: idea_rework,
            cost,
            cost_display,
            evidence_complete,
            evidence_missing,
            prs,
            recoveries,
        });
    }

    let mut median_state_times_secs = BTreeMap::new();
    for (k, mut samples) in state_samples {
        median_state_times_secs.insert(k, median(&mut samples));
    }

    let totals_cost = total_cost.unwrap_or_else(Cost::unknown);
    let cost_display = if unknown_cost_runs > 0 && totals_cost.value.is_some() {
        format!(
            "{} ({unknown_cost_runs} run{} unknown)",
            totals_cost.format(),
            if unknown_cost_runs == 1 { "" } else { "s" }
        )
    } else {
        totals_cost.format()
    };

    Ok(PilotReport {
        ideas: idea_reports,
        totals: PilotReportTotals {
            total_ideas: ideas_count(&by_state),
            delivered_ideas,
            cancelled_ideas,
            by_state,
            median_state_times_secs,
            stage_durations_secs: total_stage_durations,
            rework: total_rework,
            cost: totals_cost,
            cost_display,
            unknown_cost_runs,
            evidence_complete_count,
            evidence_checked_count,
            pr_counts,
            recoveries_performed: total_recoveries,
        },
    })
}

fn ideas_count(by_state: &BTreeMap<String, usize>) -> usize {
    by_state.values().sum()
}

fn format_mins_secs(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    if total >= 60 {
        format!("{}m", total / 60)
    } else {
        format!("{total}s")
    }
}

pub fn format_pilot_report_text(report: &PilotReport) -> String {
    let t = &report.totals;
    let pr_summary = if t.pr_counts.is_empty() {
        "none".to_string()
    } else {
        t.pr_counts
            .iter()
            .map(|(k, v)| format!("{v} {k}"))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let mut out = format!(
        "ideas delivered {} of {}      evidence complete {} of {}      PRs: {}\n",
        t.delivered_ideas,
        t.total_ideas,
        t.evidence_complete_count,
        t.evidence_checked_count,
        pr_summary,
    );

    if !t.median_state_times_secs.is_empty() {
        let states = ["ideation", "planning", "queued", "in_development", "review"];
        let parts: Vec<String> = states
            .iter()
            .filter_map(|k| {
                t.median_state_times_secs
                    .get(*k)
                    .map(|v| format!("{} {}", state_label(k), format_mins_secs(*v)))
            })
            .collect();
        if !parts.is_empty() {
            out.push_str(&format!("median time  {}\n", parts.join(" · ")));
        }
    }

    let reopened = t.rework.develop_reopened_by_test + t.rework.develop_reopened_by_review;
    out.push_str(&format!(
        "rework       retries {} ({} repeated the same failure) · develop reopened {} · review rounds {} · model upgrades {}\n",
        t.rework.total_retries,
        t.rework.repeated_rationales,
        reopened,
        t.rework.review_rounds,
        t.rework.model_upgrades,
    ));
    out.push_str(&format!("cost         {}\n", t.cost_display));
    out.push_str(&format!(
        "recovery     {} interrupted job{} resumed\n",
        t.recoveries_performed,
        if t.recoveries_performed == 1 { "" } else { "s" }
    ));

    out
}

pub fn format_pilot_report_markdown(report: &PilotReport) -> String {
    let mut out = String::from("# Factory Pilot Report\n\n");
    out.push_str("## Totals\n\n");
    out.push_str("```text\n");
    out.push_str(&format_pilot_report_text(report));
    out.push_str("```\n\n");
    out.push_str("## Ideas\n\n");
    if report.ideas.is_empty() {
        out.push_str("No ideas recorded.\n");
        return out;
    }
    out.push_str("| ID | Title | State | Retries | Review Rounds | Upgrades | Cost | Evidence | Recoveries |\n");
    out.push_str("| --- | --- | --- | ---: | ---: | ---: | --- | --- | ---: |\n");
    for idea in &report.ideas {
        let ev = match idea.evidence_complete {
            Some(true) => "complete",
            Some(false) => "incomplete",
            None => "n/a",
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            idea.id,
            idea.title.replace('|', "\\|"),
            idea.state.label(),
            idea.rework.total_retries,
            idea.rework.review_rounds,
            idea.rework.model_upgrades,
            idea.cost_display,
            ev,
            idea.recoveries,
        ));
    }
    out
}
