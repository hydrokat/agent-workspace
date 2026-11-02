use crate::coordination::schema::{Claim, STALE_SECONDS, Session};
use crate::sources::inprogress::InProgressEntry;
use crate::sources::specs::Phase;
use crate::sources::sprints::Sprint;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OngoingSpec {
    pub phase: Phase,
    #[serde(default)]
    pub next_up: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OngoingTask {
    pub phase: String,
    pub task_id: String,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub objectives_done: usize,
    #[serde(default)]
    pub objectives_total: usize,
    pub claim: Option<Claim>,
    pub session_pane: Option<String>,
    #[serde(default)]
    pub stale: bool,
    #[serde(default)]
    pub status_mismatch: bool,
    pub next_step: Option<String>,
}

pub fn phase_matches(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if let Some(rest) = a.strip_prefix(b)
        && rest.starts_with('-')
    {
        return true;
    }
    if let Some(rest) = b.strip_prefix(a)
        && rest.starts_with('-')
    {
        return true;
    }
    false
}

fn moscow_rank(moscow: &str) -> usize {
    let lower = moscow.to_lowercase();
    if lower.contains("must") {
        0
    } else if lower.contains("should") {
        1
    } else if lower.contains("could") {
        2
    } else if lower.contains("won't") || lower.contains("wont") {
        3
    } else {
        4
    }
}

pub fn ongoing_specs(
    phases: &[Phase],
    claims: &[Claim],
    entries: &[InProgressEntry],
    active_sprint: Option<&Sprint>,
) -> Vec<OngoingSpec> {
    let mut ongoing = Vec::new();
    let mut seen_phases = HashSet::new();

    for phase in phases {
        let has_in_progress = phase
            .tasks
            .iter()
            .any(|t| t.status == "In Progress" || t.status == "In Review");
        let has_claim = claims.iter().any(|c| phase_matches(&c.phase, &phase.name));
        let has_entry = entries.iter().any(|e| phase_matches(&e.spec, &phase.name));

        if has_in_progress || has_claim || has_entry {
            seen_phases.insert(phase.name.clone());
            ongoing.push(OngoingSpec {
                phase: phase.clone(),
                next_up: false,
            });
        }
    }

    for claim in claims {
        let already = seen_phases.iter().any(|p| phase_matches(p, &claim.phase));
        if !already {
            let warning = format!("{}/impl.md missing or unreadable", claim.phase);
            seen_phases.insert(claim.phase.clone());
            ongoing.push(OngoingSpec {
                phase: Phase::with_warning(&claim.phase, warning),
                next_up: false,
            });
        }
    }

    for entry in entries {
        let already = seen_phases.iter().any(|p| phase_matches(p, &entry.spec));
        if !already {
            let warning = format!("{}/impl.md missing or unreadable", entry.spec);
            seen_phases.insert(entry.spec.clone());
            ongoing.push(OngoingSpec {
                phase: Phase::with_warning(&entry.spec, warning),
                next_up: false,
            });
        }
    }

    if !ongoing.is_empty() {
        return ongoing;
    }

    if let Some(sprint) = active_sprint {
        let mut sprint_phases = sprint.phases.clone();
        sprint_phases.sort_by_key(|sp| moscow_rank(&sp.moscow));

        for sp in sprint_phases {
            if let Some(p) = phases.iter().find(|p| phase_matches(&p.name, &sp.name)) {
                let incomplete = p.tasks.is_empty() || p.done() < p.total();
                if incomplete {
                    return vec![OngoingSpec {
                        phase: p.clone(),
                        next_up: true,
                    }];
                }
            } else {
                return vec![OngoingSpec {
                    phase: Phase::with_warning(
                        &sp.name,
                        format!("{}/impl.md missing or unreadable", sp.name),
                    ),
                    next_up: true,
                }];
            }
        }
    }

    Vec::new()
}

fn extract_next_step(
    entries: &[InProgressEntry],
    phase_name: &str,
    task_id: &str,
) -> Option<String> {
    for entry in entries {
        if phase_matches(&entry.spec, phase_name) {
            let mentions_task = entry.fields.values().any(|v| v.contains(task_id));
            if mentions_task && let Some(step) = extract_next_field(entry) {
                return Some(step);
            }
        }
    }

    for entry in entries {
        if phase_matches(&entry.spec, phase_name)
            && let Some(step) = extract_next_field(entry)
        {
            return Some(step);
        }
    }

    None
}

fn extract_next_field(entry: &InProgressEntry) -> Option<String> {
    for (k, v) in &entry.fields {
        let lower = k.to_lowercase();
        if lower == "next step" || lower == "next" || lower == "next-step" || lower == "next_step" {
            return Some(v.clone());
        }
    }
    None
}

pub fn ongoing_tasks(
    phases: &[Phase],
    claims: &[Claim],
    sessions: &[Session],
    entries: &[InProgressEntry],
    now: f64,
) -> Vec<OngoingTask> {
    let mut tasks = Vec::new();
    let mut matched_claim_indices = HashSet::new();

    for phase in phases {
        for task in &phase.tasks {
            if task.status == "In Progress" {
                let matched_claim = claims.iter().enumerate().find(|(idx, c)| {
                    !matched_claim_indices.contains(idx)
                        && phase_matches(&c.phase, &phase.name)
                        && c.task == task.id
                });

                let (claim, pane, stale) = if let Some((idx, c)) = matched_claim {
                    matched_claim_indices.insert(idx);
                    let pane = sessions
                        .iter()
                        .find(|s| s.id == c.session)
                        .map(|s| s.pane.clone());
                    let is_stale = c.heartbeat > 0.0 && (now - c.heartbeat) > STALE_SECONDS as f64;
                    (Some(c.clone()), pane, is_stale)
                } else {
                    (None, None, false)
                };

                let next_step = extract_next_step(entries, &phase.name, &task.id);

                tasks.push(OngoingTask {
                    phase: phase.name.clone(),
                    task_id: task.id.clone(),
                    title: task.title.clone(),
                    status: task.status.clone(),
                    objectives_done: task.objectives_done,
                    objectives_total: task.objectives_total,
                    claim,
                    session_pane: pane,
                    stale,
                    status_mismatch: false,
                    next_step,
                });
            }
        }
    }

    for (idx, c) in claims.iter().enumerate() {
        if matched_claim_indices.contains(&idx) {
            continue;
        }

        let is_stale = c.heartbeat > 0.0 && (now - c.heartbeat) > STALE_SECONDS as f64;
        let pane = sessions
            .iter()
            .find(|s| s.id == c.session)
            .map(|s| s.pane.clone());
        let next_step = extract_next_step(entries, &c.phase, &c.task);

        let found_phase = phases.iter().find(|p| phase_matches(&c.phase, &p.name));
        let found_task = found_phase.and_then(|p| p.tasks.iter().find(|t| t.id == c.task));

        let (title, status, obj_done, obj_total) = if let Some(t) = found_task {
            (
                t.title.clone(),
                t.status.clone(),
                t.objectives_done,
                t.objectives_total,
            )
        } else {
            (String::new(), "Unknown".to_string(), 0, 0)
        };

        let phase_name = found_phase
            .map(|p| p.name.clone())
            .unwrap_or_else(|| c.phase.clone());

        tasks.push(OngoingTask {
            phase: phase_name,
            task_id: c.task.clone(),
            title,
            status,
            objectives_done: obj_done,
            objectives_total: obj_total,
            claim: Some(c.clone()),
            session_pane: pane,
            stale: is_stale,
            status_mismatch: true,
            next_step,
        });
    }

    tasks.sort_by(|a, b| (&a.phase, &a.task_id).cmp(&(&b.phase, &b.task_id)));
    tasks
}
