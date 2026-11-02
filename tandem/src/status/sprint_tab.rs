use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::sanitize::clean;
use crate::snapshot::StatusSnapshot;
use crate::sources::ongoing::phase_matches;
use crate::status::render::{bar, days_left};

fn format_date(d: &str) -> String {
    if let Ok(date) = chrono::NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d") {
        date.format("%b %-d").to_string()
    } else {
        clean(d)
    }
}

fn format_dates(start: &str, end: &str) -> String {
    let start_clean = start.trim();
    let end_clean = end.trim();
    if !start_clean.is_empty() && !end_clean.is_empty() {
        format!("{} → {}", format_date(start_clean), format_date(end_clean))
    } else if !start_clean.is_empty() {
        format_date(start_clean)
    } else if !end_clean.is_empty() {
        format_date(end_clean)
    } else {
        String::new()
    }
}

fn moscow_order(moscow: &str) -> usize {
    let lower = moscow.to_lowercase();
    if lower.contains("must") {
        0
    } else if lower.contains("should") {
        1
    } else if lower.contains("could") {
        2
    } else {
        3
    }
}

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
    let Some(sprint) = snapshot.sprint.as_ref() else {
        return vec![Line::from(vec![Span::styled(
            "No active sprint. Plan one with /pm-new-sprint.",
            Style::default().fg(Color::DarkGray),
        )])];
    };

    let mut result = Vec::new();

    let mut header_spans = Vec::new();
    let clean_id = clean(&sprint.id);
    let clean_status = clean(&sprint.status);
    let dates = format_dates(&sprint.start_date, &sprint.end_date);
    let raw_days_left = days_left(&sprint.end_date, snapshot.now);
    let left = if sprint.status == "closed" || raw_days_left.starts_with("overdue") {
        "ended".to_string()
    } else {
        raw_days_left
    };

    header_spans.push(Span::styled(
        clean_id.clone(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));

    let clean_title = clean(&sprint.title);
    let has_distinct_title = !clean_title.is_empty() && clean_title != clean_id;

    let available = (width as usize).saturating_sub(2);
    let mut base_len = clean_id.len() + 3 + clean_status.len();
    if !dates.is_empty() {
        base_len += 3 + dates.len();
    }
    if !left.is_empty() {
        base_len += 3 + left.len();
    }

    if has_distinct_title {
        let max_title_space = available.saturating_sub(base_len + 2);
        if max_title_space >= 8 {
            let disp_title = truncate_with_ellipsis(&clean_title, max_title_space);
            header_spans.push(Span::raw("  "));
            header_spans.push(Span::styled(disp_title, Style::default().fg(Color::White)));
        }
    }

    let status_color = match sprint.status.as_str() {
        "open" => Color::Green,
        "locked" => Color::Yellow,
        "closed" => Color::DarkGray,
        _ => Color::White,
    };
    header_spans.push(Span::raw("   "));
    header_spans.push(Span::styled(
        clean_status,
        Style::default().fg(status_color),
    ));

    if !dates.is_empty() {
        header_spans.push(Span::raw("   "));
        header_spans.push(Span::styled(dates, Style::default().fg(Color::DarkGray)));
    }

    if !left.is_empty() {
        let left_color = if left == "ended" || left.starts_with("overdue") {
            Color::Red
        } else if left == "ends today" || left.contains("left") {
            Color::Yellow
        } else {
            Color::DarkGray
        };
        header_spans.push(Span::raw("   "));
        header_spans.push(Span::styled(left, Style::default().fg(left_color)));
    }
    result.push(Line::from(header_spans));

    let cap_bar_w = if width < 40 { 6 } else { 16 };
    let cap_filled = (sprint.committed_days * 10.0).max(0.0).round() as usize;
    let cap_total = (sprint.capacity_days * 10.0).max(0.0).round() as usize;
    let c_bar = bar(cap_filled, cap_total, cap_bar_w);

    let mut cap_spans = vec![
        Span::styled("capacity  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            c_bar,
            Style::default().fg(if sprint.overloaded {
                Color::Red
            } else {
                Color::Cyan
            }),
        ),
        Span::styled(
            format!(
                "  {:.1} / {:.1} days",
                sprint.committed_days, sprint.capacity_days
            ),
            Style::default().fg(Color::White),
        ),
    ];
    if sprint.overloaded {
        cap_spans.push(Span::styled(
            "  overloaded",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    }
    result.push(Line::from(cap_spans));

    let prog_bar_w = if width < 40 { 6 } else { 16 };
    let p_bar = bar(sprint.done, sprint.total, prog_bar_w);
    let prog_spans = vec![
        Span::styled("progress  ", Style::default().fg(Color::DarkGray)),
        Span::styled(p_bar, Style::default().fg(Color::Green)),
        Span::styled(
            format!("  {} / {} tasks", sprint.done, sprint.total),
            Style::default().fg(Color::White),
        ),
        Span::styled(
            format!("   must-have left: {}", sprint.must_remaining),
            Style::default().fg(if sprint.must_remaining > 0 {
                Color::Yellow
            } else {
                Color::DarkGray
            }),
        ),
    ];
    result.push(Line::from(prog_spans));

    let phase_col_w = if width < 60 {
        (width as usize).saturating_sub(14 + 8 + 15).max(10)
    } else {
        31
    };

    let table_header = vec![
        Span::styled(
            format!("{:<width$}", "Phase", width = phase_col_w),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<14}", "MoSCoW"),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<8}", "Effort"),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Done",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    result.push(Line::from(table_header));

    let mut sorted_phases = sprint.phases.clone();
    sorted_phases.sort_by_key(|p| moscow_order(&p.moscow));

    for phase in sorted_phases {
        let exists = snapshot
            .phases
            .iter()
            .any(|p| phase_matches(&p.name, &phase.name));
        let clean_name = clean(&phase.name);
        let max_name_len = phase_col_w.saturating_sub(2);
        let disp_name = truncate_with_ellipsis(&clean_name, max_name_len);

        let m_color = match moscow_order(&phase.moscow) {
            0 => Color::Cyan,
            1 => Color::Blue,
            2 => Color::DarkGray,
            _ => Color::DarkGray,
        };

        let mut row_spans = vec![
            Span::styled(
                format!("{:<width$}", disp_name, width = phase_col_w),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!("{:<14}", clean(&phase.moscow)),
                Style::default().fg(m_color),
            ),
            Span::styled(
                format!("{:<8}", format!("{:.1}d", phase.effort)),
                Style::default().fg(Color::White),
            ),
        ];

        if exists {
            let phase_bar_w = if width < 40 { 6 } else { 8 };
            let row_bar = bar(phase.done, phase.total, phase_bar_w);
            row_spans.push(Span::styled(row_bar, Style::default().fg(Color::Green)));
            row_spans.push(Span::raw("  "));
            row_spans.push(Span::styled(
                format!("{}/{}", phase.done, phase.total),
                Style::default().fg(Color::White),
            ));
        } else {
            row_spans.push(Span::styled(
                "(no specs/ directory)",
                Style::default().fg(Color::Yellow),
            ));
        }

        result.push(Line::from(row_spans));
    }

    result
}
