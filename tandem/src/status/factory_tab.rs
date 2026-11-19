use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::thread;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout as RLayout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::StatefulWidget;
use unicode_width::UnicodeWidthStr;

use crate::adapters::base::Model;
use crate::coordination::schema::Query;
use crate::factory::model::{IdeaState, Risk};
use crate::factory::questions;
use crate::sanitize::clean;
use crate::snapshot::{FactorySummary, IdeaCard, StatusSnapshot};
use crate::status::factory_audit::{AuditKeyOutcome, FactoryAudit};
use crate::status::factory_detail::{DetailAction, DetailKeyOutcome, FactoryDetail};
use crate::status::factory_run::{FactoryRun, RunKeyOutcome};
use crate::status::render::age;
use crate::tmux::keys::{FACTORY_NEW_IDEA_KEY, display_shortcut, matches_crossterm_event};
use crate::ui::input::TextInput;
use crate::ui::option_list::{OptionItem, OptionList, OptionListState};
use crate::ui::text::truncate_to_width;
use crate::wizard::app::ModelLoader;
use crate::wizard::runner::{installed_clis, load_models};

const ORCHESTRATOR_CLIS: [&str; 2] = ["claude", "agy"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FactoryView {
    #[default]
    Board,
    NewIdea,
    Detail,
    RunMonitor,
    Audit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IdeaFilter {
    #[default]
    All,
    NeedsYou,
    State(IdeaState),
}

impl IdeaFilter {
    pub fn cycle_next(&mut self) {
        *self = match *self {
            Self::All => Self::NeedsYou,
            Self::NeedsYou => Self::State(IdeaState::Inbox),
            Self::State(IdeaState::Inbox) => Self::State(IdeaState::Ideation),
            Self::State(IdeaState::Ideation) => Self::State(IdeaState::Planning),
            Self::State(IdeaState::Planning) => Self::State(IdeaState::ReadyForDevelopment),
            Self::State(IdeaState::ReadyForDevelopment) => Self::State(IdeaState::Queued),
            Self::State(IdeaState::Queued) => Self::State(IdeaState::InDevelopment),
            Self::State(IdeaState::InDevelopment) => Self::State(IdeaState::Review),
            Self::State(IdeaState::Review) => Self::State(IdeaState::Delivered),
            Self::State(IdeaState::Delivered) => Self::State(IdeaState::Cancelled),
            Self::State(IdeaState::Cancelled) => Self::All,
        };
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::All => "All",
            Self::NeedsYou => "Needs you",
            Self::State(s) => s.label(),
        }
    }

    pub fn matches(&self, idea: &IdeaCard, queries: &[Query]) -> bool {
        match self {
            Self::All => true,
            Self::NeedsYou => {
                idea.blockers > 0
                    || questions::for_idea(queries, &idea.id).open > 0
                    || idea
                        .run
                        .as_ref()
                        .is_some_and(|r| r.state == crate::factory::model::RunState::Paused)
            }
            Self::State(s) => idea.state == *s,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewIdeaStep {
    Cli,
    Model,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactoryAction {
    StartIdea { cli: String, model: String },
    OpenOrResumeOrchestrator { idea_id: String },
    Refresh,
}

pub struct NewIdeaModal {
    pub step: NewIdeaStep,
    pub installed: HashMap<String, bool>,
    pub cli_items: Vec<OptionItem>,
    pub cli_state: OptionListState,
    pub chosen_cli: Option<String>,
    pub model_input: TextInput,
    pub model_items: Vec<OptionItem>,
    pub model_state: OptionListState,
    pub models: Vec<Model>,
    pub loading_models: bool,
    model_rx: Option<Receiver<Result<Vec<Model>, String>>>,
    loader: ModelLoader,
    pub message: String,
}

impl std::fmt::Debug for NewIdeaModal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewIdeaModal")
            .field("step", &self.step)
            .field("chosen_cli", &self.chosen_cli)
            .field("loading_models", &self.loading_models)
            .finish()
    }
}

impl NewIdeaModal {
    pub fn new(installed: HashMap<String, bool>, loader: ModelLoader) -> Self {
        let mut cli_items = Vec::new();
        for cli in ORCHESTRATOR_CLIS {
            let ok = installed.get(cli).copied().unwrap_or(false);
            if ok {
                cli_items.push(OptionItem::new(cli, cli));
            } else {
                cli_items.push(OptionItem::disabled(cli, format!("{cli}  (not installed)")));
            }
        }
        let mut cli_state = OptionListState::default();
        cli_state.select_first_selectable(&cli_items);
        let mut model_input = TextInput::with_placeholder("filter or type model id");
        model_input.focused = true;

        Self {
            step: NewIdeaStep::Cli,
            installed,
            cli_items,
            cli_state,
            chosen_cli: None,
            model_input,
            model_items: Vec::new(),
            model_state: OptionListState::default(),
            models: Vec::new(),
            loading_models: false,
            model_rx: None,
            loader,
            message: String::new(),
        }
    }

    pub fn poll_background(&mut self) -> bool {
        let Some(ref rx) = self.model_rx else {
            return false;
        };
        let Ok(res) = rx.try_recv() else {
            return false;
        };
        self.loading_models = false;
        self.model_rx = None;
        match res {
            Ok(models) => {
                self.models = models;
                self.message.clear();
            }
            Err(err) => {
                self.models.clear();
                self.message = format!(
                    "Could not list models ({}). Type a model ID and press Enter.",
                    clean(&err)
                );
            }
        }
        let needle = self.model_input.value.clone();
        self.fill_models(&needle);
        true
    }

    fn start_model_load(&mut self, cli: &str) {
        self.loading_models = true;
        self.message = "Loading models…".to_string();
        self.models.clear();
        self.model_items.clear();
        self.model_input.clear();
        let (tx, rx) = channel();
        self.model_rx = Some(rx);
        let cli_owned = cli.to_string();
        let loader = self.loader.clone();
        thread::spawn(move || {
            let res = loader(&cli_owned);
            let _ = tx.send(res);
        });
    }

    fn fill_models(&mut self, needle: &str) {
        let q = needle.trim().to_ascii_lowercase();
        self.model_items.clear();
        for m in &self.models {
            if q.is_empty()
                || m.id.to_ascii_lowercase().contains(&q)
                || m.label.to_ascii_lowercase().contains(&q)
            {
                self.model_items.push(OptionItem::new(&m.id, &m.label));
            }
        }
        self.model_state.select_first_selectable(&self.model_items);
    }

    fn handle_key(&mut self, event: &KeyEvent) -> ModalKeyOutcome {
        match self.step {
            NewIdeaStep::Cli => match event.code {
                KeyCode::Esc => ModalKeyOutcome::Cancel,
                KeyCode::Up => {
                    self.cli_state.select_prev(&self.cli_items);
                    ModalKeyOutcome::Consumed
                }
                KeyCode::Down => {
                    self.cli_state.select_next(&self.cli_items);
                    ModalKeyOutcome::Consumed
                }
                KeyCode::Enter => {
                    let Some(idx) = self.cli_state.selected else {
                        self.message = "No supported CLI is installed (claude or agy).".to_string();
                        return ModalKeyOutcome::Consumed;
                    };
                    let item = &self.cli_items[idx];
                    if item.disabled {
                        self.message = format!("{} is not installed.", item.id);
                        return ModalKeyOutcome::Consumed;
                    }
                    let cli = item.id.clone();
                    self.chosen_cli = Some(cli.clone());
                    self.step = NewIdeaStep::Model;
                    self.start_model_load(&cli);
                    ModalKeyOutcome::Consumed
                }
                _ => ModalKeyOutcome::Consumed,
            },
            NewIdeaStep::Model => match event.code {
                KeyCode::Esc => {
                    self.step = NewIdeaStep::Cli;
                    self.message.clear();
                    self.model_rx = None;
                    self.loading_models = false;
                    ModalKeyOutcome::Consumed
                }
                KeyCode::Up => {
                    self.model_state.select_prev(&self.model_items);
                    ModalKeyOutcome::Consumed
                }
                KeyCode::Down => {
                    self.model_state.select_next(&self.model_items);
                    ModalKeyOutcome::Consumed
                }
                KeyCode::Enter => {
                    let selected_model = self
                        .model_state
                        .selected
                        .and_then(|i| self.model_items.get(i))
                        .map(|it| it.id.clone())
                        .filter(|s| !s.is_empty());
                    let typed = self.model_input.value.trim().to_string();
                    let model = selected_model.unwrap_or(typed);
                    if model.is_empty() {
                        self.message = "Enter or select a model id.".to_string();
                        return ModalKeyOutcome::Consumed;
                    }
                    let cli = self.chosen_cli.clone().unwrap_or_default();
                    ModalKeyOutcome::Confirm { cli, model }
                }
                _ => {
                    if self.model_input.handle_key(event) {
                        let needle = self.model_input.value.clone();
                        self.fill_models(&needle);
                    }
                    ModalKeyOutcome::Consumed
                }
            },
        }
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        let max_w = area.width as usize;
        let header_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        let label_style = Style::default().fg(Color::Cyan);
        let dim_style = Style::default().fg(Color::DarkGray);
        let warn_style = Style::default().fg(Color::Yellow);

        write_clipped(buf, area.x, area.y, max_w, "New idea", header_style);
        if area.height == 1 {
            return;
        }

        let show_hint = area.height >= 4;
        let body_bottom = if show_hint {
            area.y + area.height - 1
        } else {
            area.y + area.height
        };
        let mut y = area.y + 1;

        match self.step {
            NewIdeaStep::Cli => {
                if y < body_bottom {
                    write_clipped(buf, area.x, y, max_w, "Orchestrator CLI", label_style);
                    y += 1;
                }
                let list_h = body_bottom
                    .saturating_sub(y)
                    .saturating_sub(u16::from(!self.message.is_empty()));
                if list_h > 0 {
                    let list_rect = Rect::new(area.x, y, area.width, list_h);
                    OptionList::new(&self.cli_items).render(list_rect, buf, &mut self.cli_state);
                    y += list_h;
                }
                if !self.message.is_empty() && y < body_bottom {
                    write_clipped(buf, area.x, y, max_w, &self.message, warn_style);
                }
            }
            NewIdeaStep::Model => {
                if y < body_bottom {
                    let cli = self.chosen_cli.as_deref().unwrap_or("");
                    write_clipped(
                        buf,
                        area.x,
                        y,
                        max_w,
                        &format!("Orchestrator CLI: {cli}   Model"),
                        label_style,
                    );
                    y += 1;
                }
                let remain = body_bottom.saturating_sub(y);
                if remain >= 2 {
                    let chunks = RLayout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(1), Constraint::Min(1)])
                        .split(Rect::new(area.x, y, area.width, remain));
                    self.model_input.render_with_block(chunks[0], buf, None);
                    let list_h = chunks[1]
                        .height
                        .saturating_sub(u16::from(!self.message.is_empty()));
                    if list_h > 0 {
                        let list_rect =
                            Rect::new(chunks[1].x, chunks[1].y, chunks[1].width, list_h);
                        OptionList::new(&self.model_items).render(
                            list_rect,
                            buf,
                            &mut self.model_state,
                        );
                    }
                    if !self.message.is_empty() && chunks[1].height > list_h {
                        write_clipped(
                            buf,
                            chunks[1].x,
                            chunks[1].y + list_h,
                            max_w,
                            &self.message,
                            warn_style,
                        );
                    }
                }
            }
        }

        if show_hint {
            let hint = match self.step {
                NewIdeaStep::Cli => "↑/↓ choose CLI   Enter next   Esc cancel",
                NewIdeaStep::Model => "↑/↓ choose model   type to filter   Enter start   Esc back",
            };
            write_clipped(
                buf,
                area.x,
                area.y + area.height - 1,
                max_w,
                hint,
                dim_style,
            );
        }
    }
}

enum ModalKeyOutcome {
    Consumed,
    Cancel,
    Confirm { cli: String, model: String },
}

pub struct FactoryTab {
    pub selected: Option<usize>,
    pub selected_id: Option<String>,
    pub scroll_offset: usize,
    pub view: FactoryView,
    pub filter: IdeaFilter,
    pub detail: Option<FactoryDetail>,
    pub run_monitor: Option<FactoryRun>,
    pub audit: Option<FactoryAudit>,
    pub new_idea: Option<NewIdeaModal>,
    pub notice: String,
    pub platform: String,
    pub workspace: Option<PathBuf>,
    pub installed: HashMap<String, bool>,
    pub model_loader: ModelLoader,
}

impl std::fmt::Debug for FactoryTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FactoryTab")
            .field("selected", &self.selected)
            .field("selected_id", &self.selected_id)
            .field("view", &self.view)
            .field("filter", &self.filter)
            .finish()
    }
}

impl Default for FactoryTab {
    fn default() -> Self {
        Self::new(None, installed_clis(), Arc::new(load_models))
    }
}

impl FactoryTab {
    pub fn new(
        workspace: Option<PathBuf>,
        installed: HashMap<String, bool>,
        model_loader: ModelLoader,
    ) -> Self {
        Self {
            selected: None,
            selected_id: None,
            scroll_offset: 0,
            view: FactoryView::Board,
            filter: IdeaFilter::All,
            detail: None,
            run_monitor: None,
            audit: None,
            new_idea: None,
            notice: String::new(),
            platform: std::env::consts::OS.to_string(),
            workspace,
            installed,
            model_loader,
        }
    }

    pub fn captures_input(&self) -> bool {
        match self.view {
            FactoryView::Board => false,
            FactoryView::NewIdea => self
                .new_idea
                .as_ref()
                .is_some_and(|m| m.step == NewIdeaStep::Model),
            FactoryView::Detail => self.detail.as_ref().is_some_and(|d| d.captures_input()),
            FactoryView::RunMonitor => self
                .run_monitor
                .as_ref()
                .is_some_and(|m| m.captures_input()),
            FactoryView::Audit => false,
        }
    }

    pub fn poll_background(&mut self) -> bool {
        if let Some(ref mut modal) = self.new_idea {
            return modal.poll_background();
        }
        false
    }

    pub fn open_new_idea(&mut self) {
        self.notice.clear();
        self.new_idea = Some(NewIdeaModal::new(
            self.installed.clone(),
            self.model_loader.clone(),
        ));
        self.view = FactoryView::NewIdea;
    }

    pub fn filtered_ideas<'a>(&self, snapshot: &'a StatusSnapshot) -> Vec<&'a IdeaCard> {
        let Some(summary) = snapshot.factory.as_ref() else {
            return Vec::new();
        };
        summary
            .ideas
            .iter()
            .filter(|idea| self.filter.matches(idea, &snapshot.queries))
            .collect()
    }

    pub fn sync(&mut self, summary: Option<&FactorySummary>) {
        self.sync_list(summary, &[]);
    }

    pub fn sync_snapshot(&mut self, snapshot: &StatusSnapshot, pane_open: bool) {
        self.sync_list(snapshot.factory.as_ref(), &snapshot.queries);
        if self.view == FactoryView::Detail
            && let Some(ref mut detail) = self.detail
        {
            let card = snapshot
                .factory
                .as_ref()
                .and_then(|f| f.ideas.iter().find(|i| i.id == detail.idea_id));
            detail.sync_if_changed(card, &snapshot.queries, pane_open);
        }
        if self.view == FactoryView::RunMonitor
            && let Some(ref mut monitor) = self.run_monitor
        {
            let run_summary = snapshot
                .factory
                .as_ref()
                .and_then(|f| f.ideas.iter().find(|i| i.id == monitor.idea_id))
                .and_then(|i| i.run.as_ref());
            monitor.sync_if_changed(run_summary);
        }
    }

    fn sync_list(&mut self, summary: Option<&FactorySummary>, queries: &[Query]) {
        let Some(summary) = summary else {
            self.selected = None;
            self.selected_id = None;
            self.scroll_offset = 0;
            return;
        };
        if summary.error.is_some() {
            self.selected = None;
            self.selected_id = None;
            self.scroll_offset = 0;
            return;
        }
        let filtered: Vec<&IdeaCard> = summary
            .ideas
            .iter()
            .filter(|idea| self.filter.matches(idea, queries))
            .collect();
        if filtered.is_empty() {
            self.selected = None;
            self.selected_id = None;
            self.scroll_offset = 0;
            return;
        }
        if let Some(ref target_id) = self.selected_id
            && let Some(pos) = filtered.iter().position(|idea| &idea.id == target_id)
        {
            self.selected = Some(pos);
            return;
        }
        self.selected = Some(0);
        self.selected_id = Some(filtered[0].id.clone());
    }

    pub fn select_next(&mut self, ideas: &[IdeaCard]) {
        if ideas.is_empty() {
            return;
        }
        let next = match self.selected {
            Some(idx) => (idx + 1).min(ideas.len() - 1),
            None => 0,
        };
        self.selected = Some(next);
        self.selected_id = Some(ideas[next].id.clone());
    }

    pub fn select_prev(&mut self, ideas: &[IdeaCard]) {
        if ideas.is_empty() {
            return;
        }
        let prev = match self.selected {
            Some(idx) => idx.saturating_sub(1),
            None => 0,
        };
        self.selected = Some(prev);
        self.selected_id = Some(ideas[prev].id.clone());
    }

    fn select_next_refs(&mut self, ideas: &[&IdeaCard]) {
        if ideas.is_empty() {
            return;
        }
        let next = match self.selected {
            Some(idx) => (idx + 1).min(ideas.len() - 1),
            None => 0,
        };
        self.selected = Some(next);
        self.selected_id = Some(ideas[next].id.clone());
    }

    fn select_prev_refs(&mut self, ideas: &[&IdeaCard]) {
        if ideas.is_empty() {
            return;
        }
        let prev = match self.selected {
            Some(idx) => idx.saturating_sub(1),
            None => 0,
        };
        self.selected = Some(prev);
        self.selected_id = Some(ideas[prev].id.clone());
    }

    pub fn handle_key(
        &mut self,
        event: &KeyEvent,
        snapshot: &StatusSnapshot,
    ) -> Option<FactoryAction> {
        match self.view {
            FactoryView::NewIdea => {
                let Some(ref mut modal) = self.new_idea else {
                    self.view = FactoryView::Board;
                    return None;
                };
                match modal.handle_key(event) {
                    ModalKeyOutcome::Consumed => None,
                    ModalKeyOutcome::Cancel => {
                        self.new_idea = None;
                        self.view = FactoryView::Board;
                        None
                    }
                    ModalKeyOutcome::Confirm { cli, model } => {
                        self.new_idea = None;
                        self.view = FactoryView::Board;
                        Some(FactoryAction::StartIdea { cli, model })
                    }
                }
            }
            FactoryView::Detail => {
                let Some(ref mut detail) = self.detail else {
                    self.view = FactoryView::Board;
                    return None;
                };
                match detail.handle_key(event) {
                    DetailKeyOutcome::BackToBoard => {
                        self.detail = None;
                        self.view = FactoryView::Board;
                        None
                    }
                    DetailKeyOutcome::Action(DetailAction::OpenOrResumeOrchestrator {
                        idea_id,
                    }) => Some(FactoryAction::OpenOrResumeOrchestrator { idea_id }),
                    DetailKeyOutcome::Action(DetailAction::OpenRunMonitor { run_id }) => {
                        if let Some(ref ws) = self.workspace {
                            let monitor =
                                FactoryRun::load(ws, &detail.idea_id, &run_id, &self.platform);
                            self.run_monitor = Some(monitor);
                            self.view = FactoryView::RunMonitor;
                        }
                        None
                    }
                    DetailKeyOutcome::Action(DetailAction::OpenRunEvidence { run_id }) => {
                        if let Some(ref ws) = self.workspace {
                            let mut monitor =
                                FactoryRun::load(ws, &detail.idea_id, &run_id, &self.platform);
                            monitor.open_evidence_view();
                            self.run_monitor = Some(monitor);
                            self.view = FactoryView::RunMonitor;
                        }
                        None
                    }
                    DetailKeyOutcome::Action(DetailAction::OpenAudit { idea_id }) => {
                        if let Some(ref ws) = self.workspace {
                            self.audit = Some(FactoryAudit::load(ws, &idea_id));
                            self.view = FactoryView::Audit;
                        }
                        None
                    }
                    DetailKeyOutcome::Action(DetailAction::Refresh) => Some(FactoryAction::Refresh),
                    DetailKeyOutcome::Consumed | DetailKeyOutcome::Ignored => None,
                }
            }
            FactoryView::RunMonitor => {
                let Some(ref mut monitor) = self.run_monitor else {
                    self.view = FactoryView::Detail;
                    return None;
                };
                monitor.platform = self.platform.clone();
                match monitor.handle_key(event) {
                    RunKeyOutcome::BackToDetail => {
                        self.run_monitor = None;
                        if let Some(ref mut detail) = self.detail {
                            let pane_open = detail.pane_open;
                            detail.reload(&snapshot.queries, pane_open);
                            self.view = FactoryView::Detail;
                        } else {
                            self.view = FactoryView::Board;
                        }
                        None
                    }
                    RunKeyOutcome::Refresh => Some(FactoryAction::Refresh),
                    RunKeyOutcome::Consumed | RunKeyOutcome::Ignored => None,
                }
            }
            FactoryView::Audit => {
                let Some(ref mut audit) = self.audit else {
                    self.view = FactoryView::Detail;
                    return None;
                };
                match audit.handle_key(*event) {
                    AuditKeyOutcome::BackToDetail => {
                        self.audit = None;
                        self.view = if self.detail.is_some() {
                            FactoryView::Detail
                        } else {
                            FactoryView::Board
                        };
                        None
                    }
                    AuditKeyOutcome::Consumed | AuditKeyOutcome::Ignored => None,
                }
            }
            FactoryView::Board => {
                if matches_crossterm_event(FACTORY_NEW_IDEA_KEY, event, &self.platform) {
                    if snapshot.factory.is_some() {
                        self.open_new_idea();
                    }
                    return None;
                }
                let filtered = self.filtered_ideas(snapshot);
                match event.code {
                    KeyCode::Up => {
                        self.select_prev_refs(&filtered);
                        None
                    }
                    KeyCode::Down => {
                        self.select_next_refs(&filtered);
                        None
                    }
                    KeyCode::Char('f') => {
                        self.filter.cycle_next();
                        self.sync_list(snapshot.factory.as_ref(), &snapshot.queries);
                        None
                    }
                    KeyCode::Char('n') => {
                        if snapshot.factory.is_some() {
                            self.open_new_idea();
                        }
                        None
                    }
                    KeyCode::Enter => {
                        let idx = self.selected?;
                        let card = filtered.get(idx)?;
                        let Some(ref ws) = self.workspace else {
                            self.notice = "Workspace path unavailable for detail view.".to_string();
                            return None;
                        };
                        let mut detail =
                            FactoryDetail::load(ws, &card.id, &snapshot.queries, false);
                        detail.platform = self.platform.clone();
                        self.detail = Some(detail);
                        self.view = FactoryView::Detail;
                        None
                    }
                    _ => None,
                }
            }
        }
    }

    pub fn render(&mut self, snapshot: &StatusSnapshot, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        match self.view {
            FactoryView::NewIdea => {
                if let Some(ref mut modal) = self.new_idea {
                    modal.render(area, buf);
                }
                return;
            }
            FactoryView::Detail => {
                if let Some(ref mut detail) = self.detail {
                    detail.render(snapshot.now, area, buf);
                }
                return;
            }
            FactoryView::RunMonitor => {
                if let Some(ref mut monitor) = self.run_monitor {
                    monitor.render(snapshot.now, area, buf);
                }
                return;
            }
            FactoryView::Audit => {
                if let Some(ref mut audit) = self.audit {
                    audit.render(area, buf);
                }
                return;
            }
            FactoryView::Board => {}
        }

        let max_w = area.width as usize;
        let header_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        let col_style = Style::default().fg(Color::DarkGray);
        let dim_style = Style::default().fg(Color::DarkGray);
        let err_style = Style::default().fg(Color::Red);

        let Some(summary) = snapshot.factory.as_ref() else {
            write_clipped(
                buf,
                area.x,
                area.y,
                max_w,
                "FACTORY   factory mode is disabled",
                dim_style,
            );
            return;
        };

        if let Some(ref err) = summary.error {
            let msg = format!(
                "FACTORY   could not read .agent-workspace/factory ({})",
                clean(err)
            );
            write_clipped(buf, area.x, area.y, max_w, &msg, err_style);
            return;
        }

        let new_key = display_shortcut(FACTORY_NEW_IDEA_KEY, &self.platform)
            .unwrap_or_else(|_| FACTORY_NEW_IDEA_KEY.to_string());

        if summary.ideas.is_empty() {
            write_clipped(
                buf,
                area.x,
                area.y,
                max_w,
                &format!("FACTORY   ideas 0   {new_key} new idea"),
                header_style,
            );
            if area.height >= 2 {
                write_clipped(buf, area.x, area.y + 1, max_w, "No ideas yet.", dim_style);
            }
            if area.height >= 3 {
                write_clipped(
                    buf,
                    area.x,
                    area.y + 2,
                    max_w,
                    "Create one with: tandem factory idea new --title \"…\" --text \"…\"",
                    dim_style,
                );
            }
            return;
        }

        let filtered = self.filtered_ideas(snapshot);
        let header = if area.width >= 100 {
            format!(
                "FACTORY   ideas {}   needs you {}   filter: {} ▾   {} new idea   Enter open",
                summary.ideas.len(),
                summary.needs_you,
                self.filter.label(),
                new_key,
            )
        } else {
            format!(
                "FACTORY   ideas {}   needs you {}   filter: {} ▾",
                summary.ideas.len(),
                summary.needs_you,
                self.filter.label(),
            )
        };
        write_clipped(buf, area.x, area.y, max_w, &header, header_style);
        if area.height == 1 {
            return;
        }

        if filtered.is_empty() {
            if area.height >= 2 {
                write_clipped(
                    buf,
                    area.x,
                    area.y + 1,
                    max_w,
                    &format!(
                        "No ideas match filter: {}. Press f to cycle.",
                        self.filter.label()
                    ),
                    dim_style,
                );
            }
            return;
        }

        let narrow = area.width < 80;
        let wide = area.width >= 120;
        let show_hint = area.height >= 4;

        let mut y = area.y + 1;
        let bottom = if show_hint {
            area.y + area.height - 1
        } else {
            area.y + area.height
        };

        if !narrow && area.height >= 3 {
            let col_header = if wide {
                format!(
                    " {:<12} {:<22} {:<6} {:<9} {:<6} {:<9} TITLE",
                    "ID", "STATE", "RISK", "BLOCKERS", "OWNER", "UPDATED"
                )
            } else {
                format!(
                    " {:<12} {:<22} {:<5} {:<9} {:<8} TITLE",
                    "ID", "STATE", "RISK", "BLOCKERS", "UPDATED"
                )
            };
            write_clipped(buf, area.x, y, max_w, &col_header, col_style);
            y += 1;
        }

        let rows_avail = bottom.saturating_sub(y) as usize;
        let lines_per_idea = if narrow { 2 } else { 1 };
        let visible_ideas = (rows_avail / lines_per_idea).max(usize::from(rows_avail > 0));

        if let Some(sel) = self.selected {
            if sel < self.scroll_offset {
                self.scroll_offset = sel;
            } else if visible_ideas > 0 && sel >= self.scroll_offset + visible_ideas {
                self.scroll_offset = sel + 1 - visible_ideas;
            }
        }
        if self.scroll_offset >= filtered.len() {
            self.scroll_offset = filtered.len().saturating_sub(1);
        }

        for (idx, idea) in filtered.iter().enumerate().skip(self.scroll_offset) {
            if y >= bottom {
                break;
            }
            let is_sel = self.selected == Some(idx);
            let marker = if is_sel { "▶" } else { " " };
            let style = if is_sel {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let updated = format!("{} ago", age(snapshot.now - idea.updated));
            let risk = match idea.risk {
                None => "-",
                Some(Risk::Low) => "low",
                Some(Risk::Medium) => "medium",
                Some(Risk::High) => "high",
            };
            let blockers_str = if idea.blockers > 0 {
                format!("{} ?", idea.blockers)
            } else {
                "0".to_string()
            };
            let state = if let Some(ref run) = idea.run {
                format!(
                    "{} {}/{}",
                    idea.state.label(),
                    run.stages_done,
                    run.stages_total
                )
            } else {
                idea.state.label().to_string()
            };
            let clean_title = clean(&idea.title);
            let clean_owner = clean(&idea.owner);

            if narrow {
                let narrow_blockers = if idea.blockers > 0 {
                    format!("{} ? blockers", idea.blockers)
                } else {
                    "0 blockers".to_string()
                };
                let line1 = format!(
                    "{marker}{:<11} {:<22} {:<5} {narrow_blockers}",
                    truncate_to_width(&idea.id, 11),
                    truncate_to_width(&state, 22),
                    risk,
                );
                write_clipped(buf, area.x, y, max_w, &line1, style);
                y += 1;
                if y < bottom {
                    let suffix = format!(" · {updated}");
                    let title_budget = max_w.saturating_sub(3 + suffix.width());
                    let t = truncate_to_width(&clean_title, title_budget);
                    let line2 = format!("   {t}{suffix}");
                    write_clipped(buf, area.x, y, max_w, &line2, style);
                    y += 1;
                }
            } else {
                let prefix = if wide {
                    format!(
                        "{marker}{:<12} {:<22} {:<6} {:<9} {:<6} {:<9} ",
                        truncate_to_width(&idea.id, 12),
                        truncate_to_width(&state, 22),
                        risk,
                        blockers_str,
                        truncate_to_width(&clean_owner, 6),
                        truncate_to_width(&updated, 9),
                    )
                } else {
                    format!(
                        "{marker}{:<12} {:<22} {:<5} {:<9} {:<8} ",
                        truncate_to_width(&idea.id, 12),
                        truncate_to_width(&state, 22),
                        risk,
                        blockers_str,
                        truncate_to_width(&updated, 8),
                    )
                };
                let title_budget = max_w.saturating_sub(prefix.width());
                let t = truncate_to_width(&clean_title, title_budget);
                let line = format!("{prefix}{t}");
                write_clipped(buf, area.x, y, max_w, &line, style);
                y += 1;
            }
        }

        if show_hint {
            let hint = if self.notice.is_empty() {
                format!(" ↑/↓ move   f filter   {new_key} new idea   Enter open")
            } else {
                format!(" {}", clean(&self.notice))
            };
            write_clipped(
                buf,
                area.x,
                area.y + area.height - 1,
                max_w,
                &hint,
                dim_style,
            );
        }
    }
}

fn write_clipped(buf: &mut Buffer, x: u16, y: u16, max_w: usize, text: &str, style: Style) {
    let clipped = truncate_to_width(text, max_w);
    buf.set_stringn(x, y, clipped, max_w, style);
}
