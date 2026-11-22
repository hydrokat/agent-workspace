use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::factory::auth_risk::RiskItem;
use crate::sanitize::clean;

const MODAL_WIDTH: u16 = 84;
const MIN_HEIGHT: u16 = 12;
const PROCEED_HINT: &str = "[Enter] I understand, proceed";
const CANCEL_HINT: &str = "[Esc] Cancel";

pub fn risk_block_message(risks: &[RiskItem], cli: &str) -> Option<String> {
    let risk = risks
        .iter()
        .find(|r| r.cli == cli && r.risk.as_deref() == Some("risky"))?;
    Some(format!(
        "{cli} uses a risky sign-in ({}, {}). Confirm it in Settings → Agents & sign-in first.",
        risk.method,
        risk.verdict.as_deref().unwrap_or("no verified terms")
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RiskConfirmOutcome {
    Pending,
    Cancelled,
    Confirmed,
}

#[derive(Debug, Clone)]
pub struct RiskConfirm {
    pub item: RiskItem,
    pub input: String,
    pub error: Option<String>,
}

impl RiskConfirm {
    pub fn new(item: RiskItem) -> Self {
        Self {
            item,
            input: String::new(),
            error: None,
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> RiskConfirmOutcome {
        match key.code {
            KeyCode::Esc => RiskConfirmOutcome::Cancelled,
            KeyCode::Enter => {
                if self.input.trim() == self.item.cli {
                    RiskConfirmOutcome::Confirmed
                } else {
                    self.error = Some(format!("Type {} to confirm.", self.item.cli));
                    RiskConfirmOutcome::Pending
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
                self.error = None;
                RiskConfirmOutcome::Pending
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.input.push(c);
                self.error = None;
                RiskConfirmOutcome::Pending
            }
            _ => RiskConfirmOutcome::Pending,
        }
    }

    fn body_lines(&self, inner: usize) -> Vec<(String, Style)> {
        let warn = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let plain = Style::default();
        let dim = Style::default().fg(Color::DarkGray);
        let verdict = self
            .item
            .verdict
            .as_deref()
            .map_or("NO VERIFIED TERMS".to_string(), str::to_uppercase);
        let mut paragraphs: Vec<(String, Style)> = vec![(
            format!("Terms verdict: {verdict} for orchestrated use."),
            warn,
        )];
        if let Some(reason) = &self.item.reason {
            paragraphs.push((reason.clone(), plain));
        }
        if let Some(conditions) = &self.item.conditions {
            paragraphs.push((format!("Terms: {conditions}"), dim));
        }
        paragraphs.push((
            "Your account may be limited or suspended. Safer: sign in with an API key (Settings → Agents & sign-in)."
                .to_string(),
            plain,
        ));
        if let (Some(verified), Some(reverify)) = (&self.item.verified_on, &self.item.reverify_by) {
            paragraphs.push((
                format!(
                    "Terms verified {verified}, re-verify by {reverify}. You are asked again if they change."
                ),
                dim,
            ));
        }
        let mut lines = Vec::new();
        for (text, style) in paragraphs {
            for wrapped in crate::status::factory_detail::wrap_text(&clean(&text), inner.max(1)) {
                lines.push((wrapped, style));
            }
        }
        lines
    }

    pub fn rect(&self, area: Rect) -> Rect {
        let width = MODAL_WIDTH.min(area.width);
        let inner = (width as usize).saturating_sub(3);
        let body = self.body_lines(inner).len() as u16;
        let height = (body + 6).max(MIN_HEIGHT).min(area.height);
        Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        )
    }

    pub fn key_for_click(&self, mouse: &MouseEvent, area: Rect) -> Option<KeyEvent> {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            return None;
        }
        let r = self.rect(area);
        if r.height < 4 || mouse.row != r.y + r.height - 2 {
            return None;
        }
        let proceed_end = r.x + 2 + PROCEED_HINT.chars().count() as u16;
        let cancel_start = proceed_end + 3;
        let cancel_end = cancel_start + CANCEL_HINT.chars().count() as u16;
        if mouse.column >= r.x + 2 && mouse.column < proceed_end {
            Some(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
        } else if mouse.column >= cancel_start && mouse.column < cancel_end {
            Some(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        } else {
            None
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let r = self.rect(area);
        if r.width < 20 || r.height < 8 {
            return;
        }
        for row in r.y..r.y + r.height {
            for col in r.x..r.x + r.width {
                if let Some(cell) = buf.cell_mut((col, row)) {
                    cell.set_symbol(" ");
                    cell.set_style(Style::default().bg(Color::Black));
                }
            }
        }
        let w = r.width as usize;
        let inner = w.saturating_sub(3);
        let warn = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let dim = Style::default().fg(Color::DarkGray);

        buf.set_stringn(
            r.x,
            r.y,
            format!(
                "┌ ⚠ Risky sign-in: {} ({}) ",
                self.item.cli, self.item.method
            ),
            w,
            warn,
        );
        let mut y = r.y + 1;
        for (line, style) in self.body_lines(inner) {
            if y >= r.y + r.height - 4 {
                break;
            }
            buf.set_stringn(r.x + 2, y, line, inner, style);
            y += 1;
        }
        let prompt_y = r.y + r.height - 4;
        buf.set_stringn(
            r.x + 2,
            prompt_y,
            format!("Type {} to use it anyway: {}_", self.item.cli, self.input),
            inner,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
        if let Some(err) = &self.error {
            buf.set_stringn(
                r.x + 2,
                prompt_y + 1,
                err,
                inner,
                Style::default().fg(Color::Red),
            );
        }
        buf.set_stringn(
            r.x + 2,
            r.y + r.height - 2,
            format!("{PROCEED_HINT}   {CANCEL_HINT}"),
            inner,
            dim,
        );
        buf.set_stringn(
            r.x,
            r.y + r.height - 1,
            format!("└{:─<pad$}┘", "", pad = w.saturating_sub(2)),
            w,
            warn,
        );
    }
}
