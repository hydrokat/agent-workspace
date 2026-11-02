use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Widget};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextArea {
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub focused: bool,
    pub read_only: bool,
}

impl TextArea {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            cursor_row: 0,
            cursor_col: 0,
            focused: false,
            read_only: false,
        }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn set_text(&mut self, text: &str) {
        let lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
        if lines.is_empty() {
            self.lines = vec![String::new()];
        } else {
            self.lines = lines;
        }
        self.cursor_row = self.lines.len() - 1;
        self.cursor_col = self.lines[self.cursor_row].len();
    }

    pub fn handle_key(&mut self, event: &KeyEvent) -> bool {
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }

        if self.read_only
            && matches!(
                event.code,
                KeyCode::Char(_) | KeyCode::Enter | KeyCode::Backspace
            )
        {
            return false;
        }

        match event.code {
            KeyCode::Char(c) => {
                let current_line = &mut self.lines[self.cursor_row];
                let byte_idx = current_line
                    .char_indices()
                    .nth(self.cursor_col)
                    .map(|(i, _)| i)
                    .unwrap_or(current_line.len());
                current_line.insert(byte_idx, c);
                self.cursor_col += 1;
                true
            }
            KeyCode::Enter => {
                let current_line = &mut self.lines[self.cursor_row];
                let byte_idx = current_line
                    .char_indices()
                    .nth(self.cursor_col)
                    .map(|(i, _)| i)
                    .unwrap_or(current_line.len());
                let tail = current_line.split_off(byte_idx);
                self.cursor_row += 1;
                self.cursor_col = 0;
                self.lines.insert(self.cursor_row, tail);
                true
            }
            KeyCode::Backspace => {
                if self.cursor_col > 0 {
                    let current_line = &mut self.lines[self.cursor_row];
                    let prev_idx = current_line
                        .char_indices()
                        .nth(self.cursor_col - 1)
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    let curr_idx = current_line
                        .char_indices()
                        .nth(self.cursor_col)
                        .map(|(i, _)| i)
                        .unwrap_or(current_line.len());
                    current_line.replace_range(prev_idx..curr_idx, "");
                    self.cursor_col -= 1;
                    true
                } else if self.cursor_row > 0 {
                    let popped = self.lines.remove(self.cursor_row);
                    self.cursor_row -= 1;
                    self.cursor_col = self.lines[self.cursor_row].chars().count();
                    self.lines[self.cursor_row].push_str(&popped);
                    true
                } else {
                    false
                }
            }
            KeyCode::Left => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                    true
                } else if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                    self.cursor_col = self.lines[self.cursor_row].chars().count();
                    true
                } else {
                    false
                }
            }
            KeyCode::Right => {
                let line_len = self.lines[self.cursor_row].chars().count();
                if self.cursor_col < line_len {
                    self.cursor_col += 1;
                    true
                } else if self.cursor_row + 1 < self.lines.len() {
                    self.cursor_row += 1;
                    self.cursor_col = 0;
                    true
                } else {
                    false
                }
            }
            KeyCode::Up if self.cursor_row > 0 => {
                self.cursor_row -= 1;
                let line_len = self.lines[self.cursor_row].chars().count();
                self.cursor_col = self.cursor_col.min(line_len);
                true
            }
            KeyCode::Down if self.cursor_row + 1 < self.lines.len() => {
                self.cursor_row += 1;
                let line_len = self.lines[self.cursor_row].chars().count();
                self.cursor_col = self.cursor_col.min(line_len);
                true
            }
            _ => false,
        }
    }

    pub fn click_at(
        &mut self,
        col: u16,
        row: u16,
        area_x: u16,
        area_y: u16,
        area_width: u16,
        area_height: u16,
    ) {
        if col >= area_x && col < area_x + area_width && row >= area_y && row < area_y + area_height
        {
            let relative_row = (row - area_y) as usize;
            if relative_row < self.lines.len() {
                self.cursor_row = relative_row;
                let offset = (col - area_x) as usize;
                let line_len = self.lines[self.cursor_row].chars().count();
                self.cursor_col = offset.min(line_len);
                self.focused = true;
            }
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

        for (row_idx, line) in self.lines.iter().enumerate().take(inner.height as usize) {
            let y = inner.y + row_idx as u16;
            buf.set_stringn(
                inner.x,
                y,
                line,
                inner.width as usize,
                Style::default().fg(Color::White),
            );

            if self.focused && row_idx == self.cursor_row {
                let cursor_x = inner.x + self.cursor_col as u16;
                if cursor_x < inner.x + inner.width {
                    let cursor_char = line.chars().nth(self.cursor_col).unwrap_or(' ');
                    let cursor_style = Style::default()
                        .fg(Color::Black)
                        .bg(Color::White)
                        .add_modifier(Modifier::REVERSED);
                    buf.set_string(cursor_x, y, cursor_char.to_string(), cursor_style);
                }
            }
        }
    }
}
