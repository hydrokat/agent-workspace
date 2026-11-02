use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, StatefulWidget, Widget};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionItem {
    pub id: String,
    pub label: String,
    pub disabled: bool,
    pub is_header: bool,
}

impl OptionItem {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            disabled: false,
            is_header: false,
        }
    }

    pub fn disabled(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            disabled: true,
            is_header: false,
        }
    }

    pub fn header(label: impl Into<String>) -> Self {
        Self {
            id: String::new(),
            label: label.into(),
            disabled: true,
            is_header: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OptionListState {
    pub selected: Option<usize>,
    pub scroll_offset: usize,
}

impl OptionListState {
    pub fn select(&mut self, idx: Option<usize>) {
        self.selected = idx;
    }

    pub fn select_next(&mut self, items: &[OptionItem]) {
        if items.is_empty() {
            self.selected = None;
            return;
        }
        let start = self.selected.map(|s| s + 1).unwrap_or(0);
        for (i, item) in items.iter().enumerate().skip(start) {
            if !item.disabled && !item.is_header {
                self.selected = Some(i);
                return;
            }
        }
    }

    pub fn select_prev(&mut self, items: &[OptionItem]) {
        if items.is_empty() {
            self.selected = None;
            return;
        }
        let start = match self.selected {
            Some(0) | None => return,
            Some(s) => s - 1,
        };
        for i in (0..=start).rev() {
            if !items[i].disabled && !items[i].is_header {
                self.selected = Some(i);
                return;
            }
        }
    }

    pub fn select_first_selectable(&mut self, items: &[OptionItem]) {
        for (i, it) in items.iter().enumerate() {
            if !it.disabled && !it.is_header {
                self.selected = Some(i);
                return;
            }
        }
        self.selected = None;
    }

    pub fn click_at(
        &mut self,
        _col: u16,
        row: u16,
        area_y: u16,
        area_height: u16,
        items: &[OptionItem],
    ) -> Option<usize> {
        if row < area_y || row >= area_y + area_height {
            return None;
        }
        let relative = (row - area_y) as usize;
        let item_idx = self.scroll_offset + relative;
        if item_idx < items.len() && !items[item_idx].disabled && !items[item_idx].is_header {
            self.selected = Some(item_idx);
            return Some(item_idx);
        }
        None
    }

    pub fn selected_id<'a>(&self, items: &'a [OptionItem]) -> Option<&'a str> {
        self.selected
            .and_then(|idx| items.get(idx))
            .map(|it| it.id.as_str())
    }
}

pub struct OptionList<'a> {
    items: &'a [OptionItem],
    block: Option<Block<'a>>,
    highlight_style: Style,
    disabled_style: Style,
}

impl<'a> OptionList<'a> {
    pub fn new(items: &'a [OptionItem]) -> Self {
        Self {
            items,
            block: None,
            highlight_style: Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            disabled_style: Style::default().fg(Color::DarkGray),
        }
    }

    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }
}

impl StatefulWidget for OptionList<'_> {
    type State = OptionListState;

    fn render(mut self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let inner = if let Some(block) = self.block.take() {
            let inner_area = block.inner(area);
            block.render(area, buf);
            inner_area
        } else {
            area
        };

        if inner.height == 0 || inner.width == 0 || self.items.is_empty() {
            return;
        }

        if let Some(sel) = state.selected {
            if sel < state.scroll_offset {
                state.scroll_offset = sel;
            } else if sel >= state.scroll_offset + inner.height as usize {
                state.scroll_offset = sel - inner.height as usize + 1;
            }
        }

        for row in 0..inner.height {
            let item_idx = state.scroll_offset + row as usize;
            if item_idx >= self.items.len() {
                break;
            }
            let item = &self.items[item_idx];
            let is_sel = state.selected == Some(item_idx);
            let y = inner.y + row;

            let style = if item.is_header {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if item.disabled {
                self.disabled_style
            } else if is_sel {
                self.highlight_style
            } else {
                Style::default()
            };

            let prefix = if is_sel { "▶ " } else { "  " };
            let display_text = format!("{prefix}{}", item.label);
            buf.set_stringn(inner.x, y, &display_text, inner.width as usize, style);
        }
    }
}
