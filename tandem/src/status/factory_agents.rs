use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::factory::store::{FactoryStore, write_atomic};
use crate::factory::team::{
    GlobalAgentSummary, list_all_agents, relaunch_agent, send_agent_message, terminate_agent,
};
use crate::sanitize::clean;
use crate::status::factory_run::{agent_age_text, agent_state_text};
use crate::tmux::tabs::{AgentCategory, categorize_agent};
use crate::tmux::{TmuxClient, socket_for};
use crate::ui::hits::{Hit, HitMap, Resolved};
use crate::ui::input::TextInput;
use crate::ui::text::truncate_to_width;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentFilter {
    #[default]
    Running,
    All,
}

impl AgentFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::All => "All",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RoleFilter {
    #[default]
    All,
    Orchestrators,
    Planners,
    Testers,
    Implementers,
    Reviewers,
    Ops,
}

impl RoleFilter {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Orchestrators,
            Self::Orchestrators => Self::Planners,
            Self::Planners => Self::Testers,
            Self::Testers => Self::Implementers,
            Self::Implementers => Self::Reviewers,
            Self::Reviewers => Self::Ops,
            Self::Ops => Self::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Orchestrators => "Orchestrators",
            Self::Planners => "Planners",
            Self::Testers => "Testers",
            Self::Implementers => "Implementers",
            Self::Reviewers => "Reviewers",
            Self::Ops => "Ops",
        }
    }

    pub fn matches(self, category: AgentCategory) -> bool {
        match self {
            Self::All => true,
            Self::Orchestrators => category == AgentCategory::Orchestrators,
            Self::Planners => category == AgentCategory::Planners,
            Self::Testers => category == AgentCategory::Testers,
            Self::Implementers => category == AgentCategory::Implementers,
            Self::Reviewers => category == AgentCategory::Reviewers,
            Self::Ops => category == AgentCategory::Ops,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentsKeyOutcome {
    Ignored,
    Consumed,
    Back,
    Refresh,
    OpenRun { idea_id: String, run_id: String },
    OpenWorkerPane { pane_id: String },
}

enum AgentsSub {
    Main,
    Message {
        run_id: String,
        job_id: String,
        input: TextInput,
        error: Option<String>,
    },
    ConfirmStop {
        run_id: String,
        job_id: String,
        input: TextInput,
        error: Option<String>,
    },
}

pub struct FactoryAgents {
    pub workspace: PathBuf,
    pub agents: Vec<GlobalAgentSummary>,
    pub filter: AgentFilter,
    pub role_filter: RoleFilter,
    pub selected: usize,
    pub notice: String,
    pub error: Option<String>,
    sub: AgentsSub,
    pub hits: HitMap,
    pub risks: Vec<crate::factory::auth_risk::RiskItem>,
}

impl std::fmt::Debug for FactoryAgents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FactoryAgents")
            .field("agents", &self.agents.len())
            .field("filter", &self.filter)
            .field("role_filter", &self.role_filter)
            .field("selected", &self.selected)
            .finish()
    }
}

fn is_running(agent: &GlobalAgentSummary) -> bool {
    agent.agent.job_state == "running"
}

fn agent_category(agent: &GlobalAgentSummary) -> AgentCategory {
    categorize_agent(&agent.agent.role, &agent.agent.stage)
}

impl FactoryAgents {
    pub fn with_agents(workspace: PathBuf, agents: Vec<GlobalAgentSummary>) -> Self {
        Self {
            workspace,
            agents,
            filter: AgentFilter::default(),
            role_filter: RoleFilter::default(),
            selected: 0,
            notice: String::new(),
            error: None,
            sub: AgentsSub::Main,
            hits: HitMap::default(),
            risks: Vec::new(),
        }
    }

    pub fn load(workspace: &Path) -> Self {
        let mut view = Self::with_agents(workspace.to_path_buf(), Vec::new());
        view.reload();
        view
    }

    pub fn reload(&mut self) {
        let store = FactoryStore::new(&self.workspace);
        let tmux = TmuxClient::new(socket_for(&self.workspace), None);
        match list_all_agents(&store, &tmux, false) {
            Ok(list) => {
                self.agents = list;
                self.error = None;
            }
            Err(err) => {
                self.agents.clear();
                self.error = Some(clean(&err.to_string()));
            }
        }
        let len = self.visible().len();
        if self.selected >= len {
            self.selected = len.saturating_sub(1);
        }
    }

    pub fn running_count(&self) -> usize {
        self.agents.iter().filter(|a| is_running(a)).count()
    }

    pub fn visible(&self) -> Vec<&GlobalAgentSummary> {
        let mut rows: Vec<&GlobalAgentSummary> = self
            .agents
            .iter()
            .filter(|a| self.filter == AgentFilter::All || is_running(a))
            .filter(|a| self.role_filter.matches(agent_category(a)))
            .collect();
        rows.sort_by(|a, b| {
            is_running(b)
                .cmp(&is_running(a))
                .then_with(|| agent_category(a).order().cmp(&agent_category(b).order()))
                .then_with(|| {
                    let at = |x: &GlobalAgentSummary| {
                        x.agent
                            .activity
                            .as_ref()
                            .and_then(|act| act.last_stop_at)
                            .unwrap_or(0.0)
                    };
                    at(b).total_cmp(&at(a))
                })
                .then_with(|| a.agent.run_id.cmp(&b.agent.run_id))
                .then_with(|| a.agent.job_id.cmp(&b.agent.job_id))
        });
        rows
    }

    pub fn captures_input(&self) -> bool {
        !matches!(self.sub, AgentsSub::Main)
    }

    fn selected_agent(&self) -> Option<GlobalAgentSummary> {
        self.visible().get(self.selected).map(|a| (*a).clone())
    }

    pub fn handle_key(&mut self, event: &KeyEvent) -> AgentsKeyOutcome {
        match &mut self.sub {
            AgentsSub::Message {
                run_id,
                job_id,
                input,
                error,
            } => {
                match event.code {
                    KeyCode::Esc => self.sub = AgentsSub::Main,
                    KeyCode::Enter => {
                        let msg = input.value.trim().to_string();
                        if msg.is_empty() {
                            *error = Some("Message must not be empty.".to_string());
                            return AgentsKeyOutcome::Consumed;
                        }
                        let store = FactoryStore::new(&self.workspace);
                        let path = store.run_dir(run_id).join("tui-message.txt");
                        if let Err(err) = write_atomic(&path, msg.as_bytes()) {
                            *error = Some(clean(&err.to_string()));
                            return AgentsKeyOutcome::Consumed;
                        }
                        let tmux = TmuxClient::new(socket_for(&self.workspace), None);
                        let res = send_agent_message(&store, &tmux, run_id, job_id, &path);
                        let _ = fs::remove_file(&path);
                        match res {
                            Ok(()) => {
                                let job = job_id.clone();
                                self.sub = AgentsSub::Main;
                                self.notice = format!("Sent message to {job}.");
                                self.reload();
                                return AgentsKeyOutcome::Refresh;
                            }
                            Err(err) => *error = Some(clean(&err.to_string())),
                        }
                    }
                    _ => {
                        input.handle_key(event);
                    }
                }
                AgentsKeyOutcome::Consumed
            }
            AgentsSub::ConfirmStop {
                run_id,
                job_id,
                input,
                error,
            } => {
                match event.code {
                    KeyCode::Esc => self.sub = AgentsSub::Main,
                    KeyCode::Enter => {
                        let typed = input.value.trim().to_string();
                        if typed != *job_id {
                            *error = Some(format!("Type {job_id} to confirm stop."));
                            return AgentsKeyOutcome::Consumed;
                        }
                        let store = FactoryStore::new(&self.workspace);
                        let tmux = TmuxClient::new(socket_for(&self.workspace), None);
                        match terminate_agent(&store, &tmux, run_id, job_id, &typed) {
                            Ok(()) => {
                                let job = job_id.clone();
                                self.sub = AgentsSub::Main;
                                self.notice = format!("Stopped worker {job}.");
                                self.reload();
                                return AgentsKeyOutcome::Refresh;
                            }
                            Err(err) => *error = Some(clean(&err.to_string())),
                        }
                    }
                    _ => {
                        input.handle_key(event);
                    }
                }
                AgentsKeyOutcome::Consumed
            }
            AgentsSub::Main => self.handle_main_key(event),
        }
    }

    fn handle_main_key(&mut self, event: &KeyEvent) -> AgentsKeyOutcome {
        let len = self.visible().len();
        match event.code {
            KeyCode::Esc => AgentsKeyOutcome::Back,
            KeyCode::Char('j') | KeyCode::Down => {
                self.selected = (self.selected + 1).min(len.saturating_sub(1));
                AgentsKeyOutcome::Consumed
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                AgentsKeyOutcome::Consumed
            }
            KeyCode::Char('f') => {
                self.filter = match self.filter {
                    AgentFilter::Running => AgentFilter::All,
                    AgentFilter::All => AgentFilter::Running,
                };
                self.selected = 0;
                AgentsKeyOutcome::Consumed
            }
            KeyCode::Char('c') => {
                self.role_filter = self.role_filter.next();
                self.selected = 0;
                AgentsKeyOutcome::Consumed
            }
            KeyCode::Enter => match self.selected_agent() {
                Some(a) => AgentsKeyOutcome::OpenRun {
                    idea_id: a.idea_id,
                    run_id: a.agent.run_id,
                },
                None => AgentsKeyOutcome::Ignored,
            },
            KeyCode::Char('v') => {
                let Some(a) = self.selected_agent() else {
                    return AgentsKeyOutcome::Ignored;
                };
                match (&a.agent.pane_id, a.agent.pane_alive) {
                    (Some(pane), true) => AgentsKeyOutcome::OpenWorkerPane {
                        pane_id: pane.clone(),
                    },
                    _ => {
                        self.notice = format!("Worker {} pane is closed.", a.agent.job_id);
                        AgentsKeyOutcome::Consumed
                    }
                }
            }
            KeyCode::Char('m') => {
                let Some(a) = self.selected_agent() else {
                    return AgentsKeyOutcome::Ignored;
                };
                let mut input = TextInput::with_placeholder("Message for worker");
                input.focused = true;
                self.sub = AgentsSub::Message {
                    run_id: a.agent.run_id,
                    job_id: a.agent.job_id,
                    input,
                    error: None,
                };
                AgentsKeyOutcome::Consumed
            }
            KeyCode::Char('s') => {
                let Some(a) = self.selected_agent() else {
                    return AgentsKeyOutcome::Ignored;
                };
                let mut input = TextInput::with_placeholder(&a.agent.job_id);
                input.focused = true;
                self.sub = AgentsSub::ConfirmStop {
                    run_id: a.agent.run_id,
                    job_id: a.agent.job_id,
                    input,
                    error: None,
                };
                AgentsKeyOutcome::Consumed
            }
            KeyCode::Char('r') => {
                let Some(a) = self.selected_agent() else {
                    return AgentsKeyOutcome::Ignored;
                };
                let store = FactoryStore::new(&self.workspace);
                if let Ok(target) = crate::factory::team::relaunch_target_cli(
                    &store,
                    &a.agent.run_id,
                    &a.agent.job_id,
                ) && let Some(msg) =
                    crate::status::risk_confirm::risk_block_message(&self.risks, &target)
                {
                    self.notice = msg;
                    return AgentsKeyOutcome::Consumed;
                }
                let tmux = TmuxClient::new(socket_for(&self.workspace), None);
                match relaunch_agent(&store, &tmux, &a.agent.run_id, &a.agent.job_id) {
                    Ok(launched) => {
                        self.notice = format!(
                            "Relaunched {} in {} ({}/{}).",
                            a.agent.job_id, launched.pane_id, launched.cli, launched.model
                        );
                        self.reload();
                        AgentsKeyOutcome::Refresh
                    }
                    Err(err) => {
                        self.notice = clean(&err.to_string());
                        AgentsKeyOutcome::Consumed
                    }
                }
            }
            _ => AgentsKeyOutcome::Ignored,
        }
    }

    pub fn mouse_key(&mut self, event: &MouseEvent) -> Option<KeyEvent> {
        match self.hits.resolve(event, KeyCode::Up, KeyCode::Down)? {
            Resolved::Key(key) => Some(key),
            Resolved::Row(idx) => {
                if self.selected == idx {
                    return Some(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                }
                self.selected = idx;
                None
            }
            Resolved::Cmd(_) => None,
        }
    }

    pub fn render(&mut self, now: f64, area: Rect, buf: &mut Buffer) {
        self.hits.clear();
        if area.width == 0 || area.height == 0 {
            return;
        }
        let max_w = area.width as usize;
        let header_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        let dim = Style::default().fg(Color::DarkGray);
        let warn = Style::default().fg(Color::Yellow);

        let running = self.running_count();
        let filter_tag = format!("f filter: {} ▾", self.filter.label());
        let role_tag = format!("c role: {} ▾", self.role_filter.label());
        let header = format!(
            "Agents  {running} running   {filter_tag}   {role_tag}   Enter idea   v view  m msg  s stop  r relaunch   Esc back",
        );
        write_clipped(buf, area.x, area.y, max_w, &header, header_style);
        self.hits.tags(
            Rect::new(area.x, area.y, area.width, 1),
            &header,
            &[
                (filter_tag.as_str(), Hit::ch('f')),
                (role_tag.as_str(), Hit::ch('c')),
                ("Enter idea", Hit::key(KeyCode::Enter)),
                ("v view", Hit::ch('v')),
                ("m msg", Hit::ch('m')),
                ("s stop", Hit::ch('s')),
                ("r relaunch", Hit::ch('r')),
                ("Esc back", Hit::key(KeyCode::Esc)),
            ],
        );
        if area.height == 1 {
            return;
        }

        if let Some(ref err) = self.error {
            write_clipped(
                buf,
                area.x,
                area.y + 1,
                max_w,
                &format!("could not list agents ({})", clean(err)),
                Style::default().fg(Color::Red),
            );
            return;
        }

        let mut y = area.y + 1;
        let bottom = area.y + area.height;
        let footer_rows = u16::from(area.height >= 4);
        let body_bottom = bottom - footer_rows;

        let rows: Vec<GlobalAgentSummary> = self.visible().into_iter().cloned().collect();
        if rows.is_empty() {
            let msg = if self.role_filter != RoleFilter::All {
                "No matching agents for this filter."
            } else {
                match self.filter {
                    AgentFilter::Running => {
                        "No agents running. Agents appear when a run starts a stage."
                    }
                    AgentFilter::All => "No agents recorded.",
                }
            };
            write_clipped(buf, area.x, y, max_w, msg, dim);
        } else {
            let show_tier = area.width >= 80;
            let show_age = area.width >= 100;
            let mut head = format!(
                "  {:<18} {:<18} {:<22}",
                "IDEA", "STAGE / ROLE", "CLI/MODEL"
            );
            if show_tier {
                head.push_str(&format!(" {:<9}", "TIER"));
            }
            head.push_str(&format!(" {:<10}", "STATE"));
            if show_age {
                head.push_str(&format!(" {:<8}", "AGE"));
            }
            write_clipped(buf, area.x, y, max_w, &head, dim);
            y += 1;

            enum RenderLine {
                Divider(AgentCategory),
                AgentRow(usize),
            }
            let mut lines: Vec<RenderLine> = Vec::with_capacity(rows.len() * 2);
            let mut last_cat: Option<AgentCategory> = None;
            let mut selected_visual_line = 0usize;
            for (idx, a) in rows.iter().enumerate() {
                let cat = agent_category(a);
                if last_cat != Some(cat) {
                    lines.push(RenderLine::Divider(cat));
                    last_cat = Some(cat);
                }
                if idx == self.selected {
                    selected_visual_line = lines.len();
                }
                lines.push(RenderLine::AgentRow(idx));
            }

            let capacity = body_bottom.saturating_sub(y) as usize;
            let start = if capacity > 0 && selected_visual_line >= capacity {
                selected_visual_line + 1 - capacity
            } else {
                0
            };
            let cat_style = Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD);
            for line_item in lines.iter().skip(start) {
                if y >= body_bottom {
                    break;
                }
                match line_item {
                    RenderLine::Divider(cat) => {
                        let header_label = cat.sidebar_header();
                        let fill_len = max_w.saturating_sub(header_label.chars().count() + 4);
                        let divider = format!("── {header_label} {}", "─".repeat(fill_len));
                        write_clipped(buf, area.x, y, max_w, &divider, cat_style);
                    }
                    RenderLine::AgentRow(idx) => {
                        let a = &rows[*idx];
                        let is_sel = *idx == self.selected;
                        let marker = if is_sel { "▶ " } else { "  " };
                        let state = if a.agent.job_state == "running" && !a.agent.pane_alive {
                            "pane gone".to_string()
                        } else {
                            agent_state_text(&a.agent)
                        };
                        let tier = a
                            .tier
                            .map(|t| format!("{t:?}").to_lowercase())
                            .unwrap_or_else(|| "-".to_string());
                        let role_clean = clean(&a.agent.role);
                        let stage_clean = clean(&a.agent.stage);
                        let stage_role = if role_clean.is_empty() || role_clean == "factory-worker"
                        {
                            stage_clean
                        } else {
                            format!("{stage_clean} · {role_clean}")
                        };
                        let mut line = format!(
                            "{marker}{:<18} {:<18} {:<22}",
                            truncate_to_width(&clean(a.idea_title.trim()), 18),
                            truncate_to_width(&stage_role, 18),
                            truncate_to_width(
                                &format!("{}/{}", clean(&a.agent.cli), clean(&a.agent.model)),
                                22
                            ),
                        );
                        if show_tier {
                            line.push_str(&format!(" {:<9}", truncate_to_width(&tier, 9)));
                        }
                        line.push_str(&format!(" {:<10}", truncate_to_width(&clean(&state), 10)));
                        if show_age {
                            line.push_str(&format!(
                                " {:<8}",
                                truncate_to_width(&agent_age_text(&a.agent, now), 8)
                            ));
                        }
                        let style = if is_sel {
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD | Modifier::REVERSED)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        write_clipped(buf, area.x, y, max_w, &line, style);
                        self.hits
                            .push(Rect::new(area.x, y, area.width, 1), Hit::Row(*idx));
                    }
                }
                y += 1;
            }
        }

        if footer_rows == 1 {
            let fy = bottom - 1;
            match &self.sub {
                AgentsSub::Message { input, error, .. } => {
                    let text = format!(
                        " {}Message: {}   Enter send   Esc cancel",
                        error
                            .as_ref()
                            .map(|e| format!("{e}   "))
                            .unwrap_or_default(),
                        clean(&input.value),
                    );
                    write_clipped(buf, area.x, fy, max_w, &text, warn);
                }
                AgentsSub::ConfirmStop {
                    job_id,
                    input,
                    error,
                    ..
                } => {
                    let text = format!(
                        " {}Stop {job_id}? type the job id: {}   Enter confirm   Esc cancel",
                        error
                            .as_ref()
                            .map(|e| format!("{e}   "))
                            .unwrap_or_default(),
                        clean(&input.value),
                    );
                    write_clipped(buf, area.x, fy, max_w, &text, warn);
                }
                AgentsSub::Main => {
                    if !self.notice.is_empty() {
                        write_clipped(
                            buf,
                            area.x,
                            fy,
                            max_w,
                            &format!(" {}", clean(&self.notice)),
                            dim,
                        );
                    }
                }
            }
        }
    }
}

fn write_clipped(buf: &mut Buffer, x: u16, y: u16, max_w: usize, text: &str, style: Style) {
    let clipped = truncate_to_width(text, max_w);
    buf.set_stringn(x, y, clipped, max_w, style);
}
