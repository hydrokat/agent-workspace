use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::factory::board::{COLUMN_ORDER, Move, MoveVia, state_key};
use crate::factory::model::{IdeaState, Risk};
use crate::sanitize::clean;
use crate::snapshot::{FactorySummary, IdeaCard};
use crate::ui::hits::{Hit, HitMap};
use crate::ui::text::truncate_to_width;

pub const MIN_COLUMNS_WIDTH: u16 = 110;
pub const COLUMN_MIN_WIDTH: u16 = 22;
pub const PICKER_CMD_BASE: u8 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BoardLayout {
    #[default]
    Columns,
    List,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveOutcome {
    OpenReadyFlow,
    OpenRevokeFlow,
    ConfirmCancel,
}

pub fn move_outcome(mv: &Move) -> MoveOutcome {
    match mv.via {
        MoveVia::Ready => MoveOutcome::OpenReadyFlow,
        MoveVia::RevokeReady => MoveOutcome::OpenRevokeFlow,
        MoveVia::Cancel => MoveOutcome::ConfirmCancel,
    }
}

pub fn move_label(mv: &Move) -> String {
    match mv.via {
        MoveVia::Ready => "Ready for development (Gate 2)".to_string(),
        MoveVia::RevokeReady => format!("Back to {}", mv.to.label()),
        MoveVia::Cancel => "Cancel idea".to_string(),
    }
}

pub fn state_from_key(key: &str) -> Option<IdeaState> {
    COLUMN_ORDER.into_iter().find(|s| state_key(*s) == key)
}

pub fn column_states(summary: &FactorySummary) -> Vec<IdeaState> {
    let mut states: Vec<IdeaState> = Vec::new();
    for key in &summary.columns {
        if let Some(state) = state_from_key(key)
            && !states.contains(&state)
        {
            states.push(state);
        }
    }
    if states.is_empty() {
        states = COLUMN_ORDER.to_vec();
    }
    for state in COLUMN_ORDER {
        if !states.contains(&state) && summary.ideas.iter().any(|i| i.state == state) {
            states.push(state);
        }
    }
    states
}

pub fn short_label(state: IdeaState, width: u16) -> &'static str {
    if width >= 28 {
        return state.label();
    }
    match state {
        IdeaState::ReadyForDevelopment => "Ready",
        IdeaState::InDevelopment => "In dev",
        other => other.label(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovePicker {
    pub idea_id: String,
    pub title: String,
    pub moves: Vec<Move>,
    pub selected: usize,
}

pub enum PickerOutcome {
    Consumed,
    Cancel,
    Choose(Move),
}

impl MovePicker {
    pub fn new(idea: &IdeaCard) -> Self {
        Self {
            idea_id: idea.id.clone(),
            title: clean(idea.title.trim()),
            moves: idea.moves.clone(),
            selected: 0,
        }
    }

    pub fn labels(&self) -> Vec<String> {
        self.moves.iter().map(move_label).collect()
    }

    pub fn handle_key(&mut self, event: &KeyEvent) -> PickerOutcome {
        match event.code {
            KeyCode::Esc => PickerOutcome::Cancel,
            KeyCode::Left | KeyCode::Up | KeyCode::Char('h') | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
                PickerOutcome::Consumed
            }
            KeyCode::Right | KeyCode::Down | KeyCode::Char('l') | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(self.moves.len().saturating_sub(1));
                PickerOutcome::Consumed
            }
            KeyCode::Enter => match self.moves.get(self.selected) {
                Some(mv) => PickerOutcome::Choose(*mv),
                None => PickerOutcome::Cancel,
            },
            _ => PickerOutcome::Consumed,
        }
    }

    pub fn line(&self) -> (String, Vec<(String, usize)>) {
        let mut text = format!(" Move {} to: ", self.title);
        let mut spans = Vec::new();
        for (i, label) in self.labels().into_iter().enumerate() {
            let shown = if i == self.selected {
                format!("‹{label}›")
            } else {
                format!(" {label} ")
            };
            spans.push((shown.clone(), i));
            text.push_str(&shown);
            text.push(' ');
        }
        text.push_str("  ←/→ choose  Enter confirm  Esc cancel");
        (text, spans)
    }
}

pub fn picker_hits(line_area: Rect, text: &str, spans: &[(String, usize)], hits: &mut HitMap) {
    for (needle, idx) in spans {
        hits.tags(
            line_area,
            text,
            &[(needle.as_str(), Hit::Cmd(PICKER_CMD_BASE + *idx as u8))],
        );
    }
}

pub struct ColumnView<'a> {
    pub state: IdeaState,
    pub ideas: Vec<(usize, &'a IdeaCard)>,
}

pub fn visible_column_count(width: u16, total: usize) -> usize {
    ((width / COLUMN_MIN_WIDTH) as usize).clamp(1, total.max(1))
}

pub fn first_visible(first: usize, focus: usize, visible: usize, total: usize) -> usize {
    let mut first = first.min(total.saturating_sub(visible));
    if focus < first {
        first = focus;
    } else if focus >= first + visible {
        first = focus + 1 - visible;
    }
    first.min(total.saturating_sub(visible))
}

fn risk_label(risk: Option<Risk>) -> &'static str {
    match risk {
        None => "-",
        Some(Risk::Low) => "low",
        Some(Risk::Medium) => "med",
        Some(Risk::High) => "high",
    }
}

fn meta_line(idea: &IdeaCard) -> String {
    let mut parts = vec![risk_label(idea.risk).to_string()];
    if idea.blockers > 0 {
        parts.push(format!("{} open", idea.blockers));
    }
    if idea.team_count > 0 {
        parts.push(format!("{} active", idea.team_count));
    }
    if idea.needs_sign_in {
        parts.push("sign in".to_string());
    }
    if idea.moves.iter().any(|m| m.via == MoveVia::Ready) {
        parts.push("Gate 2".to_string());
    }
    parts.join(" · ")
}

pub struct ColumnsRender<'a> {
    pub columns: &'a [ColumnView<'a>],
    pub first: usize,
    pub focus: IdeaState,
    pub selected_id: Option<&'a str>,
}

pub fn render_columns(area: Rect, buf: &mut Buffer, view: &ColumnsRender<'_>, hits: &mut HitMap) {
    let total = view.columns.len();
    if total == 0 || area.width == 0 || area.height == 0 {
        return;
    }
    let visible = visible_column_count(area.width, total);
    let col_w = area.width / visible as u16;
    for slot in 0..visible {
        let Some(column) = view.columns.get(view.first + slot) else {
            break;
        };
        let x = area.x + slot as u16 * col_w;
        let w = if slot + 1 == visible {
            area.width - slot as u16 * col_w
        } else {
            col_w
        };
        let focused = column.state == view.focus;
        let header = format!(
            "{} ({})",
            short_label(column.state, col_w),
            column.ideas.len()
        );
        let header_style = if focused {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        };
        buf.set_stringn(
            x,
            area.y,
            truncate_to_width(&header, w.saturating_sub(1) as usize),
            w.saturating_sub(1) as usize,
            header_style,
        );
        hits.push(
            Rect::new(x, area.y, w.saturating_sub(1), 1),
            Hit::Cmd((view.first + slot) as u8),
        );

        let rows = area.height.saturating_sub(1) as usize;
        let cap = (rows / 2).max(1);
        let sel_pos = column
            .ideas
            .iter()
            .position(|(_, i)| Some(i.id.as_str()) == view.selected_id);
        let start = sel_pos.map(|p| (p + 1).saturating_sub(cap)).unwrap_or(0);
        for (n, (index, idea)) in column.ideas.iter().skip(start).take(cap).enumerate() {
            let y = area.y + 1 + (n as u16) * 2;
            let is_sel = Some(idea.id.as_str()) == view.selected_id;
            let style = if is_sel {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(Color::White)
            };
            let marker = if is_sel { "▶" } else { " " };
            let inner = w.saturating_sub(2) as usize;
            let title = clean(idea.title.trim());
            let title = if title.is_empty() {
                "(not described)".to_string()
            } else {
                title
            };
            buf.set_stringn(
                x,
                y,
                truncate_to_width(&format!("{marker}{title}"), inner),
                inner,
                style,
            );
            buf.set_stringn(
                x,
                y + 1,
                truncate_to_width(&format!(" {}", meta_line(idea)), inner),
                inner,
                Style::default().fg(Color::DarkGray),
            );
            hits.push(Rect::new(x, y, w.saturating_sub(1), 2), Hit::Row(*index));
        }
    }
}
