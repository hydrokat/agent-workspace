use std::collections::HashSet;

use crate::coordination::schema::{Query, Session};
use crate::factory::model::RunState;
use crate::snapshot::FactorySummary;
use crate::tmux::tabs::PaneInfo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarRow {
    Header {
        title: String,
        count: usize,
    },
    Agent {
        pane_id: String,
        session_id: String,
        role: String,
        index: usize,
        label: String,
        name: String,
        cli: String,
        model: String,
        glyph: char,
        is_active: bool,
        is_in_grid: bool,
        has_query: bool,
    },
    NewIdea,
    Run {
        run_id: String,
        idea_id: String,
        label: String,
    },
    AddAgent,
    Empty,
}

impl SidebarRow {
    pub fn is_selectable(&self) -> bool {
        matches!(
            self,
            SidebarRow::Agent { .. }
                | SidebarRow::AddAgent
                | SidebarRow::NewIdea
                | SidebarRow::Run { .. }
        )
    }

    pub fn pane_id(&self) -> Option<&str> {
        match self {
            SidebarRow::Agent { pane_id, .. } => Some(pane_id.as_str()),
            _ => None,
        }
    }
}

pub fn rows_from(
    panes: &[PaneInfo],
    sessions: &[Session],
    open_queries: &[Query],
    active_pane: Option<&str>,
    selected: &[String],
) -> Vec<SidebarRow> {
    rows_from_for(panes, sessions, open_queries, active_pane, selected, None)
}

fn short_id(id: &str) -> String {
    id.chars().take(6).collect()
}

fn run_state_label(state: &RunState) -> &'static str {
    match state {
        RunState::Pending => "pending",
        RunState::Queued => "queued",
        RunState::Running => "running",
        RunState::Paused => "paused",
        RunState::Review => "review",
        RunState::Succeeded => "succeeded",
        RunState::Delivered => "delivered",
        RunState::Failed => "failed",
        RunState::Cancelled => "cancelled",
        RunState::Withdrawn => "withdrawn",
    }
}

fn is_active_run(state: &RunState) -> bool {
    matches!(
        state,
        RunState::Pending
            | RunState::Queued
            | RunState::Running
            | RunState::Paused
            | RunState::Review
    )
}

fn factory_rows(
    panes: &[PaneInfo],
    sessions: &[Session],
    open_queries: &[Query],
    active_pane: Option<&str>,
    selected: &[String],
    summary: &FactorySummary,
) -> Vec<SidebarRow> {
    let mut orchestrators: Vec<&PaneInfo> = panes
        .iter()
        .filter(|p| p.kind == "agent" && p.role == "orchestrator")
        .collect();
    orchestrators.sort_by_key(|p| p.index);

    let open_query_sessions: HashSet<&str> = open_queries
        .iter()
        .filter(|q| q.status == "open")
        .map(|q| q.asked_by.as_str())
        .collect();

    let mut rows = vec![
        SidebarRow::Header {
            title: "FACTORY".to_string(),
            count: 0,
        },
        SidebarRow::NewIdea,
        SidebarRow::Header {
            title: "ORCHESTRATORS".to_string(),
            count: orchestrators.len(),
        },
    ];
    if orchestrators.is_empty() {
        rows.push(SidebarRow::Empty);
    }
    for p in orchestrators {
        let mut row = build_agent_row(p, sessions, &open_query_sessions, active_pane, selected);
        if let SidebarRow::Agent { label, .. } = &mut row
            && let Some(card) = summary.ideas.iter().find(|i| i.id == p.idea)
        {
            *label = format!("{} · {}", short_id(&card.id), card.state.label());
        }
        rows.push(row);
    }

    let runs: Vec<(&str, &str, &RunState)> = summary
        .ideas
        .iter()
        .filter_map(|i| {
            i.run
                .as_ref()
                .map(|r| (i.id.as_str(), r.id.as_str(), &r.state))
        })
        .filter(|(_, _, state)| is_active_run(state))
        .collect();
    if !runs.is_empty() {
        rows.push(SidebarRow::Header {
            title: "ACTIVE RUNS".to_string(),
            count: runs.len(),
        });
        for (idea_id, run_id, state) in runs {
            rows.push(SidebarRow::Run {
                run_id: run_id.to_string(),
                idea_id: idea_id.to_string(),
                label: format!("{} · {}", short_id(run_id), run_state_label(state)),
            });
        }
    }
    rows
}

pub fn rows_from_for(
    panes: &[PaneInfo],
    sessions: &[Session],
    open_queries: &[Query],
    active_pane: Option<&str>,
    selected: &[String],
    factory: Option<&FactorySummary>,
) -> Vec<SidebarRow> {
    if let Some(summary) = factory {
        return factory_rows(
            panes,
            sessions,
            open_queries,
            active_pane,
            selected,
            summary,
        );
    }
    let mut orchestrators: Vec<&PaneInfo> = panes
        .iter()
        .filter(|p| p.kind == "agent" && p.role == "orchestrator")
        .collect();
    let mut planners: Vec<&PaneInfo> = panes
        .iter()
        .filter(|p| p.kind == "agent" && p.role == "planner")
        .collect();
    let mut implementers: Vec<&PaneInfo> = panes
        .iter()
        .filter(|p| p.kind == "agent" && p.role == "implementer")
        .collect();

    orchestrators.sort_by_key(|p| p.index);
    planners.sort_by_key(|p| p.index);
    implementers.sort_by_key(|p| p.index);

    if orchestrators.is_empty() && planners.is_empty() && implementers.is_empty() {
        return vec![SidebarRow::Empty, SidebarRow::AddAgent];
    }

    let open_query_sessions: HashSet<&str> = open_queries
        .iter()
        .filter(|q| q.status == "open")
        .map(|q| q.asked_by.as_str())
        .collect();

    let mut rows = Vec::new();

    if !orchestrators.is_empty() {
        rows.push(SidebarRow::Header {
            title: "ORCHESTRATORS".to_string(),
            count: orchestrators.len(),
        });
        for p in orchestrators {
            rows.push(build_agent_row(
                p,
                sessions,
                &open_query_sessions,
                active_pane,
                selected,
            ));
        }
    }

    if !planners.is_empty() {
        rows.push(SidebarRow::Header {
            title: "PLANNERS".to_string(),
            count: planners.len(),
        });
        for p in planners {
            rows.push(build_agent_row(
                p,
                sessions,
                &open_query_sessions,
                active_pane,
                selected,
            ));
        }
    }

    if !implementers.is_empty() {
        rows.push(SidebarRow::Header {
            title: "IMPLEMENTERS".to_string(),
            count: implementers.len(),
        });
        for p in implementers {
            rows.push(build_agent_row(
                p,
                sessions,
                &open_query_sessions,
                active_pane,
                selected,
            ));
        }
    }

    rows.push(SidebarRow::AddAgent);
    rows
}

fn build_agent_row(
    p: &PaneInfo,
    sessions: &[Session],
    open_query_sessions: &HashSet<&str>,
    active_pane: Option<&str>,
    selected: &[String],
) -> SidebarRow {
    let sess = sessions
        .iter()
        .find(|s| s.pane == p.id || (!p.session.is_empty() && s.id == p.session));

    let session_id = sess
        .map(|s| s.id.clone())
        .unwrap_or_else(|| p.session.clone());

    let cli = sess.map(|s| s.cli.clone()).unwrap_or_else(|| {
        if let Some((first, _)) = p.label.split_once('·') {
            first.to_string()
        } else {
            String::new()
        }
    });

    let model = sess.map(|s| s.model.clone()).unwrap_or_else(|| {
        if let Some((_, second)) = p.label.split_once('·') {
            second.to_string()
        } else {
            String::new()
        }
    });

    let has_query = !session_id.is_empty() && open_query_sessions.contains(session_id.as_str());
    let is_active = match active_pane {
        Some(ap) => ap == p.id,
        None => p.window == "main",
    };
    let is_in_grid = if selected.is_empty() {
        is_active
    } else {
        (!session_id.is_empty() && selected.contains(&session_id)) || selected.contains(&p.id)
    };

    let glyph = if p.dead {
        '✕'
    } else if has_query {
        '?'
    } else if is_active {
        '●'
    } else {
        '○'
    };

    let display_label = if !p.label.is_empty() {
        p.label.clone()
    } else {
        format!("{}-{}", p.role, p.index)
    };

    let name = if !p.name.is_empty() {
        p.name.clone()
    } else if let Some(s) = sess
        && !s.name.is_empty()
    {
        s.name.clone()
    } else {
        String::new()
    };

    SidebarRow::Agent {
        pane_id: p.id.clone(),
        session_id,
        role: p.role.clone(),
        index: p.index,
        label: display_label,
        name,
        cli,
        model,
        glyph,
        is_active,
        is_in_grid,
        has_query,
    }
}

pub fn first_selectable(rows: &[SidebarRow]) -> usize {
    rows.iter().position(|r| r.is_selectable()).unwrap_or(0)
}

pub fn last_selectable(rows: &[SidebarRow]) -> usize {
    rows.iter().rposition(|r| r.is_selectable()).unwrap_or(0)
}

pub fn next_selectable(rows: &[SidebarRow], current: usize) -> usize {
    if rows.is_empty() {
        return 0;
    }
    for (idx, row) in rows.iter().enumerate().skip(current + 1) {
        if row.is_selectable() {
            return idx;
        }
    }
    for (idx, row) in rows.iter().enumerate().take(current + 1) {
        if row.is_selectable() {
            return idx;
        }
    }
    current
}

pub fn prev_selectable(rows: &[SidebarRow], current: usize) -> usize {
    if rows.is_empty() {
        return 0;
    }
    for idx in (0..current).rev() {
        if rows[idx].is_selectable() {
            return idx;
        }
    }
    for idx in (current..rows.len()).rev() {
        if rows[idx].is_selectable() {
            return idx;
        }
    }
    current
}
