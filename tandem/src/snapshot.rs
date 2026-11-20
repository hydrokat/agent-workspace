pub use crate::coordination::schema::{Claim, Query, STALE_SECONDS, Session};
use crate::coordination::store::CoordStore;
use crate::factory::model::{IdeaState, Risk};
use crate::factory::store::FactoryStore;
use crate::sources::codebases::{Codebase, list_codebases};
use crate::sources::conflicts::{Conflict, find_conflicts};
use crate::sources::gitinfo::{GitInfo, git_info};
use crate::sources::inprogress::{InProgressEntry, parse_in_progress};
use crate::sources::ongoing::{OngoingSpec, OngoingTask, ongoing_specs, ongoing_tasks};
use crate::sources::specs::{Phase, load_phases};
use crate::sources::sprints::{Sprint, load_active_sprint};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SprintPhaseView {
    pub name: String,
    pub moscow: String,
    pub effort: f64,
    pub done: usize,
    pub total: usize,
    #[serde(default)]
    pub archived: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SprintView {
    pub id: String,
    pub title: String,
    pub status: String,
    pub start_date: String,
    pub end_date: String,
    pub capacity_days: f64,
    pub committed_days: f64,
    pub overloaded: bool,
    pub done: usize,
    pub total: usize,
    pub must_remaining: usize,
    pub phases: Vec<SprintPhaseView>,
}

impl SprintView {
    pub fn from_sprint(sprint: Sprint, phases: &[Phase]) -> Self {
        let views: Vec<SprintPhaseView> = sprint
            .phases
            .iter()
            .map(|sp| {
                let found = phases.iter().find(|p| p.name == sp.name);
                SprintPhaseView {
                    name: sp.name.clone(),
                    moscow: sp.moscow.clone(),
                    effort: sp.effort,
                    done: found.map(|p| p.done()).unwrap_or(0),
                    total: found.map(|p| p.total()).unwrap_or(0),
                    archived: found.map(|p| p.archived).unwrap_or(false),
                }
            })
            .collect();
        let must_remaining = views
            .iter()
            .filter(|p| p.moscow == "Must Have")
            .map(|p| p.total - p.done)
            .sum();
        Self {
            overloaded: sprint.committed_days > sprint.capacity_days,
            done: views.iter().map(|p| p.done).sum(),
            total: views.iter().map(|p| p.total).sum(),
            must_remaining,
            id: sprint.id,
            title: sprint.title,
            status: sprint.status,
            start_date: sprint.start_date,
            end_date: sprint.end_date,
            capacity_days: sprint.capacity_days,
            committed_days: sprint.committed_days,
            phases: views,
        }
    }
}

fn default_job_limit() -> usize {
    3
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageSummary {
    pub name: String,
    pub agent: String,
    pub state: crate::factory::model::StageState,
    #[serde(default)]
    pub attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRoundSummary {
    pub round: u32,
    pub verdict: crate::factory::stages::review::ReviewVerdict,
    pub total_findings: usize,
    pub open_findings: usize,
    pub fixed_findings: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunSummary {
    pub id: String,
    pub state: crate::factory::model::RunState,
    pub stages_done: usize,
    pub stages_total: usize,
    #[serde(default)]
    pub stages: Vec<StageSummary>,
    #[serde(default)]
    pub running_jobs: usize,
    #[serde(default = "default_job_limit")]
    pub job_limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_secs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<crate::factory::telemetry::Cost>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_display: Option<String>,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_complete: Option<bool>,
    #[serde(default)]
    pub evidence_missing: Vec<String>,
    #[serde(default)]
    pub review_rounds: Vec<ReviewRoundSummary>,
    #[serde(default)]
    pub prs: Vec<crate::factory::model::PrLink>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdeaCard {
    pub id: String,
    pub title: String,
    pub state: IdeaState,
    pub owner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<Risk>,
    #[serde(default)]
    pub blockers: usize,
    pub updated: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<RunSummary>,
    #[serde(default)]
    pub team_count: usize,
    #[serde(default)]
    pub needs_sign_in: bool,
}

pub fn pause_reason_needs_sign_in(reason: &str) -> bool {
    let lower = reason.to_ascii_lowercase();
    lower.contains("signed in")
        || lower.contains("sign in")
        || lower.contains("not installed")
        || lower.contains("login")
        || lower.contains("gate")
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FactorySummary {
    pub enabled: bool,
    #[serde(default)]
    pub ideas: Vec<IdeaCard>,
    #[serde(default)]
    pub needs_you: usize,
    #[serde(default)]
    pub by_state: HashMap<String, usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatusSnapshot {
    #[serde(default)]
    pub phases: Vec<Phase>,
    #[serde(default)]
    pub entries: Vec<InProgressEntry>,
    #[serde(default)]
    pub sessions: Vec<Session>,
    #[serde(default)]
    pub claims: Vec<Claim>,
    #[serde(default)]
    pub queries: Vec<Query>,
    #[serde(default)]
    pub conflicts: Vec<Conflict>,
    #[serde(default)]
    pub codebases: Vec<Codebase>,
    #[serde(default)]
    pub workspace_git: Option<GitInfo>,
    #[serde(default)]
    pub codebase_git: HashMap<String, Option<GitInfo>>,
    #[serde(default)]
    pub coord_exists: bool,
    #[serde(default)]
    pub now: f64,
    #[serde(default)]
    pub sprint: Option<SprintView>,
    #[serde(default)]
    pub ongoing_specs: Vec<OngoingSpec>,
    #[serde(default)]
    pub ongoing_tasks: Vec<OngoingTask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factory: Option<FactorySummary>,
}

impl StatusSnapshot {
    pub fn open_queries(&self) -> usize {
        self.queries.iter().filter(|q| q.status == "open").count()
    }

    pub fn stale(&self, heartbeat: f64) -> bool {
        self.now - heartbeat > STALE_SECONDS as f64
    }

    pub fn stale_claims(&self) -> Vec<&Claim> {
        self.claims
            .iter()
            .filter(|c| self.stale(c.heartbeat))
            .collect()
    }
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

fn summarize_run(
    fstore: &FactoryStore,
    r: crate::factory::model::Run,
    workspace_running_jobs: usize,
    job_limit: usize,
    now: f64,
) -> RunSummary {
    use crate::factory::broker::job::{JobInput, job_dir, jobs_dir};
    use crate::factory::broker::result::JobResult;
    use crate::factory::model::{RunState, StageState};

    let stages_total = r.stages.len();
    let stages_done = r
        .stages
        .iter()
        .filter(|s| matches!(s.state, StageState::Succeeded | StageState::Skipped))
        .count();

    let mut stages = Vec::with_capacity(stages_total);
    for s in &r.stages {
        let (provider, model) = if s.attempts > 0 {
            let input_path = job_dir(fstore, &r.id, &s.name, s.attempts).join("input.json");
            std::fs::read_to_string(&input_path)
                .ok()
                .and_then(|raw| serde_json::from_str::<JobInput>(&raw).ok())
                .map(|inp| (Some(inp.provider), Some(inp.model)))
                .unwrap_or((None, None))
        } else {
            (None, None)
        };
        stages.push(StageSummary {
            name: s.name.clone(),
            agent: s.agent.clone(),
            state: s.state,
            attempt: s.attempts,
            provider,
            model,
            started: s.started,
            finished: s.finished,
        });
    }

    let cost = if let Some(ref c) = r.cost {
        c.clone()
    } else {
        let jdir = jobs_dir(fstore, &r.id);
        let mut total_cost: Option<f64> = None;
        if jdir.is_dir()
            && let Ok(entries) = std::fs::read_dir(&jdir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let res_path = path.join("result.json");
                if let Ok(raw) = std::fs::read_to_string(&res_path)
                    && let Ok(res) = serde_json::from_str::<JobResult>(&raw)
                    && let Some(c) = res.usage.and_then(|u| u.cost_usd)
                {
                    total_cost = Some(total_cost.unwrap_or(0.0) + c);
                }
            }
        }
        match total_cost {
            Some(c) => crate::factory::telemetry::Cost {
                value: Some(c),
                basis: crate::factory::telemetry::CostBasis::Actual,
            },
            None => crate::factory::telemetry::Cost::unknown(),
        }
    };
    let total_cost = if cost.basis == crate::factory::telemetry::CostBasis::Unknown {
        None
    } else {
        cost.value
    };
    let cost_display = Some(cost.format());
    let retry_count: u32 = r
        .stages
        .iter()
        .map(|s| s.attempts.saturating_sub(1 + s.interrupted_attempts))
        .sum();
    let running_jobs = workspace_running_jobs;

    let start_ts = r
        .stages
        .iter()
        .filter_map(|s| s.started)
        .fold(None, |acc: Option<f64>, t| {
            Some(acc.map_or(t, |a| a.min(t)))
        })
        .unwrap_or(r.created);
    let end_ts = if matches!(
        r.state,
        RunState::Queued | RunState::Running | RunState::Paused
    ) && now > 0.0
    {
        now
    } else {
        r.updated
    };
    let elapsed_secs = Some((end_ts - start_ts).max(0.0));
    let pause_reason = if r.state == RunState::Paused {
        r.pause_reason.clone()
    } else {
        None
    };

    let evidence_summary = Some(crate::factory::evidence::bundle_summary(&r.evidence));
    let mut evidence_complete = None;
    let mut evidence_missing = Vec::new();
    let run_wf_path = fstore.run_dir(&r.id).join("workflow.yaml");
    let raw_wf = if run_wf_path.exists() {
        crate::factory::workflow::parse::load_workflow_file(&run_wf_path).ok()
    } else {
        fstore
            .root
            .parent()
            .and_then(|p| p.parent())
            .and_then(|ws| crate::factory::workflow::load_default_workflow(ws).ok())
            .map(|(wf, _)| wf)
    };
    if let Some(raw) = raw_wf {
        let settings = fstore.settings().unwrap_or_default();
        if let Ok(wf) = crate::factory::workflow::validate::validate(&raw, &settings) {
            let missing = crate::factory::evidence::missing(&r, &wf);
            evidence_complete = Some(missing.is_empty());
            evidence_missing = missing.iter().map(|k| k.as_str().to_string()).collect();
        }
    }

    let rounds_report = crate::factory::evidence::load_review_rounds(fstore, &r.id);
    let review_rounds = rounds_report
        .rounds
        .into_iter()
        .map(|rd| {
            let open_findings = rd.findings.len();
            let fixed_findings = rd.fixed_from_previous.len();
            ReviewRoundSummary {
                round: rd.round,
                verdict: rd.stated_verdict,
                total_findings: open_findings + fixed_findings,
                open_findings,
                fixed_findings,
            }
        })
        .collect();

    RunSummary {
        id: r.id,
        state: r.state,
        stages_done,
        stages_total,
        stages,
        running_jobs,
        job_limit,
        elapsed_secs,
        cost_usd: total_cost,
        cost: Some(cost),
        cost_display,
        retry_count,
        pause_reason,
        evidence_summary,
        evidence_complete,
        evidence_missing,
        review_rounds,
        prs: r.prs,
    }
}

pub fn collect_factory_summary(workspace: &Path, queries: &[Query], now: f64) -> FactorySummary {
    let fstore = FactoryStore::new(workspace);
    match fstore.list_ideas() {
        Ok(ideas) => {
            let mut by_state = HashMap::new();
            let mut cards = Vec::with_capacity(ideas.len());
            let mut needs_you = 0usize;
            let mut job_limit_cache: Option<usize> = None;
            let mut workspace_running_cache: Option<usize> = None;
            for idea in ideas {
                *by_state
                    .entry(state_key(idea.state).to_string())
                    .or_insert(0) += 1;
                let iq = crate::factory::questions::for_idea(queries, &idea.id);
                let run = idea.runs.last().and_then(|run_id| {
                    let limit = *job_limit_cache.get_or_insert_with(|| {
                        fstore
                            .settings()
                            .map(|s| s.max_parallel_agents.max(1))
                            .unwrap_or_else(|_| default_job_limit())
                    });
                    let workspace_running = *workspace_running_cache.get_or_insert_with(|| {
                        crate::factory::runner::count_workspace_running_jobs(&fstore)
                    });
                    crate::factory::run::get_run(&fstore, run_id)
                        .ok()
                        .flatten()
                        .map(|r| summarize_run(&fstore, r, workspace_running, limit, now))
                });
                let run_paused = run
                    .as_ref()
                    .is_some_and(|r| r.state == crate::factory::model::RunState::Paused);
                let running_job_count = run
                    .as_ref()
                    .map(|r| {
                        let jdir = crate::factory::broker::job::jobs_dir(&fstore, &r.id);
                        let mut count = 0usize;
                        if jdir.is_dir()
                            && let Ok(entries) = std::fs::read_dir(&jdir)
                        {
                            for entry in entries.flatten() {
                                let p = entry.path();
                                if p.is_dir()
                                    && matches!(
                                        crate::factory::broker::job::read_job_state(&p),
                                        Ok(crate::factory::broker::job::JobState::Running)
                                    )
                                {
                                    count += 1;
                                }
                            }
                        }
                        let stage_count = r
                            .stages
                            .iter()
                            .filter(|s| s.state == crate::factory::model::StageState::Running)
                            .count();
                        count.max(stage_count)
                    })
                    .unwrap_or(0);
                let team_count = running_job_count;
                let needs_sign_in = run.as_ref().is_some_and(|r| {
                    r.state == crate::factory::model::RunState::Paused
                        && r.pause_reason
                            .as_deref()
                            .is_some_and(pause_reason_needs_sign_in)
                });
                if iq.open > 0 || run_paused {
                    needs_you += 1;
                }
                cards.push(IdeaCard {
                    id: idea.id,
                    title: idea.title,
                    state: idea.state,
                    owner: idea.owner,
                    risk: idea.risk,
                    blockers: iq.blocking,
                    updated: idea.updated,
                    run,
                    team_count,
                    needs_sign_in,
                });
            }
            FactorySummary {
                enabled: true,
                ideas: cards,
                needs_you,
                by_state,
                error: None,
            }
        }
        Err(err) => {
            let reason = err
                .to_string()
                .lines()
                .next()
                .unwrap_or("unknown error")
                .to_string();
            FactorySummary {
                enabled: true,
                ideas: Vec::new(),
                needs_you: 0,
                by_state: HashMap::new(),
                error: Some(reason),
            }
        }
    }
}

pub fn collect(workspace: &Path, store: &CoordStore, git_timeout: Duration) -> StatusSnapshot {
    collect_with_factory(workspace, store, git_timeout, false)
}

pub fn collect_with_factory(
    workspace: &Path,
    store: &CoordStore,
    git_timeout: Duration,
    factory_enabled: bool,
) -> StatusSnapshot {
    let codebases = list_codebases(workspace);
    let claims = store.claims();
    let codebase_git = codebases
        .iter()
        .filter(|c| c.is_git)
        .map(|c| (c.name.clone(), git_info(&c.path, git_timeout)))
        .collect();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);

    let phases = load_phases(workspace);
    let raw_sprint = load_active_sprint(workspace);
    let sprint = raw_sprint
        .as_ref()
        .map(|s| SprintView::from_sprint(s.clone(), &phases));
    let entries = parse_in_progress(&workspace.join("specs").join("in-progress.md"));
    let sessions = store.sessions();

    let queries = store.queries();
    let ongoing_specs = ongoing_specs(&phases, &claims, &entries, raw_sprint.as_ref());
    let ongoing_tasks = ongoing_tasks(&phases, &claims, &sessions, &entries, now);
    let factory = if factory_enabled {
        Some(collect_factory_summary(workspace, &queries, now))
    } else {
        None
    };

    StatusSnapshot {
        phases,
        entries,
        sessions,
        claims: claims.clone(),
        queries,
        conflicts: find_conflicts(&claims),
        codebases,
        workspace_git: git_info(workspace, git_timeout),
        codebase_git,
        coord_exists: store.exists(),
        now,
        sprint,
        ongoing_specs,
        ongoing_tasks,
        factory,
    }
}
