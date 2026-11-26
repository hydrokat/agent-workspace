use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::factory::approval::Caller;
use crate::factory::model::ModelDecisionAction;
use crate::factory::model_catalog::{
    CatalogModel, ModelDriftItem, ModelFreshnessStatus, ModelSource, ModelVerificationReport,
};
use crate::factory::routing::ModelTier;
use crate::factory::settings::apply_model_decision;
use crate::factory::store::FactoryStore;
use crate::sanitize::clean;
use crate::ui::hits::{Hit, HitMap, Resolved};
use crate::ui::text::truncate_to_width;

const MODAL_MAX_WIDTH: u16 = 78;
const MODAL_MIN_HEIGHT: u16 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelPickerOutcome {
    Pending,
    Cancelled,
    Completed { saved: usize, summary: String },
}

#[derive(Debug, Clone, PartialEq)]
enum PickerChoice {
    UseModel {
        id: String,
        tier: ModelTier,
        source: ModelSource,
        recommended: bool,
    },
    KeepCurrent(String),
}

#[derive(Debug, Clone)]
pub struct ModelUpdatePicker {
    pub workspace: PathBuf,
    pub items: Vec<ModelDriftItem>,
    pub current_idx: usize,
    pub selected_option: usize,
    pub saved_count: usize,
    pub error: Option<String>,
    pub hits: HitMap,
}

impl ModelUpdatePicker {
    pub fn new(workspace: PathBuf, items: Vec<ModelDriftItem>) -> Self {
        Self {
            workspace,
            items,
            current_idx: 0,
            selected_option: 0,
            saved_count: 0,
            error: None,
            hits: HitMap::default(),
        }
    }

    pub fn from_report(workspace: PathBuf, report: &ModelVerificationReport) -> Option<Self> {
        let items: Vec<ModelDriftItem> = report
            .items
            .iter()
            .filter(|i| i.needs_attention)
            .cloned()
            .collect();
        (!items.is_empty()).then(|| Self::new(workspace, items))
    }

    pub fn current_item(&self) -> Option<&ModelDriftItem> {
        self.items.get(self.current_idx)
    }

    fn choices_for(item: &ModelDriftItem) -> Vec<PickerChoice> {
        let rec = item.recommended_model().map(str::to_string);
        let mut choices: Vec<PickerChoice> = item
            .available_models
            .iter()
            .filter(|m| m.allowed && m.id != item.configured)
            .map(|m: &CatalogModel| PickerChoice::UseModel {
                id: m.id.clone(),
                tier: m.tier,
                source: m.source,
                recommended: rec.as_deref() == Some(m.id.as_str()),
            })
            .collect();
        choices.sort_by(|a, b| match (a, b) {
            (
                PickerChoice::UseModel {
                    recommended: ra, ..
                },
                PickerChoice::UseModel {
                    recommended: rb, ..
                },
            ) => rb.cmp(ra),
            _ => std::cmp::Ordering::Equal,
        });
        choices.push(PickerChoice::KeepCurrent(item.configured.clone()));
        choices
    }

    fn persist_choice(&mut self, chosen: &str, action: ModelDecisionAction) -> ModelPickerOutcome {
        let Some(item) = self.current_item().cloned() else {
            return ModelPickerOutcome::Completed {
                saved: self.saved_count,
                summary: "No model updates pending.".to_string(),
            };
        };

        let store = FactoryStore::new(&self.workspace);
        let owner = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .unwrap_or_else(|_| "you".to_string());
        let env_fn = |k: &str| std::env::var(k).ok();
        let no_pane = |_: &str| None;
        let caller = Caller::tui(&env_fn, &no_pane);

        if let Err(err) = apply_model_decision(
            &store,
            &item.cli,
            item.tier,
            chosen,
            action,
            &item.available_signature,
            &owner,
            &caller,
        ) {
            self.error = Some(clean(&err.to_string()));
            return ModelPickerOutcome::Pending;
        }

        self.error = None;
        self.saved_count += 1;
        self.current_idx += 1;
        self.selected_option = 0;

        if self.current_idx >= self.items.len() {
            ModelPickerOutcome::Completed {
                saved: self.saved_count,
                summary: format!(
                    "Saved {} model decision(s) to settings.json.",
                    self.saved_count
                ),
            }
        } else {
            ModelPickerOutcome::Pending
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> ModelPickerOutcome {
        let Some(item) = self.current_item().cloned() else {
            return ModelPickerOutcome::Completed {
                saved: self.saved_count,
                summary: "No model updates pending.".to_string(),
            };
        };
        let choices = Self::choices_for(&item);
        let len = choices.len().max(1);

        match key.code {
            KeyCode::Esc => ModelPickerOutcome::Cancelled,
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected_option = if self.selected_option == 0 {
                    len - 1
                } else {
                    self.selected_option - 1
                };
                self.error = None;
                ModelPickerOutcome::Pending
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected_option = (self.selected_option + 1) % len;
                self.error = None;
                ModelPickerOutcome::Pending
            }
            KeyCode::Char('s') => self.persist_choice(&item.configured, ModelDecisionAction::Kept),
            KeyCode::Enter => match choices.get(self.selected_option) {
                Some(PickerChoice::UseModel { id, .. }) => {
                    self.persist_choice(id, ModelDecisionAction::Updated)
                }
                Some(PickerChoice::KeepCurrent(cur)) => {
                    self.persist_choice(cur, ModelDecisionAction::Kept)
                }
                None => ModelPickerOutcome::Pending,
            },
            _ => ModelPickerOutcome::Pending,
        }
    }

    pub fn mouse_key(&mut self, event: &MouseEvent) -> Option<KeyEvent> {
        match self.hits.resolve(event, KeyCode::Up, KeyCode::Down)? {
            Resolved::Key(k) => Some(k),
            Resolved::Row(idx) => {
                if self.selected_option == idx {
                    Some(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                } else {
                    self.selected_option = idx;
                    self.error = None;
                    None
                }
            }
            Resolved::Cmd(_) => None,
        }
    }

    pub fn rect(&self, area: Rect) -> Rect {
        let width = MODAL_MAX_WIDTH.min(area.width);
        let option_count = self
            .current_item()
            .map(|i| Self::choices_for(i).len())
            .unwrap_or(1)
            .min(10) as u16;
        let height = (option_count + 7).max(MODAL_MIN_HEIGHT).min(area.height);
        Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        )
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        self.hits.clear();
        let Some(item) = self.current_item().cloned() else {
            return;
        };
        let r = self.rect(area);
        if r.width < 28 || r.height < 8 {
            return;
        }
        for row in r.y..r.y + r.height {
            for col in r.x..r.x + r.width {
                if let Some(cell) = buf.cell_mut((col, row)) {
                    cell.set_symbol(" ");
                    cell.set_style(Style::default().bg(Color::Black).fg(Color::White));
                }
            }
        }

        let border = Style::default().fg(Color::Cyan);
        let top = format!("┌{:─<w$}┐", "", w = (r.width as usize).saturating_sub(2));
        let bot = format!("└{:─<w$}┘", "", w = (r.width as usize).saturating_sub(2));
        buf.set_stringn(r.x, r.y, &top, r.width as usize, border);
        buf.set_stringn(r.x, r.y + r.height - 1, &bot, r.width as usize, border);
        for row in r.y + 1..r.y + r.height - 1 {
            buf.set_string(r.x, row, "│", border);
            buf.set_string(r.x + r.width - 1, row, "│", border);
        }

        let inner_w = (r.width as usize).saturating_sub(4);
        let ix = r.x + 2;
        let tier_str = match item.tier {
            ModelTier::Economy => "economy",
            ModelTier::Standard => "standard",
            ModelTier::Premium => "premium",
        };
        let title = format!(
            "Update Model ({}/{}) — {} · {tier_str}",
            self.current_idx + 1,
            self.items.len(),
            item.cli
        );
        buf.set_stringn(
            ix,
            r.y + 1,
            truncate_to_width(&title, inner_w),
            inner_w,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );

        let status_label = match &item.status {
            ModelFreshnessStatus::MissingFromCatalog => "missing from live CLI catalog",
            ModelFreshnessStatus::NewerAvailable { .. } => "newer available",
            ModelFreshnessStatus::Current => "up to date",
        };
        let sub = format!("Current: {} ({status_label})", clean(&item.configured));
        buf.set_stringn(
            ix,
            r.y + 2,
            truncate_to_width(&sub, inner_w),
            inner_w,
            Style::default().fg(Color::Yellow),
        );

        let choices = Self::choices_for(&item);
        let wide = area.width >= 80;
        let list_start = r.y + 4;
        let list_end = r.y
            + r.height
                .saturating_sub(if self.error.is_some() { 3 } else { 2 });
        let visible_cap = list_end.saturating_sub(list_start) as usize;
        let start = if visible_cap > 0 && self.selected_option >= visible_cap {
            self.selected_option + 1 - visible_cap
        } else {
            0
        };

        for (idx, choice) in choices.iter().enumerate().skip(start) {
            let line_y = list_start + (idx - start) as u16;
            if line_y >= list_end {
                break;
            }
            let is_sel = idx == self.selected_option;
            let marker = if is_sel { "▶ " } else { "  " };
            let text = match choice {
                PickerChoice::UseModel {
                    id,
                    tier,
                    source,
                    recommended,
                } => {
                    let t = format!("{tier:?}").to_lowercase();
                    let rec = if *recommended { " · recommended" } else { "" };
                    if wide {
                        let src = match source {
                            ModelSource::Live => "live",
                            ModelSource::Cache => "cache",
                            ModelSource::Fallback => "fallback",
                        };
                        format!("{marker}{id:<26} ({t}{rec} · {src})")
                    } else if *recommended {
                        format!("{marker}{id} (recommended)")
                    } else {
                        format!("{marker}{id}")
                    }
                }
                PickerChoice::KeepCurrent(cur) => format!("{marker}Keep current ({})", clean(cur)),
            };
            let style = if is_sel {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(Color::White)
            };
            buf.set_stringn(
                ix,
                line_y,
                truncate_to_width(&text, inner_w),
                inner_w,
                style,
            );
            self.hits
                .push(Rect::new(ix, line_y, inner_w as u16, 1), Hit::Row(idx));
        }

        if let Some(ref err) = self.error
            && r.height >= 6
        {
            let err_y = r.y + r.height - 3;
            buf.set_stringn(
                ix,
                err_y,
                truncate_to_width(&clean(err), inner_w),
                inner_w,
                Style::default().fg(Color::Red),
            );
        }

        let hint_y = r.y + r.height - 2;
        let hint = if wide {
            "Enter apply   s keep current   Esc dismiss"
        } else {
            "Enter apply  s keep  Esc dismiss"
        };
        buf.set_stringn(
            ix,
            hint_y,
            truncate_to_width(hint, inner_w),
            inner_w,
            Style::default().fg(Color::DarkGray),
        );
        self.hits.tags(
            Rect::new(ix, hint_y, inner_w as u16, 1),
            hint,
            &[
                ("Enter apply", Hit::key(KeyCode::Enter)),
                (if wide { "s keep current" } else { "s keep" }, Hit::ch('s')),
                ("Esc dismiss", Hit::key(KeyCode::Esc)),
            ],
        );
    }
}
