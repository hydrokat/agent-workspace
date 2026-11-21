use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::thread;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout as RLayout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::StatefulWidget;
use unicode_width::UnicodeWidthStr;

use crate::adapters::base::Model;
use crate::coordination::schema::Query;
use crate::factory::model::{IdeaState, Risk, validate_text, validate_title};
use crate::factory::questions;
use crate::sanitize::clean;
use crate::snapshot::{FactorySummary, IdeaCard, StatusSnapshot};
use crate::status::factory_agents::{AgentsKeyOutcome, FactoryAgents};
use crate::status::factory_audit::{AuditKeyOutcome, FactoryAudit};
use crate::status::factory_board::{
    BoardLayout, ColumnView, ColumnsRender, MIN_COLUMNS_WIDTH, MoveOutcome, MovePicker,
    PICKER_CMD_BASE, PickerOutcome, column_states, first_visible, move_outcome, picker_hits,
    render_columns, short_label, visible_column_count,
};
use crate::status::factory_detail::{DetailAction, DetailKeyOutcome, FactoryDetail};
use crate::status::factory_run::{FactoryRun, RunKeyOutcome};
use crate::status::render::age;
use crate::tmux::keys::{FACTORY_NEW_IDEA_KEY, display_shortcut, matches_crossterm_event};
use crate::ui::hits::{Hit, HitMap, Resolved};
use crate::ui::input::TextInput;
use crate::ui::option_list::{OptionItem, OptionList, OptionListState};
use crate::ui::text::truncate_to_width;
use crate::wizard::app::ModelLoader;
use crate::wizard::runner::{installed_clis, load_models};

pub const ORCHESTRATOR_CLIS: [&str; 3] = ["claude", "agy", "opencode"];
pub const BOARD_PAGE_SIZE: usize = 200;
pub const ARCHIVE_AFTER_SECS: f64 = 30.0 * 86_400.0;

pub fn is_archived(idea: &IdeaCard, now: f64) -> bool {
    now > 0.0
        && matches!(idea.state, IdeaState::Delivered | IdeaState::Cancelled)
        && (now - idea.updated) > ARCHIVE_AFTER_SECS
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FactoryView {
    #[default]
    Board,
    NewIdea,
    Detail,
    RunMonitor,
    Audit,
    Agents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IdeaFilter {
    #[default]
    All,
    NeedsYou,
    State(IdeaState),
    Archived,
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
            Self::State(IdeaState::Cancelled) => Self::Archived,
            Self::Archived => Self::All,
        };
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::All => "All",
            Self::NeedsYou => "Needs you",
            Self::State(s) => s.label(),
            Self::Archived => "Archived",
        }
    }

    pub fn matches(&self, idea: &IdeaCard, queries: &[Query]) -> bool {
        self.matches_at(idea, queries, 0.0)
    }

    pub fn matches_at(&self, idea: &IdeaCard, queries: &[Query], now: f64) -> bool {
        match self {
            Self::All => !is_archived(idea, now),
            Self::NeedsYou => {
                !is_archived(idea, now)
                    && (idea.blockers > 0
                        || questions::for_idea(queries, &idea.id).open > 0
                        || idea
                            .run
                            .as_ref()
                            .is_some_and(|r| r.state == crate::factory::model::RunState::Paused))
            }
            Self::State(s) => idea.state == *s,
            Self::Archived => is_archived(idea, now),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewIdeaStep {
    Title,
    Description,
    Cli,
    Model,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactoryAction {
    StartIdea {
        cli: String,
        model: String,
        title: Option<String>,
        text: String,
    },
    CancelIdea {
        idea_id: String,
    },
    OpenOrResumeOrchestrator {
        idea_id: String,
    },
    OpenWorkerPane {
        pane_id: String,
    },
    OpenLoginPane {
        cli: String,
    },
    Refresh,
}

pub struct NewIdeaModal {
    pub step: NewIdeaStep,
    pub title_input: TextInput,
    pub description: String,
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
    pub hits: HitMap,
    pub list_rect: Rect,
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

        let mut title_input = TextInput::with_placeholder("short summary");
        title_input.focused = true;

        Self {
            step: NewIdeaStep::Title,
            title_input,
            description: String::new(),
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
            hits: HitMap::default(),
            list_rect: Rect::default(),
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
            NewIdeaStep::Title => match event.code {
                KeyCode::Esc => ModalKeyOutcome::Cancel,
                KeyCode::Enter => {
                    let title = self.title_input.value.trim().to_string();
                    if title.is_empty() {
                        self.message = "Title is required.".to_string();
                        return ModalKeyOutcome::Consumed;
                    }
                    if let Err(err) = validate_title(&title) {
                        self.message = clean(&err.0);
                        return ModalKeyOutcome::Consumed;
                    }
                    self.message.clear();
                    self.step = NewIdeaStep::Description;
                    ModalKeyOutcome::Consumed
                }
                _ => {
                    self.title_input.handle_key(event);
                    ModalKeyOutcome::Consumed
                }
            },
            NewIdeaStep::Description => {
                let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
                match event.code {
                    KeyCode::Esc => {
                        self.message.clear();
                        self.step = NewIdeaStep::Title;
                    }
                    KeyCode::Enter if ctrl => return self.finish_description(),
                    KeyCode::Char('d') if ctrl => return self.finish_description(),
                    KeyCode::Enter => self.description.push('\n'),
                    KeyCode::Backspace => {
                        self.description.pop();
                    }
                    KeyCode::Char(c) if !ctrl => self.description.push(c),
                    _ => {}
                }
                ModalKeyOutcome::Consumed
            }
            NewIdeaStep::Cli => match event.code {
                KeyCode::Esc => {
                    self.message.clear();
                    self.step = NewIdeaStep::Description;
                    ModalKeyOutcome::Consumed
                }
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
                        self.message =
                            "No supported CLI is installed (claude, agy, or opencode).".to_string();
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
                    ModalKeyOutcome::Confirm {
                        cli,
                        model,
                        title: self.title_input.value.trim().to_string(),
                        text: self.description.trim().to_string(),
                    }
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

    fn finish_description(&mut self) -> ModalKeyOutcome {
        if let Err(err) = validate_text(self.description.trim()) {
            self.message = clean(&err.0);
            return ModalKeyOutcome::Consumed;
        }
        self.message.clear();
        self.step = NewIdeaStep::Cli;
        ModalKeyOutcome::Consumed
    }

    fn list_item_at(&self, col: u16, row: u16) -> Option<usize> {
        let r = self.list_rect;
        if r.width == 0 || col < r.x || col >= r.x + r.width || row < r.y || row >= r.y + r.height {
            return None;
        }
        let (offset, len) = match self.step {
            NewIdeaStep::Title | NewIdeaStep::Description => return None,
            NewIdeaStep::Cli => (self.cli_state.scroll_offset, self.cli_items.len()),
            NewIdeaStep::Model => (self.model_state.scroll_offset, self.model_items.len()),
        };
        let idx = offset + (row - r.y) as usize;
        (idx < len).then_some(idx)
    }

    fn mouse_key(&mut self, event: &MouseEvent) -> Option<KeyEvent> {
        let enter = KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
        if event.kind == MouseEventKind::Down(MouseButton::Left)
            && let Some(idx) = self.list_item_at(event.column, event.row)
        {
            return match self.step {
                NewIdeaStep::Title | NewIdeaStep::Description => None,
                NewIdeaStep::Cli => {
                    let item = &self.cli_items[idx];
                    if item.disabled {
                        self.message = format!("{} is not installed.", item.id);
                        None
                    } else {
                        self.cli_state.selected = Some(idx);
                        Some(enter)
                    }
                }
                NewIdeaStep::Model if self.model_state.selected == Some(idx) => Some(enter),
                NewIdeaStep::Model => {
                    self.model_state.selected = Some(idx);
                    None
                }
            };
        }
        match self.hits.resolve(event, KeyCode::Up, KeyCode::Down)? {
            Resolved::Key(key) => Some(key),
            Resolved::Row(_) | Resolved::Cmd(_) => None,
        }
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        self.hits.clear();
        self.list_rect = Rect::default();
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
            NewIdeaStep::Title => {
                if y < body_bottom {
                    write_clipped(buf, area.x, y, max_w, "Title", label_style);
                    y += 1;
                }
                if y < body_bottom {
                    self.title_input.render_with_block(
                        Rect::new(area.x, y, area.width, 1),
                        buf,
                        None,
                    );
                    y += 1;
                }
                if !self.message.is_empty() && y < body_bottom {
                    write_clipped(buf, area.x, y, max_w, &self.message, warn_style);
                }
            }
            NewIdeaStep::Description => {
                if y < body_bottom {
                    let count = self.description.chars().count();
                    write_clipped(
                        buf,
                        area.x,
                        y,
                        max_w,
                        &format!("Description (optional)   {count}/8000"),
                        label_style,
                    );
                    y += 1;
                }
                let message_rows = u16::from(!self.message.is_empty());
                let text_rows = body_bottom.saturating_sub(y).saturating_sub(message_rows);
                let lines: Vec<&str> = self.description.split('\n').collect();
                let first = lines.len().saturating_sub(text_rows as usize);
                for (row, line) in lines.iter().skip(first).enumerate() {
                    let last = first + row + 1 == lines.len();
                    let shown = if last {
                        format!("{}▏", clean(line))
                    } else {
                        clean(line)
                    };
                    write_clipped(buf, area.x, y + row as u16, max_w, &shown, Style::default());
                }
                y += text_rows;
                if !self.message.is_empty() && y < body_bottom {
                    write_clipped(buf, area.x, y, max_w, &self.message, warn_style);
                }
            }
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
                    self.list_rect = list_rect;
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
                        self.list_rect = list_rect;
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
                NewIdeaStep::Title => "type a title   Enter next   Esc cancel",
                NewIdeaStep::Description => {
                    "type a description   Enter new line   Ctrl+Enter done   Esc back"
                }
                NewIdeaStep::Cli => "↑/↓ choose CLI   Enter next   Esc back",
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
            let line = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
            let enter = Hit::key(KeyCode::Enter);
            let esc = Hit::key(KeyCode::Esc);
            match self.step {
                NewIdeaStep::Title => {
                    self.hits
                        .tags(line, hint, &[("Enter next", enter), ("Esc cancel", esc)])
                }
                NewIdeaStep::Description => self.hits.tags(
                    line,
                    hint,
                    &[
                        (
                            "Ctrl+Enter done",
                            Hit::Key(KeyCode::Enter, KeyModifiers::CONTROL),
                        ),
                        ("Esc back", esc),
                    ],
                ),
                NewIdeaStep::Cli => self.hits.tags(
                    line,
                    hint,
                    &[
                        ("↑/↓ choose CLI", Hit::key(KeyCode::Down)),
                        ("Enter next", enter),
                        ("Esc back", esc),
                    ],
                ),
                NewIdeaStep::Model => self.hits.tags(
                    line,
                    hint,
                    &[
                        ("↑/↓ choose model", Hit::key(KeyCode::Down)),
                        ("Enter start", enter),
                        ("Esc back", esc),
                    ],
                ),
            }
        }
    }
}

enum ModalKeyOutcome {
    Consumed,
    Cancel,
    Confirm {
        cli: String,
        model: String,
        title: String,
        text: String,
    },
}

pub struct FactoryTab {
    pub selected: Option<usize>,
    pub selected_id: Option<String>,
    pub scroll_offset: usize,
    pub page: usize,
    pub view: FactoryView,
    pub filter: IdeaFilter,
    pub layout: BoardLayout,
    pub column: IdeaState,
    pub column_touched: bool,
    pub first_column: usize,
    pub last_width: u16,
    pub move_picker: Option<MovePicker>,
    pub cancel_confirm: Option<(String, String)>,
    pub detail: Option<FactoryDetail>,
    pub run_monitor: Option<FactoryRun>,
    pub audit: Option<FactoryAudit>,
    pub agents: Option<FactoryAgents>,
    pub return_to_agents: bool,
    pub new_idea: Option<NewIdeaModal>,
    pub notice: String,
    pub platform: String,
    pub workspace: Option<PathBuf>,
    pub installed: HashMap<String, bool>,
    pub model_loader: ModelLoader,
    pub hits: HitMap,
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
            page: 0,
            view: FactoryView::Board,
            filter: IdeaFilter::All,
            layout: BoardLayout::default(),
            column: IdeaState::Inbox,
            column_touched: false,
            first_column: 0,
            last_width: 0,
            move_picker: None,
            cancel_confirm: None,
            detail: None,
            run_monitor: None,
            audit: None,
            agents: None,
            return_to_agents: false,
            new_idea: None,
            notice: String::new(),
            platform: std::env::consts::OS.to_string(),
            workspace,
            installed,
            model_loader,
            hits: HitMap::default(),
        }
    }

    pub fn captures_input(&self) -> bool {
        match self.view {
            FactoryView::Board => false,
            FactoryView::NewIdea => self.new_idea.as_ref().is_some_and(|m| {
                matches!(
                    m.step,
                    NewIdeaStep::Title | NewIdeaStep::Description | NewIdeaStep::Model
                )
            }),
            FactoryView::Detail => self.detail.as_ref().is_some_and(|d| d.captures_input()),
            FactoryView::RunMonitor => self
                .run_monitor
                .as_ref()
                .is_some_and(|m| m.captures_input()),
            FactoryView::Audit => false,
            FactoryView::Agents => self.agents.as_ref().is_some_and(|a| a.captures_input()),
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

    pub fn focus_idea(&mut self, idea_id: &str) {
        self.notice.clear();
        self.new_idea = None;
        self.view = FactoryView::Board;
        self.filter = IdeaFilter::All;
        self.selected_id = Some(idea_id.to_string());
    }

    pub fn column_restricted(&self) -> bool {
        self.layout == BoardLayout::Columns
            && self.last_width != 0
            && self.last_width < MIN_COLUMNS_WIDTH
    }

    fn all_filtered<'a>(&self, snapshot: &'a StatusSnapshot) -> Vec<&'a IdeaCard> {
        let Some(summary) = snapshot.factory.as_ref() else {
            return Vec::new();
        };
        summary
            .ideas
            .iter()
            .filter(|idea| {
                self.filter
                    .matches_at(idea, &snapshot.queries, snapshot.now)
            })
            .collect()
    }

    pub fn column_members<'a>(&self, snapshot: &'a StatusSnapshot) -> Vec<&'a IdeaCard> {
        let mut members: Vec<&IdeaCard> = self
            .all_filtered(snapshot)
            .into_iter()
            .filter(|idea| idea.state == self.column)
            .collect();
        members.sort_by(|a, b| b.updated.total_cmp(&a.updated));
        members
    }

    pub fn filtered_ideas<'a>(&self, snapshot: &'a StatusSnapshot) -> Vec<&'a IdeaCard> {
        if self.column_restricted() {
            return self.column_members(snapshot);
        }
        self.all_filtered(snapshot)
    }

    fn select_id(&mut self, snapshot: &StatusSnapshot, id: Option<String>) {
        self.selected_id = id;
        let list = self.filtered_ideas(snapshot);
        self.selected = self
            .selected_id
            .as_ref()
            .and_then(|id| list.iter().position(|i| &i.id == id));
        if let Some(pos) = self.selected {
            self.page = pos / BOARD_PAGE_SIZE;
        }
    }

    fn shift_column(&mut self, delta: isize, snapshot: &StatusSnapshot) {
        let Some(summary) = snapshot.factory.as_ref() else {
            return;
        };
        let states = column_states(summary);
        let current = states.iter().position(|s| *s == self.column).unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, states.len() as isize - 1) as usize;
        self.column = states[next];
        self.column_touched = true;
        let first = self.column_members(snapshot).first().map(|i| i.id.clone());
        self.select_id(snapshot, first);
    }

    fn shift_card(&mut self, delta: isize, snapshot: &StatusSnapshot) {
        let members = self.column_members(snapshot);
        if members.is_empty() {
            return;
        }
        let current = self
            .selected_id
            .as_ref()
            .and_then(|id| members.iter().position(|i| &i.id == id))
            .map_or(0, |p| p as isize);
        let next = (current + delta).clamp(0, members.len() as isize - 1) as usize;
        let id = members[next].id.clone();
        self.select_id(snapshot, Some(id));
    }

    fn selected_card<'a>(&self, snapshot: &'a StatusSnapshot) -> Option<&'a IdeaCard> {
        let id = self.selected_id.as_ref()?;
        self.all_filtered(snapshot)
            .into_iter()
            .find(|i| &i.id == id)
    }

    fn open_detail_for(&mut self, idea_id: &str, snapshot: &StatusSnapshot) -> bool {
        let Some(ref ws) = self.workspace else {
            self.notice = "Workspace path unavailable for detail view.".to_string();
            return false;
        };
        let mut detail = FactoryDetail::load(ws, idea_id, &snapshot.queries, false);
        detail.platform = self.platform.clone();
        self.detail = Some(detail);
        self.view = FactoryView::Detail;
        true
    }

    fn open_move_picker(&mut self, snapshot: &StatusSnapshot) {
        let Some(card) = self.selected_card(snapshot) else {
            self.notice = "Select an idea first.".to_string();
            return;
        };
        if card.moves.is_empty() {
            self.notice = format!(
                "{} has no moves from {}.",
                clean(card.title.trim()),
                card.state.label()
            );
            return;
        }
        self.notice.clear();
        self.move_picker = Some(MovePicker::new(card));
    }

    fn apply_move(
        &mut self,
        picker: MovePicker,
        mv: crate::factory::board::Move,
        snapshot: &StatusSnapshot,
    ) {
        match move_outcome(&mv) {
            MoveOutcome::OpenReadyFlow | MoveOutcome::OpenRevokeFlow => {
                if self.open_detail_for(&picker.idea_id, snapshot)
                    && let Some(ref mut detail) = self.detail
                {
                    detail.request_ready_or_revoke();
                }
            }
            MoveOutcome::ConfirmCancel => {
                self.cancel_confirm = Some((picker.idea_id, picker.title));
            }
        }
    }

    pub fn current_page(&self) -> usize {
        self.page
    }

    pub fn page_count(&self, snapshot: &StatusSnapshot) -> usize {
        self.filtered_ideas(snapshot)
            .len()
            .div_ceil(BOARD_PAGE_SIZE)
            .max(1)
    }

    pub fn paged_ideas<'a>(&self, snapshot: &'a StatusSnapshot) -> Vec<&'a IdeaCard> {
        let filtered = self.filtered_ideas(snapshot);
        let total_pages = filtered.len().div_ceil(BOARD_PAGE_SIZE).max(1);
        let page = self.page.min(total_pages - 1);
        let start = page * BOARD_PAGE_SIZE;
        let end = (start + BOARD_PAGE_SIZE).min(filtered.len());
        filtered[start..end].to_vec()
    }

    pub fn sync(&mut self, summary: Option<&FactorySummary>) {
        self.sync_list_at(summary, &[], 0.0);
    }

    pub fn sync_snapshot(&mut self, snapshot: &StatusSnapshot, pane_open: bool) {
        self.sync_list_at(snapshot.factory.as_ref(), &snapshot.queries, snapshot.now);
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

    fn sync_list_at(&mut self, summary: Option<&FactorySummary>, queries: &[Query], now: f64) {
        let Some(summary) = summary else {
            self.selected = None;
            self.selected_id = None;
            self.scroll_offset = 0;
            self.page = 0;
            return;
        };
        if summary.error.is_some() {
            self.selected = None;
            self.selected_id = None;
            self.scroll_offset = 0;
            self.page = 0;
            return;
        }
        let filtered: Vec<&IdeaCard> = summary
            .ideas
            .iter()
            .filter(|idea| self.filter.matches_at(idea, queries, now))
            .collect();
        if filtered.is_empty() {
            self.selected = None;
            self.selected_id = None;
            self.scroll_offset = 0;
            self.page = 0;
            return;
        }
        if self.layout == BoardLayout::Columns {
            self.sync_columns(summary, &filtered);
            return;
        }
        if let Some(ref target_id) = self.selected_id
            && let Some(pos) = filtered.iter().position(|idea| &idea.id == target_id)
        {
            self.selected = Some(pos);
            self.page = pos / BOARD_PAGE_SIZE;
            return;
        }
        self.selected = Some(0);
        self.selected_id = Some(filtered[0].id.clone());
        self.page = 0;
    }

    fn sync_columns(&mut self, summary: &FactorySummary, filtered: &[&IdeaCard]) {
        let followed = self
            .selected_id
            .as_ref()
            .and_then(|id| filtered.iter().find(|i| &i.id == id))
            .map(|card| card.state);
        if let Some(state) = followed {
            self.column = state;
        } else {
            if !self.column_touched {
                let states = column_states(summary);
                if let Some(first) = states
                    .iter()
                    .find(|s| filtered.iter().any(|i| i.state == **s))
                {
                    self.column = *first;
                }
            }
            let mut members: Vec<&&IdeaCard> =
                filtered.iter().filter(|i| i.state == self.column).collect();
            members.sort_by(|a, b| b.updated.total_cmp(&a.updated));
            self.selected_id = members.first().map(|i| i.id.clone());
        }
        let list: Vec<&&IdeaCard> = if self.column_restricted() {
            let mut members: Vec<&&IdeaCard> =
                filtered.iter().filter(|i| i.state == self.column).collect();
            members.sort_by(|a, b| b.updated.total_cmp(&a.updated));
            members
        } else {
            filtered.iter().collect()
        };
        self.selected = self
            .selected_id
            .as_ref()
            .and_then(|id| list.iter().position(|i| &i.id == id));
        if let Some(pos) = self.selected {
            self.page = pos / BOARD_PAGE_SIZE;
        }
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
        self.page = next / BOARD_PAGE_SIZE;
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
        self.page = prev / BOARD_PAGE_SIZE;
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
                    ModalKeyOutcome::Confirm {
                        cli,
                        model,
                        title,
                        text,
                    } => {
                        self.new_idea = None;
                        self.view = FactoryView::Board;
                        Some(FactoryAction::StartIdea {
                            cli,
                            model,
                            title: (!title.is_empty()).then_some(title),
                            text,
                        })
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
                        if self.return_to_agents && self.agents.is_some() {
                            self.return_to_agents = false;
                            self.view = FactoryView::Agents;
                            return None;
                        }
                        if let Some(ref mut detail) = self.detail {
                            let pane_open = detail.pane_open;
                            detail.reload(&snapshot.queries, pane_open);
                            self.view = FactoryView::Detail;
                        } else {
                            self.view = FactoryView::Board;
                        }
                        None
                    }
                    RunKeyOutcome::OpenWorkerPane { pane_id } => {
                        Some(FactoryAction::OpenWorkerPane { pane_id })
                    }
                    RunKeyOutcome::OpenLoginPane { cli } => {
                        Some(FactoryAction::OpenLoginPane { cli })
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
            FactoryView::Agents => {
                let Some(ref mut agents) = self.agents else {
                    self.view = FactoryView::Board;
                    return None;
                };
                match agents.handle_key(event) {
                    AgentsKeyOutcome::Back => {
                        self.agents = None;
                        self.view = FactoryView::Board;
                        None
                    }
                    AgentsKeyOutcome::OpenRun { idea_id, run_id } => {
                        if let Some(ref ws) = self.workspace {
                            self.run_monitor =
                                Some(FactoryRun::load(ws, &idea_id, &run_id, &self.platform));
                            self.return_to_agents = true;
                            self.view = FactoryView::RunMonitor;
                        }
                        None
                    }
                    AgentsKeyOutcome::OpenWorkerPane { pane_id } => {
                        Some(FactoryAction::OpenWorkerPane { pane_id })
                    }
                    AgentsKeyOutcome::Refresh => Some(FactoryAction::Refresh),
                    AgentsKeyOutcome::Consumed | AgentsKeyOutcome::Ignored => None,
                }
            }
            FactoryView::Board => {
                if let Some((idea_id, _)) = self.cancel_confirm.take() {
                    return matches!(event.code, KeyCode::Char('y') | KeyCode::Char('Y'))
                        .then_some(FactoryAction::CancelIdea { idea_id });
                }
                if let Some(mut picker) = self.move_picker.take() {
                    match picker.handle_key(event) {
                        PickerOutcome::Consumed => self.move_picker = Some(picker),
                        PickerOutcome::Cancel => {}
                        PickerOutcome::Choose(mv) => self.apply_move(picker, mv, snapshot),
                    }
                    return None;
                }
                if matches_crossterm_event(FACTORY_NEW_IDEA_KEY, event, &self.platform) {
                    if snapshot.factory.is_some() {
                        self.open_new_idea();
                    }
                    return None;
                }
                let columns = self.layout == BoardLayout::Columns;
                match event.code {
                    KeyCode::Char('v') => {
                        self.layout = match self.layout {
                            BoardLayout::Columns => BoardLayout::List,
                            BoardLayout::List => BoardLayout::Columns,
                        };
                        self.sync_list_at(
                            snapshot.factory.as_ref(),
                            &snapshot.queries,
                            snapshot.now,
                        );
                        return None;
                    }
                    KeyCode::Char('m') => {
                        self.open_move_picker(snapshot);
                        return None;
                    }
                    KeyCode::Char('g') => {
                        match self.workspace {
                            Some(ref ws) => {
                                self.notice.clear();
                                self.agents = Some(FactoryAgents::load(ws));
                                self.view = FactoryView::Agents;
                            }
                            None => {
                                self.notice =
                                    "Workspace path unavailable for agents view.".to_string();
                            }
                        }
                        return None;
                    }
                    KeyCode::Char('h') | KeyCode::Left if columns => {
                        self.shift_column(-1, snapshot);
                        return None;
                    }
                    KeyCode::Char('l') | KeyCode::Right if columns => {
                        self.shift_column(1, snapshot);
                        return None;
                    }
                    KeyCode::Char('k') | KeyCode::Up if columns => {
                        self.shift_card(-1, snapshot);
                        return None;
                    }
                    KeyCode::Char('j') | KeyCode::Down if columns => {
                        self.shift_card(1, snapshot);
                        return None;
                    }
                    _ => {}
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
                    KeyCode::PageDown => {
                        let total_pages = filtered.len().div_ceil(BOARD_PAGE_SIZE).max(1);
                        if self.page + 1 < total_pages {
                            self.page += 1;
                            let start = self.page * BOARD_PAGE_SIZE;
                            if start < filtered.len() {
                                self.selected = Some(start);
                                self.selected_id = Some(filtered[start].id.clone());
                                self.scroll_offset = 0;
                            }
                        }
                        None
                    }
                    KeyCode::PageUp => {
                        if self.page > 0 {
                            self.page -= 1;
                            let start = self.page * BOARD_PAGE_SIZE;
                            if start < filtered.len() {
                                self.selected = Some(start);
                                self.selected_id = Some(filtered[start].id.clone());
                                self.scroll_offset = 0;
                            }
                        }
                        None
                    }
                    KeyCode::Char('f') => {
                        self.filter.cycle_next();
                        self.sync_list_at(
                            snapshot.factory.as_ref(),
                            &snapshot.queries,
                            snapshot.now,
                        );
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
                        let card_id = filtered.get(idx)?.id.clone();
                        self.open_detail_for(&card_id, snapshot);
                        None
                    }
                    _ => None,
                }
            }
        }
    }

    pub fn handle_mouse(
        &mut self,
        event: &MouseEvent,
        snapshot: &StatusSnapshot,
    ) -> Option<FactoryAction> {
        let key = match self.view {
            FactoryView::NewIdea => self.new_idea.as_mut().and_then(|m| m.mouse_key(event)),
            FactoryView::Detail => self.detail.as_mut().and_then(|d| d.mouse_key(event)),
            FactoryView::RunMonitor => self.run_monitor.as_mut().and_then(|r| r.mouse_key(event)),
            FactoryView::Audit => self.audit.as_mut().and_then(|a| a.mouse_key(event)),
            FactoryView::Agents => self.agents.as_mut().and_then(|a| a.mouse_key(event)),
            FactoryView::Board => self.board_mouse_key(event, snapshot),
        }?;
        self.handle_key(&key, snapshot)
    }

    fn board_mouse_key(
        &mut self,
        event: &MouseEvent,
        snapshot: &StatusSnapshot,
    ) -> Option<KeyEvent> {
        match self.hits.resolve(event, KeyCode::Up, KeyCode::Down)? {
            Resolved::Key(key) => Some(key),
            Resolved::Row(idx) => {
                let (card_id, card_state, page) = {
                    let filtered = self.filtered_ideas(snapshot);
                    let card = filtered.get(idx)?;
                    (card.id.clone(), card.state, idx / BOARD_PAGE_SIZE)
                };
                if self.selected == Some(idx) {
                    return Some(KeyEvent::new(
                        KeyCode::Enter,
                        crossterm::event::KeyModifiers::NONE,
                    ));
                }
                self.selected = Some(idx);
                self.selected_id = Some(card_id);
                self.page = page;
                if self.layout == BoardLayout::Columns {
                    self.column = card_state;
                    self.column_touched = true;
                }
                None
            }
            Resolved::Cmd(c) if c >= PICKER_CMD_BASE => {
                let picker = self.move_picker.as_mut()?;
                let idx = (c - PICKER_CMD_BASE) as usize;
                if idx >= picker.moves.len() {
                    return None;
                }
                picker.selected = idx;
                Some(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            }
            Resolved::Cmd(c) => {
                if self.move_picker.is_some() {
                    return None;
                }
                let states = column_states(snapshot.factory.as_ref()?);
                let state = *states.get(c as usize)?;
                self.column = state;
                self.column_touched = true;
                let first = self.column_members(snapshot).first().map(|i| i.id.clone());
                self.select_id(snapshot, first);
                None
            }
        }
    }

    pub fn render(&mut self, snapshot: &StatusSnapshot, area: Rect, buf: &mut Buffer) {
        self.hits.clear();
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
            FactoryView::Agents => {
                if let Some(ref mut agents) = self.agents {
                    agents.render(snapshot.now, area, buf);
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

        let was_restricted = self.column_restricted();
        self.last_width = area.width;
        if was_restricted != self.column_restricted() {
            self.sync_list_at(snapshot.factory.as_ref(), &snapshot.queries, snapshot.now);
        }

        if summary.ideas.is_empty() {
            write_clipped(
                buf,
                area.x,
                area.y,
                max_w,
                &format!("FACTORY   ideas 0   {new_key} new idea"),
                header_style,
            );
            self.hits.tags(
                Rect::new(area.x, area.y, area.width, 1),
                &format!("FACTORY   ideas 0   {new_key} new idea"),
                &[(&format!("{new_key} new idea"), Hit::ch('n'))],
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

        if self.layout == BoardLayout::Columns && area.width >= MIN_COLUMNS_WIDTH {
            self.render_columns_view(snapshot, summary, area, buf, &new_key);
            return;
        }

        let filtered = self.filtered_ideas(snapshot);
        let archived_count = summary
            .ideas
            .iter()
            .filter(|i| is_archived(i, snapshot.now))
            .count();
        let active_count = summary.ideas.len().saturating_sub(archived_count);
        let total_pages = filtered.len().div_ceil(BOARD_PAGE_SIZE).max(1);
        if self.page >= total_pages {
            self.page = total_pages - 1;
        }
        let page_start = self.page * BOARD_PAGE_SIZE;
        let page_end = (page_start + BOARD_PAGE_SIZE).min(filtered.len());
        let page_slice = &filtered[page_start..page_end];

        let ideas_label = if archived_count > 0 {
            format!("ideas {active_count} ({archived_count} archived)")
        } else {
            format!("ideas {}", summary.ideas.len())
        };
        let page_label = if total_pages > 1 {
            format!("   page {}/{}", self.page + 1, total_pages)
        } else {
            String::new()
        };
        let column_tag = if self.column_restricted() {
            format!(
                "   {} ({}) h/l",
                short_label(self.column, 40),
                filtered.len()
            )
        } else {
            String::new()
        };
        let header = if area.width >= 100 {
            format!(
                "FACTORY   {ideas_label}   needs you {}   filter: {} ▾{page_label}{column_tag}   {new_key} new idea   Enter open",
                summary.needs_you,
                self.filter.label(),
            )
        } else {
            format!(
                "FACTORY   {ideas_label}   needs you {}   filter: {} ▾{page_label}{column_tag}",
                summary.needs_you,
                self.filter.label(),
            )
        };
        write_clipped(buf, area.x, area.y, max_w, &header, header_style);
        self.hits.tags(
            Rect::new(area.x, area.y, area.width, 1),
            &header,
            &[
                (&format!("filter: {} ▾", self.filter.label()), Hit::ch('f')),
                (&format!("{new_key} new idea"), Hit::ch('n')),
                ("Enter open", Hit::key(KeyCode::Enter)),
                (page_label.trim(), Hit::key(KeyCode::PageDown)),
            ],
        );
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

        let page_selected = self
            .selected
            .and_then(|sel| sel.checked_sub(page_start))
            .filter(|&rel| rel < page_slice.len());
        if let Some(sel) = page_selected {
            if sel < self.scroll_offset {
                self.scroll_offset = sel;
            } else if visible_ideas > 0 && sel >= self.scroll_offset + visible_ideas {
                self.scroll_offset = sel + 1 - visible_ideas;
            }
        }
        if self.scroll_offset >= page_slice.len() {
            self.scroll_offset = page_slice.len().saturating_sub(1);
        }

        for (idx, idea) in page_slice.iter().enumerate().skip(self.scroll_offset) {
            if y >= bottom {
                break;
            }
            let is_sel = page_selected == Some(idx);
            let row_lines = if narrow { 2 } else { 1 };
            self.hits.push(
                Rect::new(
                    area.x,
                    y,
                    area.width,
                    row_lines.min(bottom.saturating_sub(y)),
                ),
                Hit::Row(page_start + idx),
            );
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
            let mut state = if let Some(ref run) = idea.run {
                format!(
                    "{} {}/{}",
                    idea.state.label(),
                    run.stages_done,
                    run.stages_total
                )
            } else {
                idea.state.label().to_string()
            };
            if idea.team_count > 0 {
                state.push_str(&format!(" · {}a", idea.team_count));
            }
            if idea.needs_sign_in {
                state.push_str(" · sign in");
            }
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
            self.render_hint_line(area, buf, &new_key);
        }
    }

    fn render_hint_line(&mut self, area: Rect, buf: &mut Buffer, new_key: &str) {
        let max_w = area.width as usize;
        let dim_style = Style::default().fg(Color::DarkGray);
        let y = area.y + area.height - 1;
        let line = Rect::new(area.x, y, area.width, 1);
        if let Some((ref idea_id, ref title)) = self.cancel_confirm {
            let text = format!(
                " Cancel idea {idea_id} \"{title}\"?   y confirm   n keep (any other key keeps it)"
            );
            write_clipped(
                buf,
                area.x,
                y,
                max_w,
                &text,
                Style::default().fg(Color::Yellow),
            );
            self.hits.tags(
                line,
                &text,
                &[("y confirm", Hit::ch('y')), ("n keep", Hit::ch('n'))],
            );
            return;
        }
        if let Some(ref picker) = self.move_picker {
            let (text, spans) = picker.line();
            write_clipped(
                buf,
                area.x,
                y,
                max_w,
                &text,
                Style::default().fg(Color::Yellow),
            );
            picker_hits(line, &text, &spans, &mut self.hits);
            return;
        }
        let columns = self.layout == BoardLayout::Columns;
        let hint = if !self.notice.is_empty() {
            format!(" {}", clean(&self.notice))
        } else if columns {
            format!(
                " h/l column   j/k card   m move   v list   g agents   f filter   {new_key} new idea   Enter open"
            )
        } else {
            format!(
                " ↑/↓ move   f filter   {new_key} new idea   Enter open   m move   v board   g agents"
            )
        };
        write_clipped(buf, area.x, y, max_w, &hint, dim_style);
        if self.notice.is_empty() {
            let new_label = format!("{new_key} new idea");
            let mut tags: Vec<(&str, Hit)> = vec![
                ("f filter", Hit::ch('f')),
                (new_label.as_str(), Hit::ch('n')),
                ("Enter open", Hit::key(KeyCode::Enter)),
                ("m move", Hit::ch('m')),
            ];
            if columns {
                tags.push(("h/l column", Hit::key(KeyCode::Right)));
                tags.push(("j/k card", Hit::key(KeyCode::Down)));
                tags.push(("v list", Hit::ch('v')));
                tags.push(("g agents", Hit::ch('g')));
            } else {
                tags.push(("↑/↓ move", Hit::key(KeyCode::Down)));
                tags.push(("v board", Hit::ch('v')));
                tags.push(("g agents", Hit::ch('g')));
            }
            self.hits.tags(line, &hint, &tags);
        }
    }

    fn render_columns_view(
        &mut self,
        snapshot: &StatusSnapshot,
        summary: &FactorySummary,
        area: Rect,
        buf: &mut Buffer,
        new_key: &str,
    ) {
        let max_w = area.width as usize;
        let header_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        let filtered = self.filtered_ideas(snapshot);
        let header = format!(
            "FACTORY   ideas {}   needs you {}   filter: {} ▾   {new_key} new idea   Enter open",
            summary.ideas.len(),
            summary.needs_you,
            self.filter.label(),
        );
        write_clipped(buf, area.x, area.y, max_w, &header, header_style);
        let line = Rect::new(area.x, area.y, area.width, 1);
        let new_label = format!("{new_key} new idea");
        self.hits.tags(
            line,
            &header,
            &[
                (&format!("filter: {} ▾", self.filter.label()), Hit::ch('f')),
                (new_label.as_str(), Hit::ch('n')),
                ("Enter open", Hit::key(KeyCode::Enter)),
            ],
        );
        if area.height < 3 {
            return;
        }

        let states = column_states(summary);
        let mut views: Vec<ColumnView<'_>> = states
            .iter()
            .map(|state| ColumnView {
                state: *state,
                ideas: Vec::new(),
            })
            .collect();
        for (index, idea) in filtered.iter().enumerate() {
            if let Some(view) = views.iter_mut().find(|v| v.state == idea.state) {
                view.ideas.push((index, idea));
            }
        }
        for view in &mut views {
            view.ideas
                .sort_by(|a, b| b.1.updated.total_cmp(&a.1.updated));
        }
        let focus_idx = states.iter().position(|s| *s == self.column).unwrap_or(0);
        let visible = visible_column_count(area.width, states.len());
        self.first_column = first_visible(self.first_column, focus_idx, visible, states.len());
        let body_bottom = if area.height >= 4 {
            area.y + area.height - 1
        } else {
            area.y + area.height
        };
        let body = Rect::new(
            area.x,
            area.y + 1,
            area.width,
            body_bottom.saturating_sub(area.y + 1),
        );
        let selected_id = self.selected_id.clone();
        render_columns(
            body,
            buf,
            &ColumnsRender {
                columns: &views,
                first: self.first_column,
                focus: self.column,
                selected_id: selected_id.as_deref(),
            },
            &mut self.hits,
        );
        if area.height >= 4 {
            self.render_hint_line(area, buf, new_key);
        }
    }
}

fn write_clipped(buf: &mut Buffer, x: u16, y: u16, max_w: usize, text: &str, style: Style) {
    let clipped = truncate_to_width(text, max_w);
    buf.set_stringn(x, y, clipped, max_w, style);
}
