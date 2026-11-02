use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::snapshot::StatusSnapshot;

pub fn lines(snapshot: &StatusSnapshot, _width: u16) -> Vec<Line<'static>> {
    if snapshot.sprint.is_none() {
        vec![Line::from(vec![Span::styled(
            "No active sprint",
            Style::default().fg(Color::DarkGray),
        )])]
    } else {
        vec![Line::from(vec![Span::styled(
            "Sprint details will appear here",
            Style::default().fg(Color::DarkGray),
        )])]
    }
}
