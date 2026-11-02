use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TabBar {
    pub titles: Vec<String>,
    pub selected: usize,
}

impl TabBar {
    pub fn new(titles: Vec<String>, selected: usize) -> Self {
        Self { titles, selected }
    }

    pub fn select_next(&mut self) {
        if !self.titles.is_empty() {
            self.selected = (self.selected + 1) % self.titles.len();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.titles.is_empty() {
            self.selected = (self.selected + self.titles.len() - 1) % self.titles.len();
        }
    }

    pub fn click_at(&mut self, col: u16, area_x: u16, area_width: u16) -> Option<usize> {
        if col < area_x || col >= area_x + area_width {
            return None;
        }
        let mut x = area_x;
        for (idx, title) in self.titles.iter().enumerate() {
            let width = title.chars().count() as u16 + 4;
            if col >= x && col < x + width {
                self.selected = idx;
                return Some(idx);
            }
            x += width;
        }
        None
    }
}

impl Widget for &TabBar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let mut x = area.x;
        let y = area.y;

        for (idx, title) in self.titles.iter().enumerate() {
            if x >= area.x + area.width {
                break;
            }
            let is_sel = idx == self.selected;
            let style = if is_sel {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let prefix = if is_sel { "[ " } else { "  " };
            let suffix = if is_sel { " ]" } else { "  " };
            let tab_str = format!("{prefix}{title}{suffix}");
            buf.set_stringn(x, y, &tab_str, (area.x + area.width - x) as usize, style);
            x += tab_str.chars().count() as u16;
        }
    }
}
