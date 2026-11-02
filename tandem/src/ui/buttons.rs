use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub id: String,
    pub label: String,
    pub is_default: bool,
}

impl Button {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            is_default: false,
        }
    }

    pub fn default_btn(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            is_default: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ButtonRow {
    pub buttons: Vec<Button>,
    pub focused_idx: usize,
}

impl ButtonRow {
    pub fn new(buttons: Vec<Button>) -> Self {
        let default_idx = buttons.iter().position(|b| b.is_default).unwrap_or(0);
        Self {
            buttons,
            focused_idx: default_idx,
        }
    }

    pub fn click_at(&mut self, col: u16, area_x: u16, area_width: u16) -> Option<&str> {
        if col < area_x || col >= area_x + area_width {
            return None;
        }
        let mut x = area_x;
        for (idx, btn) in self.buttons.iter().enumerate() {
            let width = btn.label.chars().count() as u16 + 4;
            if col >= x && col < x + width {
                self.focused_idx = idx;
                return Some(&btn.id);
            }
            x += width + 2;
        }
        None
    }

    pub fn handle_key(&mut self, event: &KeyEvent) -> Option<&str> {
        match event.code {
            KeyCode::Left => {
                if self.focused_idx > 0 {
                    self.focused_idx -= 1;
                }
                None
            }
            KeyCode::Right => {
                if !self.buttons.is_empty() && self.focused_idx + 1 < self.buttons.len() {
                    self.focused_idx += 1;
                }
                None
            }
            KeyCode::Enter => self.buttons.get(self.focused_idx).map(|b| b.id.as_str()),
            _ => None,
        }
    }
}

impl Widget for &ButtonRow {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let mut x = area.x;
        let y = area.y;

        for (idx, btn) in self.buttons.iter().enumerate() {
            if x >= area.x + area.width {
                break;
            }
            let is_focus = idx == self.focused_idx;
            let style = if is_focus {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else if btn.is_default {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let text = format!("[ {} ]", btn.label);
            buf.set_stringn(x, y, &text, (area.x + area.width - x) as usize, style);
            x += text.chars().count() as u16 + 2;
        }
    }
}
