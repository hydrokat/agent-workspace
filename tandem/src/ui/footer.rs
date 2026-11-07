use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FooterItem {
    pub key: String,
    pub label: String,
}

impl FooterItem {
    pub fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
        }
    }
}

pub struct Footer<'a> {
    pub items: &'a [FooterItem],
}

impl<'a> Footer<'a> {
    pub fn new(items: &'a [FooterItem]) -> Self {
        Self { items }
    }
}

impl Widget for Footer<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 || self.items.is_empty() {
            return;
        }

        let max_lines = area.height.min(2);
        let mut line: u16 = 0;
        let mut x = area.x;
        let mut y = area.y;

        for item in self.items {
            let key_len = item.key.chars().count() as u16;
            let label_len = item.label.chars().count() as u16;
            let item_width = key_len + 1 + label_len;

            if x + item_width > area.x + area.width && x > area.x {
                if line + 1 < max_lines {
                    line += 1;
                    y = area.y + line;
                    x = area.x;
                } else {
                    if x < area.x + area.width {
                        buf.set_string(x, y, "…", Style::default().fg(Color::DarkGray));
                    }
                    break;
                }
            }

            if x >= area.x + area.width {
                break;
            }

            let key_style = Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD);
            let label_style = Style::default().fg(Color::Gray);

            let remaining = (area.x + area.width).saturating_sub(x) as usize;
            buf.set_stringn(x, y, &item.key, remaining, key_style);
            x += key_len.min(remaining as u16);

            if x < area.x + area.width {
                buf.set_string(x, y, " ", Style::default());
                x += 1;
            }

            let remaining_label = (area.x + area.width).saturating_sub(x) as usize;
            buf.set_stringn(x, y, &item.label, remaining_label, label_style);
            x += label_len.min(remaining_label as u16);

            if x + 2 <= area.x + area.width {
                buf.set_string(x, y, "  ", Style::default());
                x += 2;
            }
        }
    }
}
