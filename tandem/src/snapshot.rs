pub use crate::coordination::schema::{Claim, Query, STALE_SECONDS, Session};
use crate::coordination::store::CoordStore;
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SprintPhaseView {
    pub name: String,
    pub moscow: String,
    pub effort: f64,
    pub done: usize,
    pub total: usize,
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

pub fn collect(workspace: &Path, store: &CoordStore, git_timeout: Duration) -> StatusSnapshot {
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

    let ongoing_specs = ongoing_specs(&phases, &claims, &entries, raw_sprint.as_ref());
    let ongoing_tasks = ongoing_tasks(&phases, &claims, &sessions, &entries, now);

    StatusSnapshot {
        phases,
        entries,
        sessions,
        claims: claims.clone(),
        queries: store.queries(),
        conflicts: find_conflicts(&claims),
        codebases,
        workspace_git: git_info(workspace, git_timeout),
        codebase_git,
        coord_exists: store.exists(),
        now,
        sprint,
        ongoing_specs,
        ongoing_tasks,
    }
}
