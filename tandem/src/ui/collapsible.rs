use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Collapsible {
    pub title: String,
    pub expanded: bool,
}

impl Collapsible {
    pub fn new(title: impl Into<String>, expanded: bool) -> Self {
        Self {
            title: title.into(),
            expanded,
        }
    }

    pub fn toggle(&mut self) {
        self.expanded = !self.expanded;
    }

    pub fn click_at(&mut self, row: u16, area_y: u16) -> bool {
        if row == area_y {
            self.toggle();
            true
        } else {
            false
        }
    }

    pub fn render_header(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        let icon = if self.expanded { "▼" } else { "▶" };
        let text = format!("{icon} {}", self.title);
        let style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        buf.set_stringn(area.x, area.y, &text, area.width as usize, style);
    }
}
