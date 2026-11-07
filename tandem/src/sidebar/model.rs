use std::collections::HashSet;

use crate::coordination::schema::{Query, Session};
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
        cli: String,
        model: String,
        glyph: char,
        is_active: bool,
        is_in_grid: bool,
        has_query: bool,
    },
    AddAgent,
    Empty,
}

impl SidebarRow {
    pub fn is_selectable(&self) -> bool {
        matches!(self, SidebarRow::Agent { .. } | SidebarRow::AddAgent)
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
    let mut planners: Vec<&PaneInfo> = panes
        .iter()
        .filter(|p| p.kind == "agent" && p.role == "planner")
        .collect();
    let mut implementers: Vec<&PaneInfo> = panes
        .iter()
        .filter(|p| p.kind == "agent" && p.role == "implementer")
        .collect();

    planners.sort_by_key(|p| p.index);
    implementers.sort_by_key(|p| p.index);

    if planners.is_empty() && implementers.is_empty() {
        return vec![SidebarRow::Empty, SidebarRow::AddAgent];
    }

    let open_query_sessions: HashSet<&str> = open_queries
        .iter()
        .filter(|q| q.status == "open")
        .map(|q| q.asked_by.as_str())
        .collect();

    let mut rows = Vec::new();

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

    SidebarRow::Agent {
        pane_id: p.id.clone(),
        session_id,
        role: p.role.clone(),
        index: p.index,
        label: display_label,
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
