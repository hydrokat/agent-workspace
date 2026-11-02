use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Widget};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInput {
    pub value: String,
    pub cursor: usize,
    pub placeholder: String,
    pub focused: bool,
}

impl TextInput {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_placeholder(placeholder: impl Into<String>) -> Self {
        Self {
            value: String::new(),
            cursor: 0,
            placeholder: placeholder.into(),
            focused: false,
        }
    }

    pub fn set_value(&mut self, val: impl Into<String>) {
        self.value = val.into();
        self.cursor = self.value.len();
    }

    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
    }

    pub fn handle_key(&mut self, event: &KeyEvent) -> bool {
        match event.code {
            KeyCode::Char(c) => {
                let char_idx = self
                    .value
                    .char_indices()
                    .nth(self.cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(self.value.len());
                self.value.insert(char_idx, c);
                self.cursor += 1;
                true
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    let prev_idx = self
                        .value
                        .char_indices()
                        .nth(self.cursor - 1)
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    let curr_idx = self
                        .value
                        .char_indices()
                        .nth(self.cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(self.value.len());
                    self.value.replace_range(prev_idx..curr_idx, "");
                    self.cursor -= 1;
                    true
                } else {
                    false
                }
            }
            KeyCode::Delete => {
                let total_chars = self.value.chars().count();
                if self.cursor < total_chars {
                    let curr_idx = self
                        .value
                        .char_indices()
                        .nth(self.cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    let next_idx = self
                        .value
                        .char_indices()
                        .nth(self.cursor + 1)
                        .map(|(i, _)| i)
                        .unwrap_or(self.value.len());
                    self.value.replace_range(curr_idx..next_idx, "");
                    true
                } else {
                    false
                }
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    true
                } else {
                    false
                }
            }
            KeyCode::Right => {
                let total_chars = self.value.chars().count();
                if self.cursor < total_chars {
                    self.cursor += 1;
                    true
                } else {
                    false
                }
            }
            KeyCode::Home => {
                self.cursor = 0;
                true
            }
            KeyCode::End => {
                self.cursor = self.value.chars().count();
                true
            }
            _ => false,
        }
    }

    pub fn click_at(&mut self, col: u16, area_x: u16, area_width: u16) {
        if col >= area_x && col < area_x + area_width {
            let offset = (col - area_x) as usize;
            let total_chars = self.value.chars().count();
            self.cursor = offset.min(total_chars);
            self.focused = true;
        }
    }

    pub fn render_with_block(&self, area: Rect, buf: &mut Buffer, block: Option<Block>) {
        let inner = if let Some(b) = block {
            let inner_area = b.inner(area);
            b.render(area, buf);
            inner_area
        } else {
            area
        };

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let is_empty = self.value.is_empty();
        let display = if is_empty && !self.placeholder.is_empty() {
            &self.placeholder
        } else {
            &self.value
        };

        let base_style = if is_empty {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };

        buf.set_stringn(inner.x, inner.y, display, inner.width as usize, base_style);

        if self.focused {
            let cursor_x = inner.x + self.cursor as u16;
            if cursor_x < inner.x + inner.width {
                let cursor_char = self.value.chars().nth(self.cursor).unwrap_or(' ');
                let cursor_style = Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::REVERSED);
                buf.set_string(cursor_x, inner.y, cursor_char.to_string(), cursor_style);
            }
        }
    }
}
