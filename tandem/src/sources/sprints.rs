use crate::sanitize::clean;
use crate::sources::specs::read_file_capped;
use serde::{Deserialize, Serialize};
use std::path::Path;

const DEFAULT_CAPACITY_DAYS: f64 = 5.0;
const WONT_HAVE: &str = "Won't Have";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SprintPhase {
    pub name: String,
    pub moscow: String,
    pub effort: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sprint {
    pub id: String,
    pub title: String,
    pub status: String,
    pub start_date: String,
    pub end_date: String,
    pub capacity_days: f64,
    pub committed_days: f64,
    pub phases: Vec<SprintPhase>,
}

fn is_sprint_dir(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("s-") else {
        return false;
    };
    let (digits, slug) = match rest.split_once('-') {
        Some((d, s)) => (d, Some(s)),
        None => (rest, None),
    };
    if digits.len() != 3 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    match slug {
        None => true,
        Some(s) => {
            let mut chars = s.chars();
            let first_ok = chars
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
            first_ok
                && s.len() <= 48
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        }
    }
}

fn leading_number(value: &str) -> Option<f64> {
    let first = value.split_whitespace().next()?;
    first.trim_end_matches('d').parse().ok()
}

fn bullet_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.strip_prefix("- **")?.strip_prefix(key)?;
    let rest = rest.strip_prefix("**:")?;
    Some(rest.trim())
}

fn parse_row(line: &str) -> Option<SprintPhase> {
    let cols: Vec<&str> = line
        .trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect();
    if cols.len() < 8 {
        return None;
    }
    let numbers: Option<Vec<f64>> = cols[3..6]
        .iter()
        .map(|c| c.parse::<f64>().ok())
        .chain(std::iter::once(
            cols[6].trim_end_matches('d').parse::<f64>().ok(),
        ))
        .chain(std::iter::once(cols[7].parse::<f64>().ok()))
        .collect();
    let effort = numbers.map(|n| n[3]).unwrap_or(1.0);
    Some(SprintPhase {
        name: clean(cols[0]),
        moscow: clean(cols[2]),
        effort,
    })
}

/// Parses `sprint.md` the same way `sprints.py parse_sprint_md` does: the scope table rows are the single
/// source of committed effort, and the `Committed Capacity` line is used only when the table is empty.
pub fn parse_sprint_md(text: &str, dir_name: &str) -> Option<Sprint> {
    let first = text.lines().next().unwrap_or("");
    let title = if first.starts_with("# Sprint ") {
        first
            .split_once(':')
            .map(|(_, t)| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| dir_name.to_string())
    } else {
        dir_name.to_string()
    };
    let mut status = "open".to_string();
    let mut start_date = String::new();
    let mut end_date = String::new();
    let mut capacity_days = DEFAULT_CAPACITY_DAYS;
    let mut declared: Option<f64> = None;
    let mut phases: Vec<SprintPhase> = Vec::new();
    let mut in_scope = false;
    let mut in_other = false;

    for line in text.lines() {
        let stripped = line.trim();
        if stripped.starts_with("## Metadata") {
            in_scope = false;
            in_other = false;
            continue;
        }
        if stripped.starts_with("## Prioritized Scope") || stripped.starts_with("## Scope") {
            in_scope = true;
            in_other = false;
            continue;
        }
        if stripped.starts_with("## ") {
            in_scope = false;
            in_other =
                stripped.starts_with("## Lock & Audit Log") || stripped.starts_with("## Audit Log");
            continue;
        }
        if in_scope {
            if stripped.starts_with('|')
                && !stripped.starts_with("|---")
                && !stripped.starts_with("| Phase")
                && let Some(row) = parse_row(stripped)
            {
                phases.push(row);
            }
            continue;
        }
        if in_other {
            continue;
        }
        if let Some(v) = bullet_value(stripped, "Status") {
            status = v.to_lowercase();
        } else if let Some(v) = bullet_value(stripped, "Start Date") {
            start_date = clean(v);
        } else if let Some(v) = bullet_value(stripped, "End Date") {
            end_date = clean(v);
        } else if let Some(v) = bullet_value(stripped, "AI-Assisted Capacity Limit") {
            capacity_days = leading_number(v).unwrap_or(DEFAULT_CAPACITY_DAYS);
        } else if let Some(v) = bullet_value(stripped, "Committed Capacity") {
            declared = leading_number(v);
        }
    }

    let committed_days = if phases.is_empty() {
        declared.unwrap_or(0.0)
    } else {
        phases
            .iter()
            .filter(|p| p.moscow != WONT_HAVE)
            .map(|p| p.effort)
            .sum()
    };
    Some(Sprint {
        id: dir_name.to_string(),
        title: clean(&title),
        status,
        start_date,
        end_date,
        capacity_days,
        committed_days,
        phases,
    })
}

/// The active sprint is the last `open` or `locked` sprint in sorted directory order, like `sprints.py resolve`.
pub fn load_active_sprint(workspace: &Path) -> Option<Sprint> {
    let mut names: Vec<String> = std::fs::read_dir(workspace.join("sprints"))
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| is_sprint_dir(n))
        .collect();
    names.sort();
    let mut active = None;
    for name in names {
        let Some(text) = read_file_capped(&workspace.join("sprints").join(&name).join("sprint.md"))
        else {
            continue;
        };
        if let Some(sprint) = parse_sprint_md(&text, &name)
            && (sprint.status == "open" || sprint.status == "locked")
        {
            active = Some(sprint);
        }
    }
    active
}
