use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::sanitize::clean;
use crate::snapshot::StatusSnapshot;
use crate::status::render::{age, bar};

fn short_phase(name: &str) -> String {
    if name.starts_with("p-") {
        name.splitn(3, '-').take(2).collect::<Vec<_>>().join("-")
    } else {
        name.to_string()
    }
}

pub fn lines(snapshot: &StatusSnapshot, width: u16) -> Vec<Line<'static>> {
    let in_review: usize = snapshot
        .ongoing_specs
        .iter()
        .map(|s| {
            s.phase
                .tasks
                .iter()
                .filter(|t| t.status == "In Review")
                .count()
        })
        .sum();
    let blocked: usize = snapshot
        .ongoing_specs
        .iter()
        .map(|s| {
            s.phase
                .tasks
                .iter()
                .filter(|t| t.status == "Blocked")
                .count()
        })
        .sum();

    if snapshot.ongoing_tasks.is_empty() {
        let mut empty_lines = vec![Line::from(vec![Span::styled(
            "No task is in progress.",
            Style::default().fg(Color::DarkGray),
        )])];
        if in_review > 0 || blocked > 0 {
            empty_lines.push(Line::from(vec![
                Span::styled("In Review: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    in_review.to_string(),
                    Style::default().fg(if in_review > 0 {
                        Color::Yellow
                    } else {
                        Color::White
                    }),
                ),
                Span::styled(" · Blocked: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    blocked.to_string(),
                    Style::default().fg(if blocked > 0 {
                        Color::Red
                    } else {
                        Color::White
                    }),
                ),
            ]));
        }
        return empty_lines;
    }

    let is_narrow = width <= 40;
    let (owner_label, branch_label, paths_label, next_label, indent_cont) = if is_narrow {
        ("o ", "b ", "p ", "n ", "  ")
    } else {
        (
            "owner     ",
            "branch    ",
            "paths     ",
            "next      ",
            "          ",
        )
    };

    let mut result = Vec::new();
    let available = (width as usize).saturating_sub(2);

    for (task_idx, task) in snapshot.ongoing_tasks.iter().enumerate() {
        if task_idx > 0 {
            result.push(Line::raw(""));
        }

        let clean_phase = clean(&task.phase);
        let short_p = short_phase(&clean_phase);
        let clean_id = clean(&task.task_id);
        let clean_title = clean(&task.title);

        let left_prefix = format!("{short_p} {clean_id}   {clean_title}");

        let mut right_spans = Vec::new();

        if task.objectives_total > 0 {
            let obj_bar = bar(task.objectives_done, task.objectives_total, 6);
            right_spans.push(Span::styled(obj_bar, Style::default().fg(Color::Cyan)));
            right_spans.push(Span::styled(
                format!(
                    "  objectives {} / {}",
                    task.objectives_done, task.objectives_total
                ),
                Style::default().fg(Color::White),
            ));
        }

        let mut flags = Vec::new();
        if task.claim.is_none() {
            flags.push("unclaimed");
        } else {
            if task.stale {
                flags.push("stale");
            }
            if task.status_mismatch {
                flags.push("status mismatch");
            }
        }

        if !flags.is_empty() {
            if !right_spans.is_empty() {
                right_spans.push(Span::raw("   "));
            }
            let flag_str = flags.join(", ");
            let flag_color = if flag_str.contains("stale") || flag_str.contains("mismatch") {
                Color::Red
            } else {
                Color::Yellow
            };
            right_spans.push(Span::styled(
                flag_str,
                Style::default().fg(flag_color).add_modifier(Modifier::BOLD),
            ));
        }

        let right_len: usize = right_spans.iter().map(|s| s.content.len()).sum();
        let mut header_spans = vec![
            Span::styled(
                format!("{short_p} {clean_id}"),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("   "),
            Span::styled(clean_title, Style::default().fg(Color::White)),
        ];

        let left_len = left_prefix.len();
        if right_len > 0 {
            if available > left_len + right_len + 2 {
                let padding = available - left_len - right_len;
                header_spans.push(Span::raw(" ".repeat(padding)));
            } else {
                header_spans.push(Span::raw("  "));
            }
            header_spans.extend(right_spans);
        }
        result.push(Line::from(header_spans));

        if let Some(ref claim) = task.claim {
            let s_id = clean(&claim.session);
            let clean_pane = task.session_pane.as_deref().unwrap_or("");
            let clean_pane_str = clean(clean_pane);
            let owner_val = if !clean_pane_str.is_empty() {
                format!("{s_id} (pane {clean_pane_str})")
            } else {
                s_id
            };

            let hb_str = if claim.heartbeat > 0.0 {
                format!("heartbeat {} ago", age(snapshot.now - claim.heartbeat))
            } else {
                String::new()
            };

            let mut owner_spans = vec![
                Span::raw("  "),
                Span::styled(owner_label, Style::default().fg(Color::DarkGray)),
                Span::styled(owner_val.clone(), Style::default().fg(Color::White)),
            ];

            if !hb_str.is_empty() {
                let left_w = 2 + owner_label.len() + owner_val.len();
                if available > left_w + hb_str.len() + 2 {
                    let pad = available - left_w - hb_str.len();
                    owner_spans.push(Span::raw(" ".repeat(pad)));
                } else {
                    owner_spans.push(Span::raw("          "));
                }
                owner_spans.push(Span::styled(
                    hb_str,
                    Style::default().fg(if task.stale {
                        Color::Red
                    } else {
                        Color::DarkGray
                    }),
                ));
            }
            result.push(Line::from(owner_spans));

            let clean_b = clean(&claim.branch);
            let clean_wt = clean(&claim.worktree);
            if !clean_b.is_empty() || !clean_wt.is_empty() {
                let mut branch_spans = vec![
                    Span::raw("  "),
                    Span::styled(branch_label, Style::default().fg(Color::DarkGray)),
                    Span::styled(clean_b.clone(), Style::default().fg(Color::White)),
                ];

                if !clean_wt.is_empty() {
                    let left_w = 2 + branch_label.len() + clean_b.len();
                    let wt_text = format!("worktree {clean_wt}");
                    if available > left_w + wt_text.len() + 2 {
                        let pad = available - left_w - wt_text.len();
                        branch_spans.push(Span::raw(" ".repeat(pad)));
                    } else {
                        branch_spans.push(Span::raw("       "));
                    }
                    branch_spans.push(Span::styled(
                        "worktree ",
                        Style::default().fg(Color::DarkGray),
                    ));
                    branch_spans.push(Span::styled(clean_wt, Style::default().fg(Color::White)));
                }
                result.push(Line::from(branch_spans));
            }

            if !claim.paths.is_empty() {
                for (i, p) in claim.paths.iter().enumerate() {
                    let clean_p = clean(p);
                    if i == 0 {
                        result.push(Line::from(vec![
                            Span::raw("  "),
                            Span::styled(paths_label, Style::default().fg(Color::DarkGray)),
                            Span::styled(clean_p, Style::default().fg(Color::White)),
                        ]));
                    } else {
                        result.push(Line::from(vec![
                            Span::raw("  "),
                            Span::raw(indent_cont),
                            Span::styled(clean_p, Style::default().fg(Color::White)),
                        ]));
                    }
                }
            }
        }

        if let Some(ref next) = task.next_step {
            let clean_next = clean(next);
            if !clean_next.is_empty() {
                result.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(next_label, Style::default().fg(Color::DarkGray)),
                    Span::styled(clean_next, Style::default().fg(Color::White)),
                ]));
            }
        }
    }

    result.push(Line::from(vec![
        Span::styled("In Review: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            in_review.to_string(),
            Style::default().fg(if in_review > 0 {
                Color::Yellow
            } else {
                Color::White
            }),
        ),
        Span::styled(" · Blocked: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            blocked.to_string(),
            Style::default().fg(if blocked > 0 {
                Color::Red
            } else {
                Color::White
            }),
        ),
    ]));

    result
}
