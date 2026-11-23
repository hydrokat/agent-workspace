use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use super::FactoryError;
use super::events::Event;
use super::model::{Budget, FactorySettings, ModelPrice, Run, StageState};
use super::routing::{RoutingContext, route_interactive};
use super::workflow::schema::{StageBudget as WorkflowStageBudget, Workflow};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunBudget {
    pub max_cost_usd: Option<f64>,
    pub max_duration_secs: Option<u64>,
    pub max_retries: Option<u32>,
}

impl From<Budget> for RunBudget {
    fn from(b: Budget) -> Self {
        Self {
            max_cost_usd: b.max_cost_usd,
            max_duration_secs: b.max_duration_secs,
            max_retries: b.max_retries,
        }
    }
}

impl From<RunBudget> for Budget {
    fn from(b: RunBudget) -> Self {
        Self {
            max_cost_usd: b.max_cost_usd,
            max_duration_secs: b.max_duration_secs,
            max_retries: b.max_retries,
        }
    }
}

impl RunBudget {
    pub fn from_settings(settings: &FactorySettings) -> Self {
        Self::from(settings.default_budget.clone())
    }

    pub fn from_run_or_settings(run: &Run, settings: &FactorySettings) -> Self {
        run.budget
            .clone()
            .map(Self::from)
            .unwrap_or_else(|| Self::from_settings(settings))
    }

    pub fn validate_within(&self, settings: &FactorySettings) -> Result<(), FactoryError> {
        if let Some(cost) = self.max_cost_usd {
            if !cost.is_finite() || cost < 0.0 {
                return Err(FactoryError::InvalidInput(format!(
                    "max_cost_usd must be >= 0.0 (got {cost})"
                )));
            }
            if let Some(limit) = settings.default_budget.max_cost_usd
                && cost > limit
            {
                return Err(FactoryError::InvalidInput(format!(
                    "max_cost_usd ({cost:.2}) exceeds settings limit ({limit:.2})"
                )));
            }
        }
        if let Some(dur) = self.max_duration_secs
            && let Some(limit) = settings.default_budget.max_duration_secs
            && dur > limit
        {
            return Err(FactoryError::InvalidInput(format!(
                "max_duration_secs ({dur}) exceeds settings limit ({limit})"
            )));
        }
        if let Some(retries) = self.max_retries
            && let Some(limit) = settings.default_budget.max_retries
            && retries > limit
        {
            return Err(FactoryError::InvalidInput(format!(
                "max_retries ({retries}) exceeds settings limit ({limit})"
            )));
        }
        Ok(())
    }

    pub fn with_overrides(
        &self,
        max_cost_usd: Option<f64>,
        max_duration_secs: Option<u64>,
        max_retries: Option<u32>,
        settings: &FactorySettings,
    ) -> Result<Self, FactoryError> {
        let mut next = self.clone();
        if let Some(c) = max_cost_usd {
            next.max_cost_usd = Some(c);
        }
        if let Some(d) = max_duration_secs {
            next.max_duration_secs = Some(d);
        }
        if let Some(r) = max_retries {
            next.max_retries = Some(r);
        }
        next.validate_within(settings)?;
        Ok(next)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StageBudget {
    pub max_cost_usd: Option<f64>,
    pub max_duration_secs: Option<u64>,
    pub max_retries: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Spent {
    pub cost_usd: Option<f64>,
    pub duration_secs: u64,
    pub retries: u32,
}

fn min_opt_f64(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (Some(x), None) | (None, Some(x)) => Some(x),
        (None, None) => None,
    }
}

fn min_opt_u64(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (Some(x), None) | (None, Some(x)) => Some(x),
        (None, None) => None,
    }
}

fn min_opt_u32(a: Option<u32>, b: Option<u32>) -> Option<u32> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (Some(x), None) | (None, Some(x)) => Some(x),
        (None, None) => None,
    }
}

pub fn effective_stage_budget(
    stage: &WorkflowStageBudget,
    run_budget: &RunBudget,
    spent: &Spent,
) -> StageBudget {
    let remaining_cost = run_budget
        .max_cost_usd
        .map(|cap| (cap - spent.cost_usd.unwrap_or(0.0)).max(0.0));
    let remaining_duration = run_budget
        .max_duration_secs
        .map(|cap| cap.saturating_sub(spent.duration_secs));
    let remaining_retries = run_budget
        .max_retries
        .map(|cap| cap.saturating_sub(spent.retries));

    StageBudget {
        max_cost_usd: min_opt_f64(stage.max_cost_usd, remaining_cost),
        max_duration_secs: min_opt_u64(stage.max_duration_secs, remaining_duration),
        max_retries: min_opt_u32(stage.max_retries, remaining_retries),
    }
}

pub fn spent(run: &Run, events: &[Event], now: f64) -> Spent {
    let mut active_starts: HashMap<String, f64> = HashMap::new();
    let mut total_duration = 0.0f64;
    let mut saw_stage_event = false;

    for ev in events {
        if ev.run.as_deref() != Some(run.id.as_str()) {
            continue;
        }
        match ev.r#type.as_str() {
            "stage.started" => {
                if let Some(stage) = ev.data.get("stage").and_then(|v| v.as_str()) {
                    saw_stage_event = true;
                    active_starts.insert(stage.to_string(), ev.ts);
                }
            }
            "stage.succeeded" | "stage.failed" | "stage.interrupted" | "stage.skipped" => {
                if let Some(stage) = ev.data.get("stage").and_then(|v| v.as_str())
                    && let Some(t0) = active_starts.remove(stage)
                {
                    total_duration += (ev.ts - t0).max(0.0);
                }
            }
            _ => {}
        }
    }

    for (_, t0) in active_starts {
        total_duration += (now - t0).max(0.0);
    }

    if !saw_stage_event {
        for st in &run.stages {
            if let Some(t0) = st.started {
                let t1 = st.finished.unwrap_or(now);
                total_duration += (t1 - t0).max(0.0);
            }
        }
    }

    let retries: u32 = run
        .stages
        .iter()
        .map(|s| s.attempts.saturating_sub(1 + s.interrupted_attempts))
        .sum();

    let total_cost = super::telemetry::total_cost_from_events(&run.id, events);
    let cost_usd = if total_cost.basis == super::telemetry::CostBasis::Unknown {
        None
    } else {
        total_cost.value
    };

    Spent {
        cost_usd,
        duration_secs: total_duration.round() as u64,
        retries,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetCapKind {
    Cost,
    Duration,
    Retries,
}

impl BudgetCapKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cost => "cost",
            Self::Duration => "duration",
            Self::Retries => "retries",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetBreach {
    pub cap: BudgetCapKind,
    pub cap_name: String,
    pub spent: f64,
    pub limit: f64,
    pub message: String,
    pub action_command: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BudgetCheck {
    Ok,
    Breach(BudgetBreach),
}

pub fn check(run_id: &str, budget: &RunBudget, spent: &Spent, before_job: bool) -> BudgetCheck {
    if let Some(limit) = budget.max_duration_secs {
        let breached = if limit == 0 {
            spent.duration_secs > 0
        } else {
            spent.duration_secs >= limit
        };
        if breached {
            let suggested = limit.saturating_mul(2).max(limit + 600);
            return BudgetCheck::Breach(BudgetBreach {
                cap: BudgetCapKind::Duration,
                cap_name: "duration".to_string(),
                spent: spent.duration_secs as f64,
                limit: limit as f64,
                message: format!(
                    "duration cap reached ({}s spent / {}s limit)",
                    spent.duration_secs, limit
                ),
                action_command: format!(
                    "tandem factory run budget {run_id} --max-duration {suggested}"
                ),
            });
        }
    }

    if let Some(limit) = budget.max_cost_usd
        && let Some(cost) = spent.cost_usd
    {
        let breached = if limit == 0.0 {
            cost > 0.0
        } else {
            cost >= limit
        };
        if breached {
            let suggested = (limit * 2.0).max(limit + 1.0);
            return BudgetCheck::Breach(BudgetBreach {
                cap: BudgetCapKind::Cost,
                cap_name: "cost".to_string(),
                spent: cost,
                limit,
                message: format!("cost cap reached (${cost:.2} spent / ${limit:.2} limit)"),
                action_command: format!(
                    "tandem factory run budget {run_id} --max-cost {suggested:.2}"
                ),
            });
        }
    }

    if before_job
        && let Some(limit) = budget.max_retries
        && spent.retries > limit
    {
        let suggested = limit.saturating_add(1);
        return BudgetCheck::Breach(BudgetBreach {
            cap: BudgetCapKind::Retries,
            cap_name: "retries".to_string(),
            spent: spent.retries as f64,
            limit: limit as f64,
            message: format!(
                "retry cap reached ({} spent / {} limit)",
                spent.retries, limit
            ),
            action_command: format!("tandem factory run budget {run_id} --max-retries {suggested}"),
        });
    }

    BudgetCheck::Ok
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EstimateRange<T> {
    Known { min: T, max: T, basis: String },
    Unknown { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Estimate {
    pub duration_secs: EstimateRange<u64>,
    pub cost_usd: EstimateRange<f64>,
}

impl Estimate {
    pub fn exceeds_cap(&self, budget: &RunBudget) -> Option<String> {
        if let (EstimateRange::Known { max, .. }, Some(cap)) =
            (&self.duration_secs, budget.max_duration_secs)
            && *max > cap
        {
            return Some(format!(
                "estimated duration up to {max}s exceeds {cap}s cap"
            ));
        }
        if let (EstimateRange::Known { max, .. }, Some(cap)) = (&self.cost_usd, budget.max_cost_usd)
            && *max > cap
        {
            return Some(format!(
                "estimated cost up to ${max:.2} exceeds ${cap:.2} cap"
            ));
        }
        None
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistoricalRun {
    pub workflow: String,
    pub duration_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageTokenEstimate {
    pub min_input: u64,
    pub max_input: u64,
    pub min_output: u64,
    pub max_output: u64,
}

impl Default for StageTokenEstimate {
    fn default() -> Self {
        Self {
            min_input: 10_000,
            max_input: 40_000,
            min_output: 2_000,
            max_output: 8_000,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EstimateHistory {
    pub runs: Vec<HistoricalRun>,
    pub stage_tokens: BTreeMap<String, StageTokenEstimate>,
}

impl EstimateHistory {
    pub fn from_events(events: &[Event]) -> Self {
        let mut run_workflows: HashMap<String, String> = HashMap::new();
        let mut stage_starts: HashMap<(String, String), f64> = HashMap::new();
        let mut run_durations: HashMap<String, f64> = HashMap::new();
        let mut stage_tokens: BTreeMap<String, StageTokenEstimate> = BTreeMap::new();

        for ev in events {
            let Some(run_id) = ev.run.as_deref() else {
                continue;
            };
            match ev.r#type.as_str() {
                "run.created" => {
                    if let Some(wf) = ev.data.get("workflow").and_then(|v| v.as_str()) {
                        run_workflows.insert(run_id.to_string(), wf.to_string());
                    }
                }
                "stage.started" => {
                    if let Some(stage) = ev.data.get("stage").and_then(|v| v.as_str()) {
                        stage_starts.insert((run_id.to_string(), stage.to_string()), ev.ts);
                    }
                }
                "stage.succeeded" | "stage.failed" | "stage.interrupted" | "stage.skipped" => {
                    if let Some(stage) = ev.data.get("stage").and_then(|v| v.as_str())
                        && let Some(t0) =
                            stage_starts.remove(&(run_id.to_string(), stage.to_string()))
                    {
                        *run_durations.entry(run_id.to_string()).or_insert(0.0) +=
                            (ev.ts - t0).max(0.0);
                    }
                }
                "telemetry.usage" | "job.usage" | "stage.usage" => {
                    if let (Some(stage), Some(in_tok), Some(out_tok)) = (
                        ev.data.get("stage").and_then(|v| v.as_str()),
                        ev.data.get("input_tokens").and_then(|v| v.as_u64()),
                        ev.data.get("output_tokens").and_then(|v| v.as_u64()),
                    ) {
                        stage_tokens
                            .entry(stage.to_string())
                            .and_modify(|e| {
                                e.min_input = e.min_input.min(in_tok);
                                e.max_input = e.max_input.max(in_tok);
                                e.min_output = e.min_output.min(out_tok);
                                e.max_output = e.max_output.max(out_tok);
                            })
                            .or_insert(StageTokenEstimate {
                                min_input: in_tok,
                                max_input: in_tok,
                                min_output: out_tok,
                                max_output: out_tok,
                            });
                    }
                }
                _ => {}
            }
        }

        let runs = run_workflows
            .into_iter()
            .filter_map(|(run_id, workflow)| {
                let dur = run_durations.get(&run_id).copied().unwrap_or(0.0).round() as u64;
                (dur > 0).then_some(HistoricalRun {
                    workflow,
                    duration_secs: dur,
                })
            })
            .collect();

        Self { runs, stage_tokens }
    }
}

pub fn estimate(
    workflow: &Workflow,
    routes: &BTreeMap<String, String>,
    price_table: &BTreeMap<String, ModelPrice>,
    history: &EstimateHistory,
) -> Estimate {
    let matching_durations: Vec<u64> = history
        .runs
        .iter()
        .filter(|r| r.workflow == workflow.name && r.duration_secs > 0)
        .map(|r| r.duration_secs)
        .collect();

    let duration_secs = if matching_durations.is_empty() {
        EstimateRange::Unknown {
            reason: format!("no earlier runs for workflow {}", workflow.name),
        }
    } else {
        let mut min_d = *matching_durations.iter().min().unwrap();
        let mut max_d = *matching_durations.iter().max().unwrap();
        if min_d == max_d {
            // Ensure an estimate is always a range, never a single number.
            let delta = (min_d / 5).max(1);
            min_d = min_d.saturating_sub(delta).max(1);
            max_d = max_d.saturating_add(delta);
        }
        EstimateRange::Known {
            min: min_d,
            max: max_d,
            basis: format!(
                "{} earlier run(s) of {}",
                matching_durations.len(),
                workflow.name
            ),
        }
    };

    let cost_usd = if price_table.is_empty() {
        EstimateRange::Unknown {
            reason: "no price table".to_string(),
        }
    } else if matching_durations.is_empty() && history.stage_tokens.is_empty() {
        EstimateRange::Unknown {
            reason: format!("no history for workflow {}", workflow.name),
        }
    } else {
        let mut min_cost = 0.0f64;
        let mut max_cost = 0.0f64;
        let mut priced_stages = 0usize;

        for stage in &workflow.stages {
            if let Some(model) = routes.get(&stage.name)
                && let Some(price) = price_table.get(model)
            {
                let tok = history
                    .stage_tokens
                    .get(&stage.name)
                    .cloned()
                    .unwrap_or_default();
                let lo = (tok.min_input as f64 / 1_000_000.0) * price.input
                    + (tok.min_output as f64 / 1_000_000.0) * price.output;
                let hi = (tok.max_input as f64 / 1_000_000.0) * price.input
                    + (tok.max_output as f64 / 1_000_000.0) * price.output;
                min_cost += lo.min(hi);
                max_cost += lo.max(hi);
                priced_stages += 1;
            }
        }

        if priced_stages == 0 {
            EstimateRange::Unknown {
                reason: "no routed models present in price table".to_string(),
            }
        } else {
            if (max_cost - min_cost).abs() < f64::EPSILON && max_cost > 0.0 {
                min_cost *= 0.8;
                max_cost *= 1.2;
            } else if max_cost == 0.0 {
                max_cost = 0.01;
            }
            EstimateRange::Known {
                min: min_cost,
                max: max_cost,
                basis: format!("{priced_stages} stage(s) priced from price_table"),
            }
        }
    };

    Estimate {
        duration_secs,
        cost_usd,
    }
}

pub fn routes_for_workflow(
    workflow: &Workflow,
    settings: &FactorySettings,
    ready_clis: &[&str],
    preferred_provider: Option<String>,
) -> (BTreeMap<String, String>, Option<String>) {
    let mut routes = BTreeMap::new();
    let mut primary_provider = None;
    let ctx = RoutingContext {
        preferred_provider,
        is_ui_work: false,
        avoid_provider: None,
        avoid_model: None,
        ..RoutingContext::default()
    };
    for stage in &workflow.stages {
        let grant =
            super::permissions::grant(stage.agent, stage, "workspace", workflow, settings, true)
                .unwrap_or_else(|_| {
                    super::permissions::ToolGrant::from_tools(
                        stage.agent,
                        &stage.name,
                        "workspace",
                        stage.tools.clone(),
                    )
                });
        if let Ok(r) = route_interactive(stage, &grant, workflow, settings, ready_clis, 1, &ctx) {
            if primary_provider.is_none() {
                primary_provider = Some(r.provider.clone());
            }
            routes.insert(stage.name.clone(), r.model);
        }
    }
    (routes, primary_provider)
}

pub fn provider_reports_usage(provider: &str) -> bool {
    crate::adapters::reports_usage(provider)
}

pub fn cost_cap_label(budget: &RunBudget, provider: Option<&str>) -> String {
    match (budget.max_cost_usd, provider) {
        (Some(_), Some(p)) if !provider_reports_usage(p) => {
            format!("not enforceable for {p}")
        }
        (Some(c), _) => format!("${c:.2}"),
        (None, _) => "unlimited".to_string(),
    }
}

pub fn record_budget_changed_needed(run: &Run) -> bool {
    matches!(
        run.state,
        super::model::RunState::Queued
            | super::model::RunState::Running
            | super::model::RunState::Paused
    )
}

pub fn format_duration_minutes_or_secs(secs: u64) -> String {
    if secs >= 60 && secs.is_multiple_of(60) {
        format!("{} min", secs / 60)
    } else {
        format!("{secs}s")
    }
}

pub fn remaining_summary(run_budget: &RunBudget, spent: &Spent) -> (Option<f64>, Option<u64>) {
    let rem_cost = run_budget
        .max_cost_usd
        .map(|c| (c - spent.cost_usd.unwrap_or(0.0)).max(0.0));
    let rem_dur = run_budget
        .max_duration_secs
        .map(|d| d.saturating_sub(spent.duration_secs));
    (rem_cost, rem_dur)
}

pub fn stage_is_terminal(state: StageState) -> bool {
    matches!(
        state,
        StageState::Succeeded | StageState::Failed | StageState::Skipped
    )
}
