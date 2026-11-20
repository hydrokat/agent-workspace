use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use unicode_width::UnicodeWidthStr;

use crate::ui::text::truncate_to_width;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Key(KeyCode, KeyModifiers),
    Row(usize),
    Cmd(u8),
}

impl Hit {
    pub fn key(code: KeyCode) -> Self {
        Hit::Key(code, KeyModifiers::NONE)
    }

    pub fn ch(c: char) -> Self {
        Hit::Key(KeyCode::Char(c), KeyModifiers::NONE)
    }

    pub fn ctrl(c: char) -> Self {
        Hit::Key(KeyCode::Char(c), KeyModifiers::CONTROL)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolved {
    Key(KeyEvent),
    Row(usize),
    Cmd(u8),
}

#[derive(Debug, Clone, Default)]
pub struct HitMap {
    regions: Vec<(Rect, Hit)>,
}

impl HitMap {
    pub fn clear(&mut self) {
        self.regions.clear();
    }

    pub fn push(&mut self, rect: Rect, hit: Hit) {
        if rect.width > 0 && rect.height > 0 {
            self.regions.push((rect, hit));
        }
    }

    pub fn at(&self, col: u16, row: u16) -> Option<Hit> {
        self.regions
            .iter()
            .rev()
            .find(|(r, _)| col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height)
            .map(|(_, h)| *h)
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    pub fn put_hints(
        &mut self,
        buf: &mut Buffer,
        line: Rect,
        segments: &[(String, Option<Hit>)],
        separator: &str,
        style: Style,
    ) {
        let (x, y, width) = (line.x, line.y, line.width);
        let sep_w = separator.width();
        let joined = segments
            .iter()
            .map(|(s, _)| s.as_str())
            .collect::<Vec<_>>()
            .join(separator);
        buf.set_string(x, y, truncate_to_width(&joined, width as usize), style);
        let mut offset = 0usize;
        for (text, hit) in segments {
            let w = text.width();
            if offset >= width as usize {
                break;
            }
            if let Some(hit) = hit {
                let visible = w.min(width as usize - offset);
                self.push(Rect::new(x + offset as u16, y, visible as u16, 1), *hit);
            }
            offset += w + sep_w;
        }
    }

    pub fn tags(&mut self, line: Rect, text: &str, tags: &[(&str, Hit)]) {
        for (needle, hit) in tags {
            let Some(byte) = text.find(needle) else {
                continue;
            };
            let offset = text[..byte].width();
            if offset >= line.width as usize {
                continue;
            }
            let w = needle.width().min(line.width as usize - offset);
            self.push(Rect::new(line.x + offset as u16, line.y, w as u16, 1), *hit);
        }
    }

    pub fn resolve(
        &self,
        event: &MouseEvent,
        scroll_up: KeyCode,
        scroll_down: KeyCode,
    ) -> Option<Resolved> {
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => match self.at(event.column, event.row)? {
                Hit::Key(code, mods) => Some(Resolved::Key(KeyEvent::new(code, mods))),
                Hit::Row(i) => Some(Resolved::Row(i)),
                Hit::Cmd(c) => Some(Resolved::Cmd(c)),
            },
            MouseEventKind::ScrollUp => {
                Some(Resolved::Key(KeyEvent::new(scroll_up, KeyModifiers::NONE)))
            }
            MouseEventKind::ScrollDown => Some(Resolved::Key(KeyEvent::new(
                scroll_down,
                KeyModifiers::NONE,
            ))),
            _ => None,
        }
    }
}

pub fn seg(text: impl Into<String>, hit: Hit) -> (String, Option<Hit>) {
    (text.into(), Some(hit))
}

pub fn plain(text: impl Into<String>) -> (String, Option<Hit>) {
    (text.into(), None)
}
