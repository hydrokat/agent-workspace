use std::cell::Cell;
use std::path::PathBuf;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::factory::broker::gate::{
    AuthType, ProviderStatusItem, consent_provider, provider_statuses, revoke_provider,
    today_iso_date,
};
use crate::factory::broker::terms::{TermsVerdict, lookup as lookup_terms};
use crate::factory::login::{
    HostLoginProbe, LoginProbe, LoginStatusItem, all_statuses, default_login_items, open_login_pane,
};
use crate::factory::model::{FactorySettings, ProviderGateState};
use crate::factory::settings as factory_settings;
use crate::factory::store::FactoryStore;
use crate::factory::workflow::{parse as wf_parse, validate as wf_validate};
use crate::sanitize::clean;
use crate::tmux::client::{TmuxClient, socket_for};
use crate::tmux::grid::{FillOrder, GridCols, GridPrefs, GridRows, SidebarMode};
use crate::ui::hits::{Hit, HitMap};

const STANDARD_SECTION_HEIGHT: usize = 10;

pub const COLS_OPTIONS: [GridCols; 5] = [
    GridCols::Auto,
    GridCols::Fixed(1),
    GridCols::Fixed(2),
    GridCols::Fixed(3),
    GridCols::Fixed(4),
];

pub const ROWS_OPTIONS: [GridRows; 4] = [
    GridRows::Auto,
    GridRows::Fixed(1),
    GridRows::Fixed(2),
    GridRows::Fixed(3),
];

pub const WORKSPACE_NAME_ROW: usize = 4;
pub const RESET_ROW: usize = 5;

pub const FILL_OPTIONS: [FillOrder; 2] = [FillOrder::RowFirst, FillOrder::ColumnFirst];

pub const SIDEBAR_OPTIONS: [SidebarMode; 4] = [
    SidebarMode::Auto,
    SidebarMode::Wide,
    SidebarMode::Compact,
    SidebarMode::Hidden,
];

pub const QUIT_ROW: usize = 6;

const AUTH_TYPES: [AuthType; 3] = [AuthType::Subscription, AuthType::ApiKey, AuthType::Cloud];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsAction {
    Changed,
    ResetUi,
    QuitMenu,
    RenameWorkspace(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactoryRow {
    MaxParallelAgents,
    DefaultTemperature,
    MaxReviewRounds,
    DefaultWorkflow,
    RunBudget,
    ApprovalGraceSecs,
    Provider(usize),
    ModelTier(usize),
    CheckCommands,
    DeployCommand,
    AgentLogin(usize),
    DefaultWorkerCli,
    OrchestratorMayChoose,
}

#[derive(Debug, Clone)]
pub struct FactoryLoginPaneState {
    pub cli: String,
    pub pane_id: String,
    pub confirm_cancel: bool,
}

#[derive(Debug, Clone)]
pub struct FactoryEditState {
    pub key: String,
    pub label: String,
    pub buffer: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FactoryConsentState {
    pub provider: String,
    pub auth_idx: usize,
    pub input: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FactoryRevokeState {
    pub provider: String,
    pub confirm_yes: bool,
}

#[derive(Debug, Clone)]
struct StyledLine {
    text: String,
    style: Style,
    row_index: Option<usize>,
}

pub struct SettingsTab {
    pub prefs: GridPrefs,
    pub sidebar_mode: SidebarMode,
    pub focused_row: usize,
    pub confirm_reset: bool,
    pub modal_focus: usize,
    pub flash_message: Option<(String, Instant)>,
    pub workspace_name: String,
    pub editing_workspace_name: Option<String>,
    pub factory_enabled: bool,
    pub workspace: Option<PathBuf>,
    pub factory_settings: FactorySettings,
    pub factory_providers: Vec<ProviderStatusItem>,
    pub factory_logins: Vec<LoginStatusItem>,
    pub factory_workflows: Vec<String>,
    pub budget_sub_field: usize,
    pub tier_sub_field: usize,
    pub factory_edit: Option<FactoryEditState>,
    pub factory_consent: Option<FactoryConsentState>,
    pub factory_revoke: Option<FactoryRevokeState>,
    pub factory_login_pane: Option<FactoryLoginPaneState>,
    pub logins_probed: bool,
    pub scroll_offset: usize,
    pub render_scroll: Cell<usize>,
}

impl Default for SettingsTab {
    fn default() -> Self {
        let defaults = FactorySettings::default();
        let providers = provider_statuses(&defaults, &today_iso_date());
        Self {
            prefs: GridPrefs::default(),
            sidebar_mode: SidebarMode::default(),
            focused_row: 0,
            confirm_reset: false,
            modal_focus: 1,
            flash_message: None,
            workspace_name: String::new(),
            editing_workspace_name: None,
            factory_enabled: false,
            workspace: None,
            factory_settings: defaults,
            factory_providers: providers,
            factory_logins: default_login_items(),
            factory_workflows: Vec::new(),
            budget_sub_field: 0,
            tier_sub_field: 0,
            factory_edit: None,
            factory_consent: None,
            factory_revoke: None,
            factory_login_pane: None,
            logins_probed: false,
            scroll_offset: 0,
            render_scroll: Cell::new(0),
        }
    }
}

impl SettingsTab {
    pub fn new(prefs: GridPrefs, sidebar_mode: SidebarMode) -> Self {
        Self {
            prefs,
            sidebar_mode,
            ..Self::default()
        }
    }

    pub fn is_editing_or_modal(&self) -> bool {
        self.confirm_reset
            || self.editing_workspace_name.is_some()
            || self.factory_edit.is_some()
            || self.factory_consent.is_some()
            || self.factory_revoke.is_some()
            || self.factory_login_pane.is_some()
    }

    pub fn refresh_logins_with(&mut self, probe: &dyn LoginProbe) {
        self.factory_logins = all_statuses(probe);
        self.logins_probed = true;
    }

    pub fn refresh_logins(&mut self) {
        self.refresh_logins_with(&HostLoginProbe);
    }

    pub fn ensure_logins_probed_with(&mut self, probe: &dyn LoginProbe) {
        if !self.logins_probed {
            self.refresh_logins_with(probe);
        }
    }

    pub fn ensure_logins_probed(&mut self) {
        self.ensure_logins_probed_with(&HostLoginProbe);
    }

    pub fn configure_factory(&mut self, enabled: bool, workspace: Option<PathBuf>) {
        self.factory_enabled = enabled;
        self.workspace = workspace;
        if enabled {
            self.reload_factory();
        }
    }

    pub fn reload_factory(&mut self) {
        let Some(ref ws) = self.workspace else {
            return;
        };
        let store = FactoryStore::new(ws);
        let settings = factory_settings::load(&store).unwrap_or_default();
        let providers = provider_statuses(&settings, &today_iso_date());

        let mut workflows = Vec::new();
        let wf_dir = ws.join("workflows");
        if wf_dir.is_dir()
            && let Ok(entries) = std::fs::read_dir(&wf_dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                    continue;
                }
                if let Ok(raw) = wf_parse::load_workflow_file(&path)
                    && let Ok(wf) = wf_validate::validate(&raw, &settings)
                {
                    workflows.push(wf.name);
                }
            }
        }
        workflows.sort();
        workflows.dedup();

        self.factory_settings = settings;
        self.factory_providers = providers;
        self.factory_workflows = workflows;
    }

    fn factory_rows(&self) -> Vec<FactoryRow> {
        if !self.factory_enabled {
            return Vec::new();
        }
        let mut rows = vec![
            FactoryRow::MaxParallelAgents,
            FactoryRow::DefaultTemperature,
            FactoryRow::MaxReviewRounds,
            FactoryRow::DefaultWorkflow,
            FactoryRow::RunBudget,
            FactoryRow::ApprovalGraceSecs,
        ];
        for i in 0..self.factory_providers.len() {
            rows.push(FactoryRow::Provider(i));
        }
        let tier_count = self.factory_settings.model_tiers.len();
        for i in 0..tier_count {
            rows.push(FactoryRow::ModelTier(i));
        }
        rows.push(FactoryRow::CheckCommands);
        rows.push(FactoryRow::DeployCommand);
        for i in 0..self.factory_logins.len() {
            rows.push(FactoryRow::AgentLogin(i));
        }
        rows.push(FactoryRow::DefaultWorkerCli);
        rows.push(FactoryRow::OrchestratorMayChoose);
        rows
    }

    fn max_row_index(&self) -> usize {
        if !self.factory_enabled {
            QUIT_ROW
        } else {
            QUIT_ROW + self.factory_rows().len()
        }
    }

    pub fn focus_factory_key(&mut self, key: &str) {
        let rows = self.factory_rows();
        let target = match key {
            "max_parallel_agents" => Some(FactoryRow::MaxParallelAgents),
            "default_temperature" => Some(FactoryRow::DefaultTemperature),
            "max_review_rounds" => Some(FactoryRow::MaxReviewRounds),
            "default_workflow" => Some(FactoryRow::DefaultWorkflow),
            "default_budget" => Some(FactoryRow::RunBudget),
            "approval_grace_secs" => Some(FactoryRow::ApprovalGraceSecs),
            _ => None,
        };
        if let Some(t) = target
            && let Some(idx) = rows.iter().position(|r| r == &t)
        {
            self.focused_row = QUIT_ROW + 1 + idx;
        }
    }

    pub fn focus_factory_provider(&mut self, provider: &str) {
        if let Some(p_idx) = self
            .factory_providers
            .iter()
            .position(|p| p.provider == provider)
            && let Some(row_idx) = self
                .factory_rows()
                .iter()
                .position(|r| matches!(r, FactoryRow::Provider(i) if *i == p_idx))
        {
            self.focused_row = QUIT_ROW + 1 + row_idx;
        }
    }

    pub fn flash(&mut self, msg: &str) {
        self.flash_message = Some((msg.to_string(), Instant::now()));
    }

    fn owner_name() -> String {
        std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .unwrap_or_else(|_| "you".to_string())
    }

    fn save_factory_setting(&mut self, key: &str, value: &str) -> Result<(), String> {
        let Some(ref ws) = self.workspace else {
            return Err("Workspace not configured".to_string());
        };
        let store = FactoryStore::new(ws);
        let owner = Self::owner_name();
        let env_fn = |k: &str| std::env::var(k).ok();
        let no_pane = |_: &str| None;
        let caller = crate::factory::approval::Caller::tui(&env_fn, &no_pane);
        match factory_settings::set(&store, key, value, &owner, &caller) {
            Ok(res) => {
                self.factory_settings = res.settings;
                self.factory_providers =
                    provider_statuses(&self.factory_settings, &today_iso_date());
                self.flash(&format!("{} = {}", res.key, res.new_value));
                Ok(())
            }
            Err(err) => Err(clean(&err.to_string())),
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> Option<SettingsAction> {
        if self.factory_login_pane.is_some() {
            return self.handle_login_pane_key(key);
        }
        if self.factory_consent.is_some() {
            return self.handle_consent_key(key);
        }
        if self.factory_revoke.is_some() {
            return self.handle_revoke_key(key);
        }
        if self.factory_edit.is_some() {
            return self.handle_factory_edit_key(key);
        }

        if let Some(ref mut buf) = self.editing_workspace_name {
            match key.code {
                KeyCode::Esc => {
                    self.editing_workspace_name = None;
                    None
                }
                KeyCode::Backspace => {
                    buf.pop();
                    None
                }
                KeyCode::Char(c)
                    if crate::adapters::base::is_label_char(c)
                        && buf.chars().count() < crate::adapters::base::LABEL_MAX_CHARS =>
                {
                    let starts_ok = !buf.is_empty() || c.is_ascii_alphanumeric();
                    let space_ok = c != ' ' || !buf.ends_with(' ');
                    if starts_ok && space_ok {
                        buf.push(c);
                    }
                    None
                }
                KeyCode::Enter => {
                    let normalized = crate::adapters::base::normalize_label(buf);
                    if normalized.is_empty() || crate::adapters::base::is_valid_label(&normalized) {
                        self.editing_workspace_name = None;
                        self.workspace_name = normalized.clone();
                        self.flash("Workspace renamed");
                        Some(SettingsAction::RenameWorkspace(normalized))
                    } else {
                        self.flash(crate::adapters::base::LABEL_RULE_TEXT);
                        None
                    }
                }
                _ => None,
            }
        } else if self.confirm_reset {
            match key.code {
                KeyCode::Esc => {
                    self.confirm_reset = false;
                    None
                }
                KeyCode::Left
                | KeyCode::Right
                | KeyCode::Tab
                | KeyCode::BackTab
                | KeyCode::Char('h')
                | KeyCode::Char('l') => {
                    self.modal_focus = if self.modal_focus == 0 { 1 } else { 0 };
                    None
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    self.confirm_reset = false;
                    if self.modal_focus == 0 {
                        self.flash("UI reset to defaults");
                        Some(SettingsAction::ResetUi)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            let max_row = self.max_row_index();
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.focused_row = if self.focused_row == 0 {
                        max_row
                    } else {
                        self.focused_row - 1
                    };
                    None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.focused_row = if self.focused_row >= max_row {
                        0
                    } else {
                        self.focused_row + 1
                    };
                    None
                }
                KeyCode::Left | KeyCode::Char('h') => self.handle_horizontal(false),
                KeyCode::Right | KeyCode::Char('l') => self.handle_horizontal(true),
                KeyCode::Char('r')
                    if matches!(self.focused_factory_row(), Some(FactoryRow::AgentLogin(_))) =>
                {
                    self.refresh_logins();
                    self.flash("Refreshed CLI sign-in status");
                    None
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    if self.focused_row == QUIT_ROW {
                        Some(SettingsAction::QuitMenu)
                    } else if self.focused_row == RESET_ROW {
                        self.confirm_reset = true;
                        self.modal_focus = 1;
                        None
                    } else if self.focused_row == WORKSPACE_NAME_ROW {
                        self.editing_workspace_name = Some(self.workspace_name.clone());
                        None
                    } else if self.focused_row > QUIT_ROW {
                        self.activate_factory_row();
                        None
                    } else {
                        self.cycle_option(true)
                    }
                }
                _ => None,
            }
        }
    }

    fn handle_horizontal(&mut self, next: bool) -> Option<SettingsAction> {
        if self.focused_row <= 3 {
            return self.cycle_option(next);
        }
        if let Some(row) = self.focused_factory_row() {
            match row {
                FactoryRow::DefaultWorkflow => {
                    self.cycle_workflow(next);
                }
                FactoryRow::RunBudget => {
                    self.budget_sub_field = if next {
                        (self.budget_sub_field + 1) % 3
                    } else {
                        (self.budget_sub_field + 2) % 3
                    };
                }
                FactoryRow::ModelTier(_) => {
                    self.tier_sub_field = if next {
                        (self.tier_sub_field + 1) % 3
                    } else {
                        (self.tier_sub_field + 2) % 3
                    };
                }
                FactoryRow::DefaultWorkerCli => {
                    self.cycle_worker_cli(next);
                }
                FactoryRow::OrchestratorMayChoose => {
                    self.toggle_orchestrator_may_choose();
                }
                _ => {}
            }
        }
        None
    }

    fn focused_factory_row(&self) -> Option<FactoryRow> {
        if !self.factory_enabled || self.focused_row <= QUIT_ROW {
            return None;
        }
        let idx = self.focused_row - (QUIT_ROW + 1);
        self.factory_rows().into_iter().nth(idx)
    }

    fn cycle_workflow(&mut self, next: bool) {
        if self.factory_workflows.is_empty() {
            self.flash("No valid workflows found");
            return;
        }
        let cur = self
            .factory_workflows
            .iter()
            .position(|w| w == &self.factory_settings.default_workflow)
            .unwrap_or(0);
        let new_idx = if next {
            (cur + 1) % self.factory_workflows.len()
        } else {
            (cur + self.factory_workflows.len() - 1) % self.factory_workflows.len()
        };
        let chosen = self.factory_workflows[new_idx].clone();
        if let Err(err) = self.save_factory_setting("default_workflow", &chosen) {
            self.flash(&err);
        }
    }

    fn cycle_worker_cli(&mut self, next: bool) {
        const OPTIONS: [&str; 4] = ["auto", "claude", "agy", "opencode"];
        let cur = OPTIONS
            .iter()
            .position(|&c| c == self.factory_settings.default_worker_cli)
            .unwrap_or(0);
        let new_idx = if next {
            (cur + 1) % OPTIONS.len()
        } else {
            (cur + OPTIONS.len() - 1) % OPTIONS.len()
        };
        if let Err(err) = self.save_factory_setting("default_worker_cli", OPTIONS[new_idx]) {
            self.flash(&err);
        }
    }

    fn toggle_orchestrator_may_choose(&mut self) {
        let next = if self.factory_settings.orchestrator_may_choose {
            "false"
        } else {
            "true"
        };
        if let Err(err) = self.save_factory_setting("orchestrator_may_choose", next) {
            self.flash(&err);
        }
    }

    fn activate_agent_login(&mut self, idx: usize) {
        let Some(mut item) = self.factory_logins.get(idx).cloned() else {
            return;
        };
        if !item.installed {
            self.refresh_logins();
            let Some(updated) = self.factory_logins.get(idx).cloned() else {
                return;
            };
            if !updated.installed {
                self.flash(&format!("{} is not installed — see README", updated.cli));
                return;
            }
            item = updated;
        }
        if !item.has_login_command {
            let hint = item
                .login_hint
                .unwrap_or_else(|| format!("Sign in from {}'s own TUI", item.cli));
            self.flash(&hint);
            return;
        }
        let Some(ref ws) = self.workspace else {
            self.flash("Workspace not configured");
            return;
        };
        let client = TmuxClient::new(socket_for(ws), None);
        match open_login_pane(&client, ws, &item.cli, &HostLoginProbe) {
            Ok(res) => {
                self.factory_login_pane = Some(FactoryLoginPaneState {
                    cli: res.cli,
                    pane_id: res.pane_id,
                    confirm_cancel: false,
                });
            }
            Err(err) => {
                self.flash(&clean(&err.to_string()));
            }
        }
    }

    fn handle_login_pane_key(&mut self, key: &KeyEvent) -> Option<SettingsAction> {
        let state = self.factory_login_pane.as_mut()?;
        match key.code {
            KeyCode::Esc => {
                if state.confirm_cancel {
                    let pane = state.pane_id.clone();
                    self.close_login_pane(&pane, true);
                } else {
                    state.confirm_cancel = true;
                }
                None
            }
            KeyCode::Char('y') | KeyCode::Char('Y') if state.confirm_cancel => {
                let pane = state.pane_id.clone();
                self.close_login_pane(&pane, true);
                None
            }
            KeyCode::Char('n') | KeyCode::Char('N') if state.confirm_cancel => {
                state.confirm_cancel = false;
                None
            }
            KeyCode::Enter => {
                if state.confirm_cancel {
                    let pane = state.pane_id.clone();
                    self.close_login_pane(&pane, true);
                } else {
                    let pane = state.pane_id.clone();
                    self.close_login_pane(&pane, false);
                    self.refresh_logins();
                    self.flash("Sign-in status refreshed");
                }
                None
            }
            _ => None,
        }
    }

    fn close_login_pane(&mut self, pane_id: &str, cancelled: bool) {
        if let Some(ref ws) = self.workspace {
            let client = TmuxClient::new(socket_for(ws), None);
            let _ = client.run(&["kill-pane", "-t", pane_id]);
        }
        self.factory_login_pane = None;
        if cancelled {
            self.flash("Login cancelled");
        }
    }

    fn activate_factory_row(&mut self) {
        let Some(row) = self.focused_factory_row() else {
            return;
        };
        match row {
            FactoryRow::MaxParallelAgents => {
                self.factory_edit = Some(FactoryEditState {
                    key: "max_parallel_agents".to_string(),
                    label: "Max parallel agents (1-8)".to_string(),
                    buffer: self.factory_settings.max_parallel_agents.to_string(),
                    error: None,
                });
            }
            FactoryRow::DefaultTemperature => {
                self.factory_edit = Some(FactoryEditState {
                    key: "default_temperature".to_string(),
                    label: "Default temperature (0.0-1.0)".to_string(),
                    buffer: format!("{:.1}", self.factory_settings.default_temperature),
                    error: None,
                });
            }
            FactoryRow::MaxReviewRounds => {
                self.factory_edit = Some(FactoryEditState {
                    key: "max_review_rounds".to_string(),
                    label: "Max review rounds (0-5)".to_string(),
                    buffer: self.factory_settings.max_review_rounds.to_string(),
                    error: None,
                });
            }
            FactoryRow::DefaultWorkflow => {
                self.cycle_workflow(true);
            }
            FactoryRow::RunBudget => {
                let (key, label, val) = match self.budget_sub_field {
                    0 => (
                        "default_budget.max_cost_usd",
                        "Budget max cost USD (or null)",
                        self.factory_settings
                            .default_budget
                            .max_cost_usd
                            .map(|v| format!("{v:.2}"))
                            .unwrap_or_else(|| "null".to_string()),
                    ),
                    1 => (
                        "default_budget.max_duration_secs",
                        "Budget max duration secs (or null)",
                        self.factory_settings
                            .default_budget
                            .max_duration_secs
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "null".to_string()),
                    ),
                    _ => (
                        "default_budget.max_retries",
                        "Budget max retries (or null)",
                        self.factory_settings
                            .default_budget
                            .max_retries
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "null".to_string()),
                    ),
                };
                self.factory_edit = Some(FactoryEditState {
                    key: key.to_string(),
                    label: label.to_string(),
                    buffer: val,
                    error: None,
                });
            }
            FactoryRow::ApprovalGraceSecs => {
                self.factory_edit = Some(FactoryEditState {
                    key: "approval_grace_secs".to_string(),
                    label: "Approval grace period (0-600 s)".to_string(),
                    buffer: self.factory_settings.approval_grace_secs.to_string(),
                    error: None,
                });
            }
            FactoryRow::Provider(idx) => {
                if let Some(p) = self.factory_providers.get(idx).cloned() {
                    if p.gate == ProviderGateState::Open {
                        self.factory_revoke = Some(FactoryRevokeState {
                            provider: p.provider,
                            confirm_yes: false,
                        });
                    } else {
                        self.factory_consent = Some(FactoryConsentState {
                            provider: p.provider,
                            auth_idx: 0,
                            input: String::new(),
                            error: None,
                        });
                    }
                }
            }
            FactoryRow::ModelTier(idx) => {
                if let Some((provider, tiers)) = self.factory_settings.model_tiers.iter().nth(idx) {
                    let (tier_name, cur_val) = match self.tier_sub_field {
                        0 => ("economy", tiers.economy.clone()),
                        1 => ("standard", tiers.standard.clone()),
                        _ => ("premium", tiers.premium.clone()),
                    };
                    self.factory_edit = Some(FactoryEditState {
                        key: format!("model_tiers.{provider}.{tier_name}"),
                        label: format!("Model tier {provider}.{tier_name}"),
                        buffer: cur_val,
                        error: None,
                    });
                }
            }
            FactoryRow::CheckCommands => {
                self.flash("Check commands are confirmed in the Factory Run view or via CLI");
            }
            FactoryRow::DeployCommand => {
                self.flash("Deploy command is confirmed via `tandem factory deploy confirm`");
            }
            FactoryRow::AgentLogin(idx) => {
                self.activate_agent_login(idx);
            }
            FactoryRow::DefaultWorkerCli => {
                self.cycle_worker_cli(true);
            }
            FactoryRow::OrchestratorMayChoose => {
                self.toggle_orchestrator_may_choose();
            }
        }
    }

    fn handle_factory_edit_key(&mut self, key: &KeyEvent) -> Option<SettingsAction> {
        let edit = self.factory_edit.as_mut()?;
        match key.code {
            KeyCode::Esc => {
                self.factory_edit = None;
                None
            }
            KeyCode::Backspace => {
                edit.buffer.pop();
                edit.error = None;
                None
            }
            KeyCode::Char(c) if !c.is_control() => {
                edit.buffer.push(c);
                edit.error = None;
                None
            }
            KeyCode::Enter => {
                let k = edit.key.clone();
                let v = edit.buffer.trim().to_string();
                match self.save_factory_setting(&k, &v) {
                    Ok(()) => {
                        self.factory_edit = None;
                    }
                    Err(err) => {
                        if let Some(ref mut e) = self.factory_edit {
                            e.error = Some(err);
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn handle_consent_key(&mut self, key: &KeyEvent) -> Option<SettingsAction> {
        let state = self.factory_consent.as_mut()?;
        match key.code {
            KeyCode::Esc => {
                self.factory_consent = None;
                None
            }
            KeyCode::Left => {
                state.auth_idx = if state.auth_idx == 0 {
                    AUTH_TYPES.len() - 1
                } else {
                    state.auth_idx - 1
                };
                state.error = None;
                None
            }
            KeyCode::Right => {
                state.auth_idx = (state.auth_idx + 1) % AUTH_TYPES.len();
                state.error = None;
                None
            }
            KeyCode::Backspace => {
                state.input.pop();
                state.error = None;
                None
            }
            KeyCode::Char(c) if !c.is_control() => {
                state.input.push(c);
                state.error = None;
                None
            }
            KeyCode::Enter => {
                let provider = state.provider.clone();
                let auth = AUTH_TYPES[state.auth_idx];
                let confirm = state.input.trim().to_string();
                if confirm != provider {
                    state.error = Some(format!("Type {provider} to confirm consent."));
                    return None;
                }
                let Some(ref ws) = self.workspace else {
                    state.error = Some("Workspace not configured".to_string());
                    return None;
                };
                let store = FactoryStore::new(ws);
                let owner = Self::owner_name();
                let env_fn = |k: &str| std::env::var(k).ok();
                let no_pane = |_: &str| None;
                let caller = crate::factory::approval::Caller::tui(&env_fn, &no_pane);
                match consent_provider(&store, &provider, auth, &confirm, &owner, &caller) {
                    Ok(_) => {
                        self.factory_consent = None;
                        self.reload_factory();
                        self.flash(&format!("Consented to {provider} ({})", auth.as_str()));
                    }
                    Err(err) => {
                        if let Some(ref mut st) = self.factory_consent {
                            st.error = Some(clean(&err.to_string()));
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn handle_revoke_key(&mut self, key: &KeyEvent) -> Option<SettingsAction> {
        let state = self.factory_revoke.as_mut()?;
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                self.factory_revoke = None;
                None
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                let provider = state.provider.clone();
                self.perform_revoke(&provider);
                None
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                state.confirm_yes = !state.confirm_yes;
                None
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                let yes = state.confirm_yes;
                let provider = state.provider.clone();
                if yes {
                    self.perform_revoke(&provider);
                } else {
                    self.factory_revoke = None;
                }
                None
            }
            _ => None,
        }
    }

    fn perform_revoke(&mut self, provider: &str) {
        let Some(ref ws) = self.workspace else {
            self.factory_revoke = None;
            return;
        };
        let store = FactoryStore::new(ws);
        let owner = Self::owner_name();
        let env_fn = |k: &str| std::env::var(k).ok();
        let no_pane = |_: &str| None;
        let caller = crate::factory::approval::Caller::tui(&env_fn, &no_pane);
        match revoke_provider(&store, provider, &owner, &caller) {
            Ok(_) => {
                self.factory_revoke = None;
                self.reload_factory();
                self.flash(&format!("Revoked consent for {provider}"));
            }
            Err(err) => {
                self.factory_revoke = None;
                self.flash(&clean(&err.to_string()));
            }
        }
    }

    fn cycle_option(&mut self, next: bool) -> Option<SettingsAction> {
        match self.focused_row {
            0 => {
                let idx = COLS_OPTIONS
                    .iter()
                    .position(|&c| c == self.prefs.cols)
                    .unwrap_or(0);
                let new_idx = if next {
                    (idx + 1) % COLS_OPTIONS.len()
                } else {
                    (idx + COLS_OPTIONS.len() - 1) % COLS_OPTIONS.len()
                };
                self.prefs.cols = COLS_OPTIONS[new_idx];
                self.flash("Settings saved");
                Some(SettingsAction::Changed)
            }
            1 => {
                let idx = ROWS_OPTIONS
                    .iter()
                    .position(|&r| r == self.prefs.rows)
                    .unwrap_or(0);
                let new_idx = if next {
                    (idx + 1) % ROWS_OPTIONS.len()
                } else {
                    (idx + ROWS_OPTIONS.len() - 1) % ROWS_OPTIONS.len()
                };
                self.prefs.rows = ROWS_OPTIONS[new_idx];
                self.flash("Settings saved");
                Some(SettingsAction::Changed)
            }
            2 => {
                let idx = FILL_OPTIONS
                    .iter()
                    .position(|&f| f == self.prefs.fill)
                    .unwrap_or(0);
                let new_idx = if next {
                    (idx + 1) % FILL_OPTIONS.len()
                } else {
                    (idx + FILL_OPTIONS.len() - 1) % FILL_OPTIONS.len()
                };
                self.prefs.fill = FILL_OPTIONS[new_idx];
                self.flash("Settings saved");
                Some(SettingsAction::Changed)
            }
            3 => {
                let idx = SIDEBAR_OPTIONS
                    .iter()
                    .position(|&s| s == self.sidebar_mode)
                    .unwrap_or(0);
                let new_idx = if next {
                    (idx + 1) % SIDEBAR_OPTIONS.len()
                } else {
                    (idx + SIDEBAR_OPTIONS.len() - 1) % SIDEBAR_OPTIONS.len()
                };
                self.sidebar_mode = SIDEBAR_OPTIONS[new_idx];
                self.flash("Settings saved");
                Some(SettingsAction::Changed)
            }
            _ => None,
        }
    }

    fn handle_wheel(&mut self, up: bool) -> Option<SettingsAction> {
        if self.is_editing_or_modal() {
            return None;
        }
        let max_row = self.max_row_index();
        self.focused_row = if up {
            self.focused_row.saturating_sub(1)
        } else {
            (self.focused_row + 1).min(max_row)
        };
        None
    }

    fn handle_factory_modal_click(
        &mut self,
        mouse: &MouseEvent,
        area: Rect,
    ) -> Option<SettingsAction> {
        let mut hits = HitMap::default();
        if self.factory_edit.is_some() {
            let r = modal_rect(area, EDIT_MODAL_SIZE);
            if r.width >= 10 && r.height >= 5 {
                hits.tags(
                    Rect::new(r.x + 2, r.y + 3, r.width.saturating_sub(3), 1),
                    EDIT_MODAL_HINT,
                    &[
                        ("Enter save", Hit::key(KeyCode::Enter)),
                        ("Esc cancel", Hit::key(KeyCode::Esc)),
                    ],
                );
            }
        } else if let Some(ref state) = self.factory_consent {
            let r = modal_rect(area, CONSENT_MODAL_SIZE);
            if r.width >= 12 && r.height >= 6 {
                let auth = AUTH_TYPES[state.auth_idx].as_str();
                let auth_line = format!("Auth: < {auth} >");
                let line_w = r.width.saturating_sub(3);
                hits.tags(
                    Rect::new(r.x + 2, r.y + 1, line_w, 1),
                    &auth_line,
                    &[
                        ("<", Hit::key(KeyCode::Left)),
                        (auth, Hit::key(KeyCode::Right)),
                        (">", Hit::key(KeyCode::Right)),
                    ],
                );
                hits.tags(
                    Rect::new(r.x + 2, r.y + 4, line_w, 1),
                    CONSENT_MODAL_HINT,
                    &[
                        ("Left/Right auth", Hit::key(KeyCode::Right)),
                        ("Enter consent", Hit::key(KeyCode::Enter)),
                        ("Esc cancel", Hit::key(KeyCode::Esc)),
                    ],
                );
            }
        } else if self.factory_revoke.is_some() {
            let r = modal_rect(area, REVOKE_MODAL_SIZE);
            if r.width >= 12 && r.height >= 5 {
                hits.push(Rect::new(r.x + 12, r.y + 3, 7, 1), Hit::ch('y'));
                hits.push(Rect::new(r.x + 24, r.y + 3, 6, 1), Hit::ch('n'));
            }
        } else if self.factory_login_pane.is_some() {
            let r = modal_rect(area, LOGIN_MODAL_SIZE);
            if r.width >= 16 && r.height >= 5 {
                hits.tags(
                    Rect::new(r.x + 2, r.y + 3, r.width.saturating_sub(3), 1),
                    LOGIN_MODAL_HINT,
                    &[
                        ("[Done]", Hit::key(KeyCode::Enter)),
                        ("[Cancel]", Hit::key(KeyCode::Esc)),
                    ],
                );
            }
        }
        match hits.at(mouse.column, mouse.row)? {
            Hit::Key(code, mods) => self.handle_key(&KeyEvent::new(code, mods)),
            Hit::Row(_) | Hit::Cmd(_) => None,
        }
    }

    pub fn handle_mouse(&mut self, mouse: &MouseEvent, area: Rect) -> Option<SettingsAction> {
        match mouse.kind {
            MouseEventKind::ScrollUp => return self.handle_wheel(true),
            MouseEventKind::ScrollDown => return self.handle_wheel(false),
            MouseEventKind::Down(MouseButton::Left) => {}
            _ => return None,
        }

        if self.factory_edit.is_some()
            || self.factory_consent.is_some()
            || self.factory_revoke.is_some()
            || self.factory_login_pane.is_some()
        {
            return self.handle_factory_modal_click(mouse, area);
        }

        if self.confirm_reset {
            let modal_w = 48u16.min(area.width);
            let modal_h = 5u16.min(area.height);
            let modal_x = area.x + (area.width.saturating_sub(modal_w)) / 2;
            let modal_y = area.y + (area.height.saturating_sub(modal_h)) / 2;

            if mouse.row == modal_y + 3 {
                if mouse.column >= modal_x + 14 && mouse.column <= modal_x + 24 {
                    self.confirm_reset = false;
                    self.flash("UI reset to defaults");
                    return Some(SettingsAction::ResetUi);
                } else if mouse.column >= modal_x + 26 && mouse.column <= modal_x + 38 {
                    self.confirm_reset = false;
                    return None;
                }
            }
            self.confirm_reset = false;
            return None;
        }

        let rel_y = mouse.row.saturating_sub(area.y) as usize
            + self.scroll_offset
            + self.render_scroll.get();
        match rel_y {
            0 => {
                if mouse.column >= area.x + 14 && mouse.column <= area.x + 22 {
                    self.focused_row = QUIT_ROW;
                    Some(SettingsAction::QuitMenu)
                } else {
                    None
                }
            }
            1 => {
                self.focused_row = 0;
                self.cycle_option(true)
            }
            2 => {
                self.focused_row = 1;
                self.cycle_option(true)
            }
            3 => {
                self.focused_row = 2;
                self.cycle_option(true)
            }
            4 => {
                self.focused_row = 3;
                self.cycle_option(true)
            }
            5 => {
                self.focused_row = WORKSPACE_NAME_ROW;
                self.editing_workspace_name = Some(self.workspace_name.clone());
                None
            }
            6 => {
                self.focused_row = RESET_ROW;
                self.confirm_reset = true;
                self.modal_focus = 1;
                None
            }
            y if self.factory_enabled && y >= STANDARD_SECTION_HEIGHT => {
                let lines = self.build_factory_lines(area.width);
                let line = lines.get(y - STANDARD_SECTION_HEIGHT)?;
                let row = line.row_index?;
                self.focused_row = row;
                if matches!(self.focused_factory_row(), Some(FactoryRow::AgentLogin(_)))
                    && let Some(pos) = line.text.find("[Recheck]")
                {
                    let start = area.x + line.text[..pos].chars().count() as u16;
                    let end = start + "[Recheck]".len() as u16;
                    if mouse.column >= start && mouse.column < end {
                        self.refresh_logins();
                        self.flash("Refreshed CLI sign-in status");
                        return None;
                    }
                }
                self.handle_key(&KeyEvent::new(
                    KeyCode::Enter,
                    crossterm::event::KeyModifiers::NONE,
                ))
            }
            _ => None,
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        if !self.factory_enabled {
            self.render_scroll.set(0);
            self.render_standard_section(area, buf, 0);
            self.render_overlays(area, buf);
            return;
        }

        let factory_lines = self.build_factory_lines(area.width);
        let standard_height = STANDARD_SECTION_HEIGHT;
        let total_lines = standard_height + factory_lines.len();
        let focused_line = if self.focused_row <= QUIT_ROW {
            match self.focused_row {
                QUIT_ROW => 0,
                WORKSPACE_NAME_ROW => 5,
                RESET_ROW => 6,
                r => 1 + r,
            }
        } else {
            standard_height
                + factory_lines
                    .iter()
                    .position(|l| l.row_index == Some(self.focused_row))
                    .unwrap_or(0)
        };

        let view_h = area.height as usize;
        let scroll = if focused_line >= view_h {
            focused_line + 1 - view_h
        } else {
            0
        };

        self.render_scroll.set(scroll);
        if scroll == 0 {
            self.render_standard_section(area, buf, 0);
        }

        for (i, line) in factory_lines.iter().enumerate() {
            let abs_y = standard_height + i;
            if abs_y < scroll {
                continue;
            }
            let screen_y = area.y + (abs_y - scroll) as u16;
            if screen_y >= area.y + area.height {
                break;
            }
            buf.set_stringn(
                area.x,
                screen_y,
                &line.text,
                area.width as usize,
                line.style,
            );
        }

        let _ = total_lines;
        self.render_overlays(area, buf);
    }

    fn render_standard_section(&self, area: Rect, buf: &mut Buffer, _scroll: usize) {
        let header_style = Style::default().add_modifier(Modifier::BOLD);
        buf.set_string(area.x, area.y, "Grid layout", header_style);

        if area.width >= 23 {
            let is_focused = self.focused_row == QUIT_ROW && !self.is_editing_or_modal();
            let quit_style = if is_focused {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(Color::Yellow)
            };
            buf.set_string(area.x + 14, area.y, "[ Quit… ]", quit_style);

            if area.width >= 60 {
                let desc_style = Style::default().fg(Color::DarkGray);
                buf.set_string(
                    area.x + 25,
                    area.y,
                    "save, detach, update, discard",
                    desc_style,
                );
            }
        }

        let grid_labels = ["Columns", "Rows per page", "Fill order", "Sidebar"];
        for (idx, label) in grid_labels.iter().enumerate() {
            let row_y = area.y + 1 + idx as u16;
            if row_y >= area.y + area.height {
                break;
            }

            let is_focused = self.focused_row == idx && !self.is_editing_or_modal();
            let prefix = if is_focused { "▶ " } else { "  " };

            let label_str = format!("{prefix}{label:<14} ");
            let label_style = if is_focused {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            buf.set_string(area.x, row_y, &label_str, label_style);

            let mut cur_x = area.x + label_str.len() as u16;

            match idx {
                0 => {
                    for opt in &COLS_OPTIONS {
                        let is_active = *opt == self.prefs.cols;
                        let name = match opt {
                            GridCols::Auto => "Auto".to_string(),
                            GridCols::Fixed(n) => n.to_string(),
                        };
                        cur_x = render_option_badge(buf, cur_x, row_y, &name, is_active);
                    }
                }
                1 => {
                    for opt in &ROWS_OPTIONS {
                        let is_active = *opt == self.prefs.rows;
                        let name = match opt {
                            GridRows::Auto => "Auto".to_string(),
                            GridRows::Fixed(n) => n.to_string(),
                        };
                        cur_x = render_option_badge(buf, cur_x, row_y, &name, is_active);
                    }
                }
                2 => {
                    for opt in &FILL_OPTIONS {
                        let is_active = *opt == self.prefs.fill;
                        let name = match opt {
                            FillOrder::RowFirst => "Row first".to_string(),
                            FillOrder::ColumnFirst => "Column first".to_string(),
                        };
                        cur_x = render_option_badge(buf, cur_x, row_y, &name, is_active);
                    }
                }
                3 => {
                    for opt in &SIDEBAR_OPTIONS {
                        let is_active = *opt == self.sidebar_mode;
                        let name = match opt {
                            SidebarMode::Auto => "Auto".to_string(),
                            SidebarMode::Wide => "Wide".to_string(),
                            SidebarMode::Compact => "Compact".to_string(),
                            SidebarMode::Hidden => "Hidden".to_string(),
                        };
                        cur_x = render_option_badge(buf, cur_x, row_y, &name, is_active);
                    }
                }
                _ => {}
            }
        }

        let name_y = area.y + 5;
        if name_y < area.y + area.height {
            let is_focused = self.focused_row == WORKSPACE_NAME_ROW && !self.is_editing_or_modal();
            let prefix = if is_focused { "▶ " } else { "  " };
            let label_str = format!("{prefix}{:<14} ", "Workspace name");
            let label_style = if is_focused {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            buf.set_string(area.x, name_y, &label_str, label_style);

            let cur_x = area.x + label_str.len() as u16;
            if let Some(ref buf_text) = self.editing_workspace_name {
                let edit_str = format!("{buf_text}_");
                let edit_style = Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD);
                buf.set_string(cur_x, name_y, &edit_str, edit_style);
                let hint_x = cur_x + edit_str.len() as u16 + 2;
                if hint_x < area.x + area.width {
                    buf.set_string(
                        hint_x,
                        name_y,
                        "Letters, digits, spaces, - _ . · Enter saves · Esc cancels",
                        Style::default().fg(Color::DarkGray),
                    );
                }
            } else {
                let disp = if self.workspace_name.is_empty() {
                    "(default)"
                } else {
                    &self.workspace_name
                };
                let val_style = if is_focused {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::UNDERLINED)
                } else {
                    Style::default().fg(Color::Cyan)
                };
                buf.set_string(cur_x, name_y, disp, val_style);
                if is_focused {
                    let hint_x = cur_x + disp.len() as u16 + 2;
                    if hint_x < area.x + area.width {
                        buf.set_string(
                            hint_x,
                            name_y,
                            "Enter rename",
                            Style::default().fg(Color::DarkGray),
                        );
                    }
                }
            }
        }

        let reset_y = area.y + 6;
        if reset_y < area.y + area.height {
            let is_focused = self.focused_row == RESET_ROW && !self.is_editing_or_modal();
            let reset_prefix = if is_focused { "▶ " } else { "  " };
            let reset_btn = format!(
                "{reset_prefix}[ Reset UI ]   restores layout, sizes, grid and sidebar to defaults"
            );
            let style = if is_focused {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default().fg(Color::Yellow)
            };
            buf.set_string(area.x, reset_y, &reset_btn, style);
        }

        let about_y = area.y + 8;
        if about_y < area.y + area.height {
            let ver = env!("CARGO_PKG_VERSION");
            let about = format!("tandem {ver} · github.com/hydrokat/agent-workspace ↗");
            let style = Style::default().fg(Color::DarkGray);
            buf.set_string(area.x, about_y, &about, style);
        }
    }

    fn build_factory_lines(&self, width: u16) -> Vec<StyledLine> {
        let narrow = width < 80;
        let mut lines = Vec::new();
        lines.push(StyledLine {
            text: "FACTORY".to_string(),
            style: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            row_index: None,
        });

        let rows = self.factory_rows();
        let s = &self.factory_settings;
        for (idx, row) in rows.into_iter().enumerate() {
            let row_idx = QUIT_ROW + 1 + idx;
            let is_focused = self.focused_row == row_idx && !self.is_editing_or_modal();
            let prefix = if is_focused { "▶ " } else { "  " };
            let style = if is_focused {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            if matches!(row, FactoryRow::AgentLogin(0)) {
                lines.push(StyledLine {
                    text: "Agents & sign-in".to_string(),
                    style: Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                    row_index: None,
                });
            }

            let (label, value_and_range) = match row {
                FactoryRow::MaxParallelAgents => (
                    "Max parallel agents".to_string(),
                    format!("{}  (1-8)", s.max_parallel_agents),
                ),
                FactoryRow::DefaultTemperature => (
                    "Default temperature".to_string(),
                    format!(
                        "{:.1}  (0.0-1.0, where the provider allows)",
                        s.default_temperature
                    ),
                ),
                FactoryRow::MaxReviewRounds => (
                    "Max review rounds".to_string(),
                    format!("{}  (0-5)", s.max_review_rounds),
                ),
                FactoryRow::DefaultWorkflow => {
                    let wf = if self.factory_workflows.is_empty() {
                        format!("{} (no valid workflow)", s.default_workflow)
                    } else {
                        format!("{}  ▾", s.default_workflow)
                    };
                    ("Default workflow".to_string(), wf)
                }
                FactoryRow::RunBudget => {
                    let cost = s
                        .default_budget
                        .max_cost_usd
                        .map(|v| format!("${v:.2}"))
                        .unwrap_or_else(|| "none".to_string());
                    let dur = s
                        .default_budget
                        .max_duration_secs
                        .map(|v| format!("{} min", v / 60))
                        .unwrap_or_else(|| "none".to_string());
                    let ret = s
                        .default_budget
                        .max_retries
                        .map(|v| format!("{v} retries"))
                        .unwrap_or_else(|| "none".to_string());
                    let sub_hint = match self.budget_sub_field {
                        0 => "[cost] · dur · retries",
                        1 => "cost · [dur] · retries",
                        _ => "cost · dur · [retries]",
                    };
                    (
                        "Run budget".to_string(),
                        format!("{cost} · {dur} · {ret}  ({sub_hint})"),
                    )
                }
                FactoryRow::ApprovalGraceSecs => (
                    "Approval grace period".to_string(),
                    format!("{} s  (0-600 s)", s.approval_grace_secs),
                ),
                FactoryRow::Provider(p_idx) => {
                    let p = &self.factory_providers[p_idx];
                    let state_str = if p.gate == ProviderGateState::Open {
                        let date = s
                            .provider_gates
                            .iter()
                            .find(|g| g.provider == p.provider)
                            .and_then(|g| g.verified_on.as_deref())
                            .unwrap_or(&p.verified_on);
                        format!("open (consent {date})  [revoke]")
                    } else {
                        let reason = p.reason.as_deref().unwrap_or("gate closed");
                        format!("closed ({reason})  [consent]")
                    };
                    (
                        if p_idx == 0 {
                            "Providers".to_string()
                        } else {
                            String::new()
                        },
                        format!("{:<8} {state_str}", p.provider),
                    )
                }
                FactoryRow::ModelTier(t_idx) => {
                    let (provider, tiers) = s
                        .model_tiers
                        .iter()
                        .nth(t_idx)
                        .map(|(k, v)| (k.as_str(), v))
                        .unwrap();
                    let tiers_str = format!(
                        "{provider}: economy {} · standard {} · premium {}",
                        tiers.economy, tiers.standard, tiers.premium
                    );
                    (
                        if t_idx == 0 {
                            "Model tiers".to_string()
                        } else {
                            String::new()
                        },
                        tiers_str,
                    )
                }
                FactoryRow::CheckCommands => {
                    let val = if s.checks.is_empty() {
                        "none confirmed — confirm in Run view or `tandem factory checks confirm`"
                            .to_string()
                    } else {
                        let targets: Vec<&str> = s.checks.keys().map(String::as_str).collect();
                        format!(
                            "confirmed for {} (read-only; confirm via Run view)",
                            targets.join(", ")
                        )
                    };
                    ("Check commands".to_string(), val)
                }
                FactoryRow::DeployCommand => {
                    let val = if s.deploy_commands.is_empty() {
                        "none confirmed — confirm via `tandem factory deploy confirm`".to_string()
                    } else {
                        let targets: Vec<&str> =
                            s.deploy_commands.keys().map(String::as_str).collect();
                        format!(
                            "confirmed for {} (read-only; confirm via CLI)",
                            targets.join(", ")
                        )
                    };
                    ("Deploy command".to_string(), val)
                }
                FactoryRow::AgentLogin(idx) => {
                    let item = &self.factory_logins[idx];
                    let ver = if !item.installed {
                        "✗        ".to_string()
                    } else if item.version.is_empty() {
                        "✓        ".to_string()
                    } else {
                        format!("✓ {:<7}", item.version)
                    };
                    let status = if !item.installed {
                        "Not installed — see README [Recheck]".to_string()
                    } else if item.signed_in {
                        let method = item
                            .method
                            .as_deref()
                            .map(|m| format!(" ({m})"))
                            .unwrap_or_default();
                        let action = if item.has_login_command {
                            "[Sign in…]"
                        } else {
                            "[Own TUI]"
                        };
                        format!("Signed in{method}  {action} [Recheck]")
                    } else if item.has_login_command {
                        "Not signed in  [Sign in] [Recheck]".to_string()
                    } else {
                        "Not signed in — sign in in own TUI [Recheck]".to_string()
                    };
                    (format!("CLI {}", item.cli), format!("{ver} {status}"))
                }
                FactoryRow::DefaultWorkerCli => (
                    "Default worker CLI".to_string(),
                    format!("[{} ▾]", s.default_worker_cli),
                ),
                FactoryRow::OrchestratorMayChoose => (
                    "Orchestrator may choose".to_string(),
                    if s.orchestrator_may_choose {
                        "[✓]".to_string()
                    } else {
                        "[ ]".to_string()
                    },
                ),
            };

            if narrow {
                let first_text = if label.is_empty() {
                    format!("{prefix}{value_and_range}")
                } else {
                    format!("{prefix}{label}")
                };
                lines.push(StyledLine {
                    text: first_text,
                    style,
                    row_index: Some(row_idx),
                });
                if !label.is_empty() {
                    lines.push(StyledLine {
                        text: format!("    {value_and_range}"),
                        style,
                        row_index: Some(row_idx),
                    });
                }
            } else {
                lines.push(StyledLine {
                    text: format!("{prefix}{label:<23} {value_and_range}"),
                    style,
                    row_index: Some(row_idx),
                });
            }
        }

        lines
    }

    fn render_overlays(&self, area: Rect, buf: &mut Buffer) {
        if let Some((ref msg, time)) = self.flash_message
            && time.elapsed().as_secs_f64() < 2.0
            && area.height > 1
        {
            let flash_y = area.y + area.height - 1;
            let flash_style = Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD);
            buf.set_stringn(area.x, flash_y, msg, area.width as usize, flash_style);
        }

        if self.confirm_reset {
            render_modal(area, buf, self.modal_focus);
        } else if let Some(ref edit) = self.factory_edit {
            render_factory_edit_modal(area, buf, edit);
        } else if let Some(ref consent) = self.factory_consent {
            render_factory_consent_modal(area, buf, consent);
        } else if let Some(ref revoke) = self.factory_revoke {
            render_factory_revoke_modal(area, buf, revoke);
        } else if let Some(ref login_pane) = self.factory_login_pane {
            render_factory_login_pane_modal(area, buf, login_pane);
        }
    }
}

fn render_option_badge(buf: &mut Buffer, x: u16, y: u16, label: &str, is_active: bool) -> u16 {
    let bullet = if is_active { "•" } else { " " };
    let text = format!("({bullet}{label}) ");
    let style = if is_active {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    buf.set_string(x, y, &text, style);
    x + text.len() as u16
}

fn clear_box(buf: &mut Buffer, x: u16, y: u16, width: u16, height: u16) {
    for r in y..y + height {
        for c in x..x + width {
            if let Some(cell) = buf.cell_mut((c, r)) {
                cell.set_symbol(" ");
                cell.set_style(Style::default().bg(Color::Black));
            }
        }
    }
}

const EDIT_MODAL_SIZE: (u16, u16) = (62, 6);
const CONSENT_MODAL_SIZE: (u16, u16) = (66, 7);
const REVOKE_MODAL_SIZE: (u16, u16) = (54, 5);
const LOGIN_MODAL_SIZE: (u16, u16) = (64, 5);
const EDIT_MODAL_HINT: &str = "Enter save · Esc cancel";
const CONSENT_MODAL_HINT: &str = "Left/Right auth · Enter consent · Esc cancel";
const LOGIN_MODAL_HINT: &str = "[Done] refresh · [Cancel] close login pane";

fn modal_rect(area: Rect, size: (u16, u16)) -> Rect {
    let width = size.0.min(area.width);
    let height = size.1.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

fn render_modal(area: Rect, buf: &mut Buffer, modal_focus: usize) {
    let width = 50u16.min(area.width);
    let height = 5u16.min(area.height);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;

    clear_box(buf, x, y, width, height);

    if width < 4 || height < 5 {
        return;
    }

    let top = format!(
        "┌ Reset UI {:─<width$}┐",
        "",
        width = (width as usize).saturating_sub(12)
    );
    buf.set_stringn(
        x,
        y,
        &top,
        width as usize,
        Style::default().fg(Color::Yellow),
    );

    let prompt = "  Reset the UI to defaults? Agents keep running.";
    buf.set_stringn(
        x,
        y + 1,
        prompt,
        width as usize,
        Style::default().add_modifier(Modifier::BOLD),
    );

    let reset_style = if modal_focus == 0 {
        Style::default()
            .add_modifier(Modifier::REVERSED)
            .fg(Color::Red)
    } else {
        Style::default().fg(Color::Red)
    };
    let cancel_style = if modal_focus == 1 {
        Style::default()
            .add_modifier(Modifier::REVERSED)
            .fg(Color::White)
    } else {
        Style::default().fg(Color::White)
    };

    if width >= 36 {
        buf.set_stringn(
            x + 14,
            y + 3,
            "[ Reset ]",
            (width as usize).saturating_sub(14),
            reset_style,
        );
        buf.set_stringn(
            x + 26,
            y + 3,
            "[ Cancel ]",
            (width as usize).saturating_sub(26),
            cancel_style,
        );
    }

    let bottom = format!(
        "└{:─<width$}┘",
        "",
        width = (width as usize).saturating_sub(2)
    );
    buf.set_stringn(
        x,
        y + 4,
        &bottom,
        width as usize,
        Style::default().fg(Color::Yellow),
    );
}

fn render_factory_edit_modal(area: Rect, buf: &mut Buffer, edit: &FactoryEditState) {
    let Rect {
        x,
        y,
        width,
        height,
    } = modal_rect(area, EDIT_MODAL_SIZE);
    clear_box(buf, x, y, width, height);
    if width < 10 || height < 5 {
        return;
    }

    let w = width as usize;
    buf.set_stringn(
        x,
        y,
        format!("┌ Edit {} ", edit.label),
        w,
        Style::default().fg(Color::Cyan),
    );
    buf.set_stringn(
        x + 2,
        y + 1,
        format!("> {}_", edit.buffer),
        w.saturating_sub(3),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    );
    if let Some(ref err) = edit.error {
        buf.set_stringn(
            x + 2,
            y + 2,
            err,
            w.saturating_sub(3),
            Style::default().fg(Color::Red),
        );
    }
    buf.set_stringn(
        x + 2,
        y + 3,
        EDIT_MODAL_HINT,
        w.saturating_sub(3),
        Style::default().fg(Color::DarkGray),
    );
    buf.set_stringn(
        x,
        y + height - 1,
        format!("└{:─<pad$}┘", "", pad = w.saturating_sub(2)),
        w,
        Style::default().fg(Color::Cyan),
    );
}

fn render_factory_consent_modal(area: Rect, buf: &mut Buffer, state: &FactoryConsentState) {
    let Rect {
        x,
        y,
        width,
        height,
    } = modal_rect(area, CONSENT_MODAL_SIZE);
    clear_box(buf, x, y, width, height);
    if width < 12 || height < 6 {
        return;
    }

    let w = width as usize;
    let auth = AUTH_TYPES[state.auth_idx];
    buf.set_stringn(
        x,
        y,
        format!("┌ Provider Consent: {} ", state.provider),
        w,
        Style::default().fg(Color::Yellow),
    );
    let verdict = lookup_terms(&state.provider, auth)
        .map(|r| match r.verdict {
            TermsVerdict::Allowed => "allowed",
            TermsVerdict::AllowedWithConditions => "allowed with conditions",
            TermsVerdict::NotAllowed => "not allowed",
            TermsVerdict::Unclear => "unclear",
        })
        .unwrap_or("unclear");
    buf.set_stringn(
        x + 2,
        y + 1,
        format!("Auth: < {} >  Terms: {verdict}", auth.as_str()),
        w.saturating_sub(3),
        Style::default(),
    );
    buf.set_stringn(
        x + 2,
        y + 2,
        format!("Type '{}' to confirm: {}_", state.provider, state.input),
        w.saturating_sub(3),
        Style::default().fg(Color::Yellow),
    );
    if let Some(ref err) = state.error {
        buf.set_stringn(
            x + 2,
            y + 3,
            err,
            w.saturating_sub(3),
            Style::default().fg(Color::Red),
        );
    }
    buf.set_stringn(
        x + 2,
        y + 4,
        CONSENT_MODAL_HINT,
        w.saturating_sub(3),
        Style::default().fg(Color::DarkGray),
    );
    buf.set_stringn(
        x,
        y + height - 1,
        format!("└{:─<pad$}┘", "", pad = w.saturating_sub(2)),
        w,
        Style::default().fg(Color::Yellow),
    );
}

fn render_factory_revoke_modal(area: Rect, buf: &mut Buffer, state: &FactoryRevokeState) {
    let Rect {
        x,
        y,
        width,
        height,
    } = modal_rect(area, REVOKE_MODAL_SIZE);
    clear_box(buf, x, y, width, height);
    if width < 12 || height < 5 {
        return;
    }

    let w = width as usize;
    buf.set_stringn(
        x,
        y,
        format!("┌ Revoke Provider: {} ", state.provider),
        w,
        Style::default().fg(Color::Red),
    );
    buf.set_stringn(
        x + 2,
        y + 1,
        format!("Close gate for {}? (y/n)", state.provider),
        w.saturating_sub(3),
        Style::default().add_modifier(Modifier::BOLD),
    );
    let yes_style = if state.confirm_yes {
        Style::default()
            .fg(Color::Red)
            .add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(Color::Red)
    };
    let no_style = if !state.confirm_yes {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::REVERSED)
    } else {
        Style::default().fg(Color::White)
    };
    buf.set_stringn(x + 12, y + 3, "[ Yes ]", w.saturating_sub(12), yes_style);
    buf.set_stringn(x + 24, y + 3, "[ No ]", w.saturating_sub(24), no_style);
    buf.set_stringn(
        x,
        y + height - 1,
        format!("└{:─<pad$}┘", "", pad = w.saturating_sub(2)),
        w,
        Style::default().fg(Color::Red),
    );
}

fn render_factory_login_pane_modal(area: Rect, buf: &mut Buffer, state: &FactoryLoginPaneState) {
    let Rect {
        x,
        y,
        width,
        height,
    } = modal_rect(area, LOGIN_MODAL_SIZE);
    clear_box(buf, x, y, width, height);
    if width < 16 || height < 5 {
        return;
    }

    let w = width as usize;
    buf.set_stringn(
        x,
        y,
        format!("┌ login: {} ({}) ", state.cli, state.pane_id),
        w,
        Style::default().fg(Color::Cyan),
    );
    let body = if state.confirm_cancel {
        "Cancel sign-in and close login pane? (y/n)"
    } else {
        "Sign in inside the CLI's own pane; tandem never reads credentials."
    };
    buf.set_stringn(
        x + 2,
        y + 1,
        body,
        w.saturating_sub(3),
        Style::default().add_modifier(Modifier::BOLD),
    );
    buf.set_stringn(
        x + 2,
        y + 3,
        LOGIN_MODAL_HINT,
        w.saturating_sub(3),
        Style::default().fg(Color::DarkGray),
    );
    buf.set_stringn(
        x,
        y + height - 1,
        format!("└{:─<pad$}┘", "", pad = w.saturating_sub(2)),
        w,
        Style::default().fg(Color::Cyan),
    );
}
