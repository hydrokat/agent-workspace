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
        if area.height == 0 || area.width == 0 {
            return;
        }

        let mut x = area.x;
        let y = area.y;

        for item in self.items {
            if x >= area.x + area.width {
                break;
            }
            let key_style = Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD);
            let label_style = Style::default().fg(Color::Gray);

            buf.set_string(x, y, &item.key, key_style);
            x += item.key.chars().count() as u16;

            if x < area.x + area.width {
                buf.set_string(x, y, " ", Style::default());
                x += 1;
            }

            buf.set_string(x, y, &item.label, label_style);
            x += item.label.chars().count() as u16;

            if x < area.x + area.width {
                buf.set_string(x, y, "  ", Style::default());
                x += 2;
            }
        }
    }
}
