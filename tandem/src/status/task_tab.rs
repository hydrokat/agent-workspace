use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::snapshot::StatusSnapshot;

pub fn lines(_snapshot: &StatusSnapshot, _width: u16) -> Vec<Line<'static>> {
    vec![Line::from(vec![Span::styled(
        "Task details will appear here",
        Style::default().fg(Color::DarkGray),
    )])]
}
