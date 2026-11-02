use crossterm::event::{KeyCode, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollView {
    pub offset: usize,
}

impl ScrollView {
    pub fn new() -> Self {
        Self { offset: 0 }
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.offset = self.offset.saturating_sub(n);
    }

    pub fn scroll_down(&mut self, n: usize, total_lines: usize, view_height: usize) {
        let max_offset = total_lines.saturating_sub(view_height);
        self.offset = (self.offset + n).min(max_offset);
    }

    pub fn scroll_page_up(&mut self, view_height: usize) {
        self.scroll_up(view_height.max(1));
    }

    pub fn scroll_page_down(&mut self, total_lines: usize, view_height: usize) {
        self.scroll_down(view_height.max(1), total_lines, view_height);
    }

    pub fn scroll_to_top(&mut self) {
        self.offset = 0;
    }

    pub fn scroll_to_bottom(&mut self, total_lines: usize, view_height: usize) {
        self.offset = total_lines.saturating_sub(view_height);
    }

    pub fn clamp(&mut self, total_lines: usize, view_height: usize) {
        let max_offset = total_lines.saturating_sub(view_height);
        if self.offset > max_offset {
            self.offset = max_offset;
        }
    }

    pub fn handle_key(&mut self, code: KeyCode, total_lines: usize, view_height: usize) -> bool {
        match code {
            KeyCode::Up => {
                self.scroll_up(1);
                true
            }
            KeyCode::Down => {
                self.scroll_down(1, total_lines, view_height);
                true
            }
            KeyCode::PageUp => {
                self.scroll_page_up(view_height);
                true
            }
            KeyCode::PageDown => {
                self.scroll_page_down(total_lines, view_height);
                true
            }
            KeyCode::Home => {
                self.scroll_to_top();
                true
            }
            KeyCode::End => {
                self.scroll_to_bottom(total_lines, view_height);
                true
            }
            _ => false,
        }
    }

    pub fn handle_mouse(
        &mut self,
        kind: MouseEventKind,
        total_lines: usize,
        view_height: usize,
    ) -> bool {
        match kind {
            MouseEventKind::ScrollUp => {
                self.scroll_up(3);
                true
            }
            MouseEventKind::ScrollDown => {
                self.scroll_down(3, total_lines, view_height);
                true
            }
            _ => false,
        }
    }

    pub fn render_lines(&mut self, lines: &[Line], area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        let height = area.height as usize;
        self.clamp(lines.len(), height);

        let visible = lines.iter().skip(self.offset).take(height);
        for (i, line) in visible.enumerate() {
            let y = area.y + i as u16;
            buf.set_line(
                area.x + 1,
                y,
                line,
                (area.width as usize).saturating_sub(2) as u16,
            );
        }
    }
}
