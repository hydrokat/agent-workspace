use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::sanitize::clean;
use crate::snapshot::StatusSnapshot;
use crate::status::render::bar;

fn truncate_with_ellipsis(text: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    let char_count = text.chars().count();
    if char_count <= max_len {
        return text.to_string();
    }
    if max_len == 1 {
        return "…".to_string();
    }
    let truncated: String = text.chars().take(max_len - 1).collect();
    format!("{truncated}…")
}

pub fn lines(snapshot: &StatusSnapshot, width: u16) -> Vec<Line<'static>> {
    if snapshot.ongoing_specs.is_empty() {
        return vec![Line::from(vec![Span::styled(
            "No ongoing spec.",
            Style::default().fg(Color::DarkGray),
        )])];
    }

    let mut result = Vec::new();

    for (spec_idx, ongoing) in snapshot.ongoing_specs.iter().enumerate() {
        if spec_idx > 0 {
            result.push(Line::raw(""));
        }

        let phase = &ongoing.phase;
        let clean_name = clean(&phase.name);
        let clean_title = clean(&phase.title);

        let mut header_spans = Vec::new();
        if ongoing.next_up {
            header_spans.push(Span::styled(
                "next up: ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        }

        header_spans.push(Span::styled(
            clean_name.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

        if !clean_title.is_empty() && clean_title != clean_name {
            header_spans.push(Span::raw("   "));
            header_spans.push(Span::styled(clean_title, Style::default().fg(Color::White)));
        }
        result.push(Line::from(header_spans));

        let clean_branch = clean(&phase.branch);
        let clean_target = clean(&phase.target);
        let has_branch = !clean_branch.is_empty();
        let branch_text = if has_branch && !clean_target.is_empty() {
            format!("{clean_branch} → {clean_target}")
        } else if has_branch {
            clean_branch
        } else {
            String::new()
        };

        let bar_w = if width < 40 { 6 } else { 8 };
        let done_count = phase.done();
        let total_count = phase.total();
        let p_bar = bar(done_count, total_count, bar_w);
        let progress_text = format!("  {done_count} / {total_count} done");

        let mut branch_row_spans = Vec::new();
        let available = (width as usize).saturating_sub(2);
        let right_len = bar_w + progress_text.len();

        if !branch_text.is_empty() {
            let left_len = branch_text.len();
            branch_row_spans.push(Span::styled(
                branch_text,
                Style::default().fg(Color::DarkGray),
            ));

            let space_needed = left_len + 2 + right_len;
            if available > space_needed {
                let padding = available - left_len - right_len;
                branch_row_spans.push(Span::raw(" ".repeat(padding)));
            } else {
                branch_row_spans.push(Span::raw("  "));
            }
        }

        branch_row_spans.push(Span::styled(p_bar, Style::default().fg(Color::Green)));
        branch_row_spans.push(Span::styled(
            progress_text,
            Style::default().fg(Color::White),
        ));
        result.push(Line::from(branch_row_spans));

        if !phase.warning.is_empty() {
            let clean_w = clean(&phase.warning);
            let w_line = if clean_w.starts_with("warning:") {
                clean_w
            } else {
                format!("warning: {clean_w}")
            };
            result.push(Line::from(vec![Span::styled(
                w_line,
                Style::default().fg(Color::Yellow),
            )]));
        }

        let mut sorted_tasks = phase.tasks.clone();
        sorted_tasks.sort_by(|a, b| a.id.cmp(&b.id));

        for task in sorted_tasks {
            let is_in_progress = task.status == "In Progress";
            let marker_span = if is_in_progress {
                Span::styled(
                    "▶ ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("  ")
            };

            let clean_task_id = clean(&task.id);
            let id_span = Span::styled(
                format!("{:<6}", clean_task_id),
                Style::default().fg(Color::Cyan),
            );

            let clean_status = clean(&task.status);
            let status_color = match clean_status.as_str() {
                "In Progress" => Color::Cyan,
                "Done" => Color::Green,
                "In Review" => Color::Yellow,
                "Blocked" => Color::Red,
                "Pending" => Color::DarkGray,
                _ => Color::White,
            };
            let mut status_style = Style::default().fg(status_color);
            if is_in_progress {
                status_style = status_style.add_modifier(Modifier::BOLD);
            }
            let status_span = Span::styled(format!("{:<12}", clean_status), status_style);

            let prefix_len = 2 + 6 + 12;
            let available_for_title = available.saturating_sub(prefix_len + 1);
            let clean_title = clean(&task.title);
            let disp_title = truncate_with_ellipsis(&clean_title, available_for_title);
            let title_span = Span::styled(disp_title, Style::default().fg(Color::White));

            result.push(Line::from(vec![
                marker_span,
                id_span,
                status_span,
                Span::raw(" "),
                title_span,
            ]));
        }
    }

    result
}
