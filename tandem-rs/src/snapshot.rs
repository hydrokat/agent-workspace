use crate::coordination::schema::{Claim, Query, STALE_SECONDS, Session};
use crate::coordination::store::CoordStore;
use crate::sources::codebases::{Codebase, list_codebases};
use crate::sources::conflicts::{Conflict, find_conflicts};
use crate::sources::gitinfo::{GitInfo, git_info};
use crate::sources::inprogress::{InProgressEntry, parse_in_progress};
use crate::sources::specs::{Phase, load_phases};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
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

    StatusSnapshot {
        phases: load_phases(workspace),
        entries: parse_in_progress(&workspace.join("specs").join("in-progress.md")),
        sessions: store.sessions(),
        claims: claims.clone(),
        queries: store.queries(),
        conflicts: find_conflicts(&claims),
        codebases,
        workspace_git: git_info(workspace, git_timeout),
        codebase_git,
        coord_exists: store.exists(),
        now,
    }
}
