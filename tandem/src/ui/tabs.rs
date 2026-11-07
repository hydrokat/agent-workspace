use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TabBar {
    pub titles: Vec<String>,
    pub short_titles: Vec<String>,
    pub number_titles: Vec<String>,
    pub selected: usize,
}

impl TabBar {
    pub fn new(titles: Vec<String>, selected: usize) -> Self {
        Self {
            titles,
            short_titles: Vec::new(),
            number_titles: Vec::new(),
            selected,
        }
    }

    pub fn from_tabs(
        tabs: &[crate::status::app::StatusTab],
        open_queries: usize,
        selected: crate::status::app::StatusTab,
    ) -> Self {
        let titles = tabs.iter().map(|t| t.title(open_queries)).collect();
        let short_titles = tabs.iter().map(|t| t.short_title(open_queries)).collect();
        let number_titles = tabs.iter().map(|t| t.number_title(open_queries)).collect();
        Self {
            titles,
            short_titles,
            number_titles,
            selected: selected.index(),
        }
    }

    pub fn titles_for_width(&self, width: u16) -> &[String] {
        if width >= 120 || self.short_titles.is_empty() {
            &self.titles
        } else if width >= 80 || self.number_titles.is_empty() {
            &self.short_titles
        } else {
            &self.number_titles
        }
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
        let titles = self.titles_for_width(area_width);
        let mut x = area_x;
        for (idx, title) in titles.iter().enumerate() {
            let width = if area_width < 80 && !self.number_titles.is_empty() {
                if idx == self.selected {
                    let active_name = self.titles.get(idx).map(|s| s.as_str()).unwrap_or("");
                    (title.chars().count() + active_name.chars().count() + 4) as u16
                } else {
                    (title.chars().count() + 3) as u16
                }
            } else {
                title.chars().count() as u16 + 4
            };
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

        let titles = self.titles_for_width(area.width);
        let mut x = area.x;
        let y = area.y;

        for (idx, title) in titles.iter().enumerate() {
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

            let tab_str = if area.width < 80 && !self.number_titles.is_empty() {
                if is_sel {
                    let active_name = self.titles.get(idx).map(|s| s.as_str()).unwrap_or("");
                    format!("[{title}] {active_name} ")
                } else {
                    format!("[{title}] ")
                }
            } else {
                let prefix = if is_sel { "[ " } else { "  " };
                let suffix = if is_sel { " ]" } else { "  " };
                format!("{prefix}{title}{suffix}")
            };

            buf.set_stringn(x, y, &tab_str, (area.x + area.width - x) as usize, style);
            x += tab_str.chars().count() as u16;
        }
    }
}
