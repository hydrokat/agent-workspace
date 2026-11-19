use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;

use crate::factory::approval::{
    Caller, SnapshotVerification, mark_ready_with_workflow, revoke_ready, verify_snapshot,
};
use crate::factory::artifacts::{self, REQUIRED_PLAN_SECTIONS};
use crate::factory::events::{self, Event};
use crate::factory::evidence;
use crate::factory::handoff;
use crate::factory::machine::EventData;
use crate::factory::model::{
    Actor, ActorKind, FeasibilityVerdict, Idea, IdeaState, PrLink, Risk, Run, RunState, StageState,
};
use crate::factory::questions::{self, IdeaQuestions};
use crate::factory::run::{get_run, withdraw_run};
use crate::factory::store::FactoryStore;
use crate::factory::workflow::load_default_workflow;
use crate::factory::workflow::parse::load_workflow_file;
use crate::factory::workflow::schema::EvidenceKind;
use crate::factory::workflow::validate::validate;
use crate::sanitize::clean;
use crate::snapshot::IdeaCard;
use crate::status::render::age;
use crate::tmux::keys::{FACTORY_MARK_READY_KEY, display_shortcut, matches_crossterm_event};
use crate::ui::input::TextInput;
use crate::ui::link::{is_github_url, osc8};
use crate::ui::scroll::ScrollView;
use crate::ui::text::truncate_to_width;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowOption {
    pub name: String,
    pub version: u32,
    pub valid: bool,
    pub diagnostic: Option<String>,
    pub recommended: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailSubView {
    Main,
    Reader {
        title: String,
        raw_text: String,
        scroll: ScrollView,
    },
    ConfirmReady {
        input: TextInput,
        error: Option<String>,
        workflows: Vec<WorkflowOption>,
        selected_wf: usize,
        deploy_requested: bool,
    },
    ConfirmRevoke,
    ConfirmWithdraw {
        run_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailKeyOutcome {
    Ignored,
    Consumed,
    BackToBoard,
    Action(DetailAction),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailAction {
    OpenOrResumeOrchestrator { idea_id: String },
    OpenRunMonitor { run_id: String },
    OpenRunEvidence { run_id: String },
    OpenAudit { idea_id: String },
    Refresh,
}

#[derive(Debug, Clone)]
pub struct FactoryDetail {
    pub idea_id: String,
    pub workspace: PathBuf,
    pub idea: Option<Idea>,
    pub run: Option<Run>,
    pub events: Vec<Event>,
    pub questions: IdeaQuestions,
    pub snapshot_verification: Option<SnapshotVerification>,
    pub pane_open: bool,
    pub sub_view: DetailSubView,
    pub notice: String,
    pub error: Option<String>,
    pub platform: String,
    pub owner: String,
    pub loaded_updated: Option<f64>,
    pub loaded_state: Option<IdeaState>,
    pub loaded_blockers: Option<usize>,
    pub loaded_open_questions: Option<usize>,
}

impl FactoryDetail {
    pub fn load(
        workspace: &Path,
        idea_id: &str,
        queries: &[crate::coordination::schema::Query],
        pane_open: bool,
    ) -> Self {
        let owner = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .unwrap_or_else(|_| "you".to_string());
        let mut detail = Self {
            idea_id: idea_id.to_string(),
            workspace: workspace.to_path_buf(),
            idea: None,
            run: None,
            events: Vec::new(),
            questions: IdeaQuestions::default(),
            snapshot_verification: None,
            pane_open,
            sub_view: DetailSubView::Main,
            notice: String::new(),
            error: None,
            platform: std::env::consts::OS.to_string(),
            owner,
            loaded_updated: None,
            loaded_state: None,
            loaded_blockers: None,
            loaded_open_questions: None,
        };
        detail.reload(queries, pane_open);
        detail
    }

    pub fn reload(&mut self, queries: &[crate::coordination::schema::Query], pane_open: bool) {
        self.pane_open = pane_open;
        self.questions = questions::for_idea(queries, &self.idea_id);
        self.loaded_open_questions = Some(self.questions.open);
        self.loaded_blockers = Some(self.questions.blocking);

        let store = FactoryStore::new(&self.workspace);
        match store.get_idea(&self.idea_id) {
            Ok(Some(idea)) => {
                self.loaded_updated = Some(idea.updated);
                self.loaded_state = Some(idea.state);
                self.snapshot_verification = verify_snapshot(&store, &idea);
                self.run = idea
                    .runs
                    .last()
                    .and_then(|run_id| get_run(&store, run_id).ok().flatten());
                self.idea = Some(idea);
                self.error = None;
            }
            Ok(None) => {
                self.idea = None;
                self.run = None;
                self.snapshot_verification = None;
                self.error = Some(format!("idea not found: {}", self.idea_id));
            }
            Err(err) => {
                self.idea = None;
                self.run = None;
                self.snapshot_verification = None;
                self.error = Some(clean(&err.to_string()));
            }
        }

        let log_path = store.root.join("events.jsonl");
        if log_path.exists()
            && let Ok(res) = events::read_all(&log_path)
        {
            self.events = res
                .events
                .into_iter()
                .filter(|ev| ev.idea == self.idea_id)
                .collect();
        } else {
            self.events.clear();
        }
    }

    pub fn sync_if_changed(
        &mut self,
        card: Option<&IdeaCard>,
        queries: &[crate::coordination::schema::Query],
        pane_open: bool,
    ) {
        self.pane_open = pane_open;
        let q = questions::for_idea(queries, &self.idea_id);
        let card_changed = match card {
            Some(c) => {
                self.loaded_updated != Some(c.updated)
                    || self.loaded_state != Some(c.state)
                    || self.loaded_blockers != Some(c.blockers)
            }
            None => false,
        };
        let questions_changed =
            self.loaded_open_questions != Some(q.open) || self.loaded_blockers != Some(q.blocking);
        if card_changed || questions_changed {
            self.reload(queries, pane_open);
        }
    }

    pub fn captures_input(&self) -> bool {
        matches!(self.sub_view, DetailSubView::ConfirmReady { .. })
    }

    pub fn handle_key(&mut self, event: &KeyEvent) -> DetailKeyOutcome {
        match &mut self.sub_view {
            DetailSubView::Reader {
                raw_text, scroll, ..
            } => {
                if event.code == KeyCode::Esc {
                    self.sub_view = DetailSubView::Main;
                    return DetailKeyOutcome::Consumed;
                }
                let lines = wrap_text(raw_text, 80);
                if scroll.handle_key(event.code, lines.len(), 12) {
                    return DetailKeyOutcome::Consumed;
                }
                DetailKeyOutcome::Consumed
            }
            DetailSubView::ConfirmReady {
                input,
                error,
                workflows,
                selected_wf,
                deploy_requested,
            } => match event.code {
                KeyCode::Esc => {
                    self.sub_view = DetailSubView::Main;
                    DetailKeyOutcome::Consumed
                }
                KeyCode::Char('w') if input.value.is_empty() => {
                    let valid_indices: Vec<usize> = workflows
                        .iter()
                        .enumerate()
                        .filter(|(_, w)| w.valid)
                        .map(|(i, _)| i)
                        .collect();
                    if let Some(pos) = valid_indices.iter().position(|&i| i == *selected_wf) {
                        *selected_wf = valid_indices[(pos + 1) % valid_indices.len()];
                    }
                    DetailKeyOutcome::Consumed
                }
                KeyCode::Char('d') if input.value.is_empty() => {
                    *deploy_requested = !*deploy_requested;
                    DetailKeyOutcome::Consumed
                }
                KeyCode::Enter => {
                    let typed = input.value.trim().to_string();
                    if typed != self.idea_id {
                        *error = Some(format!("Type \"{}\" to confirm.", self.idea_id));
                        return DetailKeyOutcome::Consumed;
                    }
                    let wf_name = workflows.get(*selected_wf).map(|w| w.name.as_str());
                    let deploy = *deploy_requested;
                    let store = FactoryStore::new(&self.workspace);
                    let owner = self.owner.clone();
                    let env_fn = |var: &str| std::env::var(var).ok();
                    let no_pane = |_: &str| None;
                    let caller = Caller::tui(&env_fn, &no_pane);
                    match mark_ready_with_workflow(
                        &store,
                        &self.workspace,
                        &self.idea_id,
                        &typed,
                        &owner,
                        wf_name,
                        deploy,
                        &caller,
                    ) {
                        Ok(res) => {
                            self.notice = if let Some(ref run_err) = res.run_error {
                                format!("Ready, but run failed: {run_err}")
                            } else {
                                res.message.to_string()
                            };
                            self.sub_view = DetailSubView::Main;
                            self.reload(&[], self.pane_open);
                            DetailKeyOutcome::Action(DetailAction::Refresh)
                        }
                        Err(err) => {
                            *error = Some(clean(&err.to_string()));
                            DetailKeyOutcome::Consumed
                        }
                    }
                }
                _ => {
                    input.handle_key(event);
                    *error = None;
                    DetailKeyOutcome::Consumed
                }
            },
            DetailSubView::ConfirmWithdraw { run_id } => {
                let run_id = run_id.clone();
                match event.code {
                    KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                        self.sub_view = DetailSubView::Main;
                        DetailKeyOutcome::Consumed
                    }
                    KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                        let store = FactoryStore::new(&self.workspace);
                        let actor = Actor {
                            kind: ActorKind::User,
                            id: self.owner.clone(),
                        };
                        match withdraw_run(&store, &run_id, &actor) {
                            Ok(_) => {
                                self.notice = format!("Withdrew run {run_id}.");
                                self.sub_view = DetailSubView::Main;
                                self.reload(&[], self.pane_open);
                                DetailKeyOutcome::Action(DetailAction::Refresh)
                            }
                            Err(err) => {
                                self.notice = clean(&err.to_string());
                                self.sub_view = DetailSubView::Main;
                                DetailKeyOutcome::Consumed
                            }
                        }
                    }
                    _ => DetailKeyOutcome::Consumed,
                }
            }
            DetailSubView::ConfirmRevoke => match event.code {
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.sub_view = DetailSubView::Main;
                    DetailKeyOutcome::Consumed
                }
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    let store = FactoryStore::new(&self.workspace);
                    let owner = self.owner.clone();
                    let env_fn = |var: &str| std::env::var(var).ok();
                    let no_pane = |_: &str| None;
                    let caller = Caller::tui(&env_fn, &no_pane);
                    match revoke_ready(&store, &self.idea_id, &owner, &caller) {
                        Ok(_) => {
                            self.notice = "Revoked ready for development.".to_string();
                            self.sub_view = DetailSubView::Main;
                            self.reload(&[], self.pane_open);
                            DetailKeyOutcome::Action(DetailAction::Refresh)
                        }
                        Err(err) => {
                            self.notice = clean(&err.to_string());
                            self.sub_view = DetailSubView::Main;
                            DetailKeyOutcome::Consumed
                        }
                    }
                }
                _ => DetailKeyOutcome::Consumed,
            },
            DetailSubView::Main => {
                if matches_crossterm_event(FACTORY_MARK_READY_KEY, event, &self.platform) {
                    self.trigger_mark_ready_or_revoke();
                    return DetailKeyOutcome::Consumed;
                }
                match event.code {
                    KeyCode::Esc => DetailKeyOutcome::BackToBoard,
                    KeyCode::Enter => {
                        if let Some(ref run) = self.run {
                            return DetailKeyOutcome::Action(DetailAction::OpenRunMonitor {
                                run_id: run.id.clone(),
                            });
                        }
                        DetailKeyOutcome::Ignored
                    }
                    KeyCode::Char('f') => {
                        self.open_artifact("feasibility", "Feasibility study");
                        DetailKeyOutcome::Consumed
                    }
                    KeyCode::Char('p') => {
                        self.open_artifact("plan", "Plan");
                        DetailKeyOutcome::Consumed
                    }
                    KeyCode::Char('a') => {
                        self.open_assumptions();
                        DetailKeyOutcome::Consumed
                    }
                    KeyCode::Char('u') => DetailKeyOutcome::Action(DetailAction::OpenAudit {
                        idea_id: self.idea_id.clone(),
                    }),
                    KeyCode::Char('h') => {
                        self.open_handoff();
                        DetailKeyOutcome::Consumed
                    }
                    KeyCode::Char('e') => {
                        if let Some(ref run) = self.run {
                            return DetailKeyOutcome::Action(DetailAction::OpenRunEvidence {
                                run_id: run.id.clone(),
                            });
                        }
                        self.notice = "No run recorded yet.".to_string();
                        DetailKeyOutcome::Consumed
                    }
                    KeyCode::Char('r') => {
                        let Some(run_id) = self.run.as_ref().map(|r| r.id.clone()) else {
                            self.notice = "No run recorded yet.".to_string();
                            return DetailKeyOutcome::Consumed;
                        };
                        let store = FactoryStore::new(&self.workspace);
                        let actor = Actor {
                            kind: ActorKind::User,
                            id: self.owner.clone(),
                        };
                        match handoff::refresh_run_prs(&store, &self.workspace, &run_id, &actor) {
                            Ok(prs) => {
                                self.reload(&[], self.pane_open);
                                self.notice = if prs.is_empty() {
                                    "No pull requests recorded.".to_string()
                                } else {
                                    format!("Refreshed {} pull request(s).", prs.len())
                                };
                                DetailKeyOutcome::Action(DetailAction::Refresh)
                            }
                            Err(err) => {
                                self.notice = clean(&err.to_string());
                                DetailKeyOutcome::Consumed
                            }
                        }
                    }
                    KeyCode::Char('x') => {
                        if let Some(ref run) = self.run
                            && run.state == RunState::Queued
                        {
                            self.notice.clear();
                            self.sub_view = DetailSubView::ConfirmWithdraw {
                                run_id: run.id.clone(),
                            };
                        }
                        DetailKeyOutcome::Consumed
                    }
                    KeyCode::Char('o') => {
                        if let Some(ref idea) = self.idea {
                            if idea.orchestrator.is_some() {
                                return DetailKeyOutcome::Action(
                                    DetailAction::OpenOrResumeOrchestrator {
                                        idea_id: self.idea_id.clone(),
                                    },
                                );
                            }
                            self.notice = "No orchestrator bound to this idea.".to_string();
                        }
                        DetailKeyOutcome::Consumed
                    }
                    _ => DetailKeyOutcome::Ignored,
                }
            }
        }
    }

    fn open_artifact(&mut self, kind: &str, title: &str) {
        let store = FactoryStore::new(&self.workspace);
        match artifacts::read(&store, &self.idea_id, kind, None) {
            Ok(text) => {
                self.notice.clear();
                self.sub_view = DetailSubView::Reader {
                    title: title.to_string(),
                    raw_text: clean(&text),
                    scroll: ScrollView::new(),
                };
            }
            Err(_) => {
                self.notice = format!("No {kind} stored yet.");
            }
        }
    }

    fn open_assumptions(&mut self) {
        let Some(ref idea) = self.idea else {
            return;
        };
        if idea.assumptions.is_empty() {
            self.notice = "No assumptions recorded yet.".to_string();
            return;
        }
        let mut out = String::new();
        for (idx, a) in idea.assumptions.iter().enumerate() {
            if idx > 0 {
                out.push('\n');
            }
            out.push_str(&format!("{}. {}", idx + 1, clean(a)));
        }
        self.notice.clear();
        self.sub_view = DetailSubView::Reader {
            title: format!("Assumptions ({})", idea.assumptions.len()),
            raw_text: out,
            scroll: ScrollView::new(),
        };
    }

    fn open_handoff(&mut self) {
        let Some(ref run) = self.run else {
            self.notice = "No handoff recorded yet.".to_string();
            return;
        };
        let store = FactoryStore::new(&self.workspace);
        if let Some(item) = run
            .evidence
            .iter()
            .rfind(|i| i.kind == EvidenceKind::Handoff)
            && let Ok(text) = evidence::read_item(&store, &run.id, &item.id)
        {
            self.notice.clear();
            self.sub_view = DetailSubView::Reader {
                title: "Handoff".to_string(),
                raw_text: clean(&text),
                scroll: ScrollView::new(),
            };
            return;
        }
        for path in [
            store.run_dir(&run.id).join("artifacts").join("handoff.md"),
            store.run_dir(&run.id).join("handoff.md"),
        ] {
            if path.is_file()
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                self.notice.clear();
                self.sub_view = DetailSubView::Reader {
                    title: "Handoff".to_string(),
                    raw_text: clean(&text),
                    scroll: ScrollView::new(),
                };
                return;
            }
        }
        self.notice = "No handoff recorded yet.".to_string();
    }

    fn recorded_prs(&self) -> &[PrLink] {
        if let Some(ref run) = self.run
            && !run.prs.is_empty()
        {
            return &run.prs;
        }
        if let Some(ref idea) = self.idea {
            return &idea.prs;
        }
        &[]
    }

    pub fn pr_terminal_links(&self) -> Vec<(String, String)> {
        self.recorded_prs()
            .iter()
            .filter(|pr| is_github_url(&pr.url))
            .map(|pr| {
                let label = format!("#{}", pr.number);
                let link = osc8(&label, &pr.url);
                (label, link)
            })
            .collect()
    }

    fn trigger_mark_ready_or_revoke(&mut self) {
        let Some(ref idea) = self.idea else {
            return;
        };
        match idea.state {
            IdeaState::Planning => {
                if let Some(reason) = self.unmet_gate_reason(idea) {
                    self.notice = reason;
                    return;
                }
                let mut input = TextInput::with_placeholder(&self.idea_id);
                input.focused = true;
                let recommended = idea
                    .plan
                    .as_ref()
                    .map(|p| p.workflow.as_str())
                    .unwrap_or("");
                let (workflows, selected_wf) =
                    Self::load_workflow_options(&self.workspace, recommended);
                self.notice.clear();
                self.sub_view = DetailSubView::ConfirmReady {
                    input,
                    error: None,
                    workflows,
                    selected_wf,
                    deploy_requested: false,
                };
            }
            IdeaState::ReadyForDevelopment => {
                self.notice.clear();
                self.sub_view = DetailSubView::ConfirmRevoke;
            }
            _ => {
                self.notice = format!(
                    "Cannot mark ready in {}: advance to Planning first.",
                    idea.state.label()
                );
            }
        }
    }

    fn load_workflow_options(workspace: &Path, recommended: &str) -> (Vec<WorkflowOption>, usize) {
        let store = FactoryStore::new(workspace);
        let settings = if store.exists() {
            store.settings().unwrap_or_default()
        } else {
            Default::default()
        };

        let mut valid_opts = Vec::new();
        let mut invalid_opts = Vec::new();

        let mut seen_default = false;
        let dir = workspace.join("workflows");
        if dir.is_dir()
            && let Ok(entries) = std::fs::read_dir(&dir)
        {
            let mut paths: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("yaml"))
                .collect();
            paths.sort();
            for path in paths {
                let stem = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                if stem == "default-delivery" {
                    seen_default = true;
                }
                match load_workflow_file(&path) {
                    Ok(raw) => match validate(&raw, &settings) {
                        Ok(wf) => {
                            let is_rec = wf.name == recommended || stem == recommended;
                            valid_opts.push(WorkflowOption {
                                name: wf.name,
                                version: wf.version,
                                valid: true,
                                diagnostic: None,
                                recommended: is_rec,
                            });
                        }
                        Err(diags) => {
                            let first = diags
                                .first()
                                .map(|d| format!("{} {}", d.code, d.message))
                                .unwrap_or_else(|| "invalid".to_string());
                            let ver = raw
                                .version
                                .as_ref()
                                .and_then(|v| u32::try_from(v.value).ok())
                                .unwrap_or(1);
                            invalid_opts.push(WorkflowOption {
                                name: stem,
                                version: ver,
                                valid: false,
                                diagnostic: Some(first),
                                recommended: false,
                            });
                        }
                    },
                    Err(err) => {
                        invalid_opts.push(WorkflowOption {
                            name: stem,
                            version: 1,
                            valid: false,
                            diagnostic: Some(err.to_string()),
                            recommended: false,
                        });
                    }
                }
            }
        }

        if !seen_default
            && let Ok((raw, _)) = load_default_workflow(workspace)
            && let Ok(wf) = validate(&raw, &settings)
        {
            let is_rec = wf.name == recommended;
            valid_opts.insert(
                0,
                WorkflowOption {
                    name: wf.name,
                    version: wf.version,
                    valid: true,
                    diagnostic: None,
                    recommended: is_rec,
                },
            );
        }

        let selected = valid_opts
            .iter()
            .position(|w| w.recommended)
            .or_else(|| valid_opts.iter().position(|w| w.name == "default-delivery"))
            .unwrap_or(0);

        valid_opts.extend(invalid_opts);
        (valid_opts, selected)
    }

    fn unmet_gate_reason(&self, idea: &Idea) -> Option<String> {
        let Some(ref plan) = idea.plan else {
            return Some("Cannot mark ready yet: plan is missing.".to_string());
        };
        if !plan.missing_sections.is_empty() {
            return Some(format!(
                "Cannot mark ready yet: plan is missing required sections: {}.",
                plan.missing_sections.join(", ")
            ));
        }
        if self.questions.blocking > 0 {
            let noun = if self.questions.blocking == 1 {
                "question is"
            } else {
                "questions are"
            };
            return Some(format!(
                "Cannot mark ready yet: {} blocking {noun} open. Answer it in the Queries tab.",
                self.questions.blocking
            ));
        }
        None
    }

    pub fn render(&mut self, now: f64, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        let max_w = area.width as usize;

        match &mut self.sub_view {
            DetailSubView::Reader {
                title,
                raw_text,
                scroll,
            } => {
                let header_style = Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD);
                let dim_style = Style::default().fg(Color::DarkGray);
                write_clipped(
                    buf,
                    area.x,
                    area.y,
                    max_w,
                    &format!("◀ Detail   {}", clean(title)),
                    header_style,
                );
                if area.height == 1 {
                    return;
                }
                let show_hint = area.height >= 3;
                let body_h = area.height.saturating_sub(1 + u16::from(show_hint)).max(1);
                let wrapped = wrap_text(raw_text, max_w.max(1));
                let lines: Vec<Line> = wrapped.into_iter().map(Line::from).collect();
                let body_rect = Rect::new(area.x, area.y + 1, area.width, body_h);
                scroll.render_lines(&lines, body_rect, buf);
                if show_hint {
                    write_clipped(
                        buf,
                        area.x,
                        area.y + area.height - 1,
                        max_w,
                        "↑/↓ scroll   PgUp/PgDn page   Esc back",
                        dim_style,
                    );
                }
                return;
            }
            DetailSubView::ConfirmReady {
                input,
                error,
                workflows,
                selected_wf,
                deploy_requested,
            } => {
                let header_style = Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD);
                let text_style = Style::default().fg(Color::White);
                let err_style = Style::default().fg(Color::Red);
                let dim_style = Style::default().fg(Color::DarkGray);

                let plan_ver = self
                    .idea
                    .as_ref()
                    .and_then(|i| i.plan.as_ref())
                    .map(|p| p.artifact.version)
                    .unwrap_or(1);

                let mut y = area.y;
                let bottom = area.y + area.height;
                write_clipped(
                    buf,
                    area.x,
                    y,
                    max_w,
                    &format!("Mark {} ready for development?", self.idea_id),
                    header_style,
                );
                y += 1;
                if y < bottom {
                    let auth_msg = format!(
                        "This freezes plan v{plan_ver} and authorizes Factory Mode to spend budget, change the repository on a new branch, and open a pull request."
                    );
                    for wrapped in wrap_text(&auth_msg, max_w.max(1)) {
                        if y >= bottom {
                            break;
                        }
                        write_clipped(buf, area.x, y, max_w, &wrapped, text_style);
                        y += 1;
                    }
                }
                if y < bottom {
                    let wf_line = if let Some(wf) = workflows.get(*selected_wf) {
                        let rec = if wf.recommended {
                            " (recommended by the Orchestrator)"
                        } else {
                            ""
                        };
                        format!("Workflow     {} v{}{}   w change", wf.name, wf.version, rec)
                    } else {
                        "Workflow     default-delivery v1   w change".to_string()
                    };
                    write_clipped(buf, area.x, y, max_w, &wf_line, text_style);
                    y += 1;
                }
                for inv in workflows.iter().filter(|w| !w.valid) {
                    if y >= bottom {
                        break;
                    }
                    let diag = inv.diagnostic.as_deref().unwrap_or("invalid");
                    let line = format!(
                        "             {} v{} · invalid: {}",
                        inv.name, inv.version, diag
                    );
                    write_clipped(buf, area.x, y, max_w, &line, dim_style);
                    y += 1;
                }
                if y < bottom {
                    let deploy_line = if *deploy_requested {
                        "Deployment   requested (needs a separate approval when requested)   d toggle"
                    } else {
                        "Deployment   not requested (needs a separate approval when requested)   d toggle"
                    };
                    write_clipped(buf, area.x, y, max_w, deploy_line, text_style);
                    y += 1;
                }
                if y < bottom {
                    let prompt = format!("Type the idea id to confirm: {}_", clean(&input.value));
                    write_clipped(buf, area.x, y, max_w, &prompt, text_style);
                    y += 1;
                }
                if let Some(err) = error
                    && y < bottom
                {
                    write_clipped(buf, area.x, y, max_w, err, err_style);
                    y += 1;
                }
                if y < bottom {
                    write_clipped(
                        buf,
                        area.x,
                        area.y + area.height - 1,
                        max_w,
                        "Enter confirm   Esc cancel",
                        dim_style,
                    );
                }
                return;
            }
            DetailSubView::ConfirmRevoke => {
                let header_style = Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD);
                let dim_style = Style::default().fg(Color::DarkGray);
                write_clipped(
                    buf,
                    area.x,
                    area.y,
                    max_w,
                    &format!("Revoke ready-for-development for {}?", self.idea_id),
                    header_style,
                );
                if area.height >= 2 {
                    write_clipped(
                        buf,
                        area.x,
                        area.y + 1,
                        max_w,
                        "y / Enter confirm revoke   n / Esc cancel",
                        dim_style,
                    );
                }
                return;
            }
            DetailSubView::ConfirmWithdraw { run_id } => {
                let header_style = Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD);
                let dim_style = Style::default().fg(Color::DarkGray);
                write_clipped(
                    buf,
                    area.x,
                    area.y,
                    max_w,
                    &format!("Withdraw queued run {}?", run_id),
                    header_style,
                );
                if area.height >= 2 {
                    write_clipped(
                        buf,
                        area.x,
                        area.y + 1,
                        max_w,
                        "y / Enter confirm withdraw   n / Esc cancel",
                        dim_style,
                    );
                }
                return;
            }
            DetailSubView::Main => {}
        }

        let header_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        let text_style = Style::default().fg(Color::White);
        let dim_style = Style::default().fg(Color::DarkGray);
        let ok_style = Style::default().fg(Color::Green);
        let warn_style = Style::default().fg(Color::Yellow);
        let err_style = Style::default().fg(Color::Red);

        if let Some(ref err) = self.error {
            write_clipped(
                buf,
                area.x,
                area.y,
                max_w,
                &format!("◀ Ideas   {}   ({err})", self.idea_id),
                err_style,
            );
            return;
        }

        let Some(ref idea) = self.idea else {
            return;
        };

        let narrow = area.width < 80;
        let short = area.height <= 8;
        let risk_str = match idea.risk {
            None => "none",
            Some(Risk::Low) => "low",
            Some(Risk::Medium) => "medium",
            Some(Risk::High) => "high",
        };
        let clean_title = clean(&idea.title);
        let mut y = area.y;
        let bottom = area.y + area.height;

        if narrow {
            write_clipped(
                buf,
                area.x,
                y,
                max_w,
                &format!("◀ Ideas  {}  {}", idea.id, idea.state.label()),
                header_style,
            );
            y += 1;
            if y < bottom {
                write_clipped(
                    buf,
                    area.x,
                    y,
                    max_w,
                    &format!("{clean_title}  (risk: {risk_str})"),
                    text_style,
                );
                y += 1;
            }
        } else {
            let header = format!(
                "◀ Ideas   {}  {}   {}   risk: {}",
                idea.id,
                clean_title,
                idea.state.label(),
                risk_str
            );
            write_clipped(buf, area.x, y, max_w, &header, header_style);
            y += 1;
        }

        if y < bottom {
            let orch_line = match &idea.orchestrator {
                Some(orch) => {
                    let conv = if orch.conversation.is_some() {
                        "conversation saved"
                    } else {
                        "no saved conversation"
                    };
                    let pane = if self.pane_open { "open" } else { "closed" };
                    format!(
                        "Orchestrator: {} / {}   {}   pane: {}",
                        clean(&orch.cli),
                        clean(&orch.model),
                        conv,
                        pane
                    )
                }
                None => "Orchestrator: none".to_string(),
            };
            write_clipped(buf, area.x, y, max_w, &orch_line, text_style);
            y += 1;
        }

        if y < bottom {
            let feas_part = match (&idea.feasibility, idea.feasibility_verdict) {
                (Some(f), Some(v)) => {
                    let vw = match v {
                        FeasibilityVerdict::Feasible => "feasible",
                        FeasibilityVerdict::FeasibleWithRisks => "feasible with risks",
                        FeasibilityVerdict::Infeasible => "infeasible",
                    };
                    format!("Feasibility  v{}  {vw}", f.version)
                }
                (Some(f), None) => format!("Feasibility  v{}", f.version),
                (None, _) => "Feasibility  missing".to_string(),
            };
            let total_sec = REQUIRED_PLAN_SECTIONS.len();
            let plan_part = match &idea.plan {
                Some(p) => {
                    let present = total_sec.saturating_sub(p.missing_sections.len());
                    if p.missing_sections.is_empty() {
                        format!(
                            "Plan  v{}  {present}/{total_sec} sections",
                            p.artifact.version
                        )
                    } else {
                        format!(
                            "Plan  v{}  {present}/{total_sec} sections (missing: {})",
                            p.artifact.version,
                            p.missing_sections.join(", ")
                        )
                    }
                }
                None => "Plan  missing".to_string(),
            };
            let assump_part = format!("Assumptions {}", idea.assumptions.len());

            if short && area.height <= 5 {
                let compact = format!(
                    "F:{}  P:{}  A:{}",
                    idea.feasibility.as_ref().map(|f| f.version).unwrap_or(0),
                    idea.plan.as_ref().map(|p| p.artifact.version).unwrap_or(0),
                    idea.assumptions.len()
                );
                write_clipped(buf, area.x, y, max_w, &compact, text_style);
                y += 1;
            } else if narrow {
                write_clipped(buf, area.x, y, max_w, &feas_part, text_style);
                y += 1;
                if y < bottom {
                    write_clipped(buf, area.x, y, max_w, &plan_part, text_style);
                    y += 1;
                }
                if y < bottom {
                    write_clipped(buf, area.x, y, max_w, &assump_part, text_style);
                    y += 1;
                }
            } else {
                let art_line = format!("{feas_part}      {plan_part}   {assump_part}");
                write_clipped(buf, area.x, y, max_w, &art_line, text_style);
                y += 1;
            }
        }

        if y < bottom {
            let q_line = format!(
                "Questions    {} open, {} blocking → Queries tab",
                self.questions.open, self.questions.blocking
            );
            let q_style = if self.questions.blocking > 0 {
                warn_style
            } else {
                text_style
            };
            write_clipped(buf, area.x, y, max_w, &q_line, q_style);
            y += 1;
        }

        if let Some(ref run) = self.run
            && y < bottom
        {
            let run_state_label = match run.state {
                RunState::Pending => "Pending",
                RunState::Queued => "Queued",
                RunState::Running => "Running",
                RunState::Paused => "Paused",
                RunState::Review => "Review",
                RunState::Succeeded => "Succeeded",
                RunState::Delivered => "Delivered",
                RunState::Failed => "Failed",
                RunState::Cancelled => "Cancelled",
                RunState::Withdrawn => "Withdrawn",
            };
            let withdraw_hint = if run.state == RunState::Queued {
                "   x withdraw"
            } else {
                ""
            };
            let run_hdr = format!(
                "Run {}   {}   {} v{}{}",
                run.id, run_state_label, run.workflow, run.workflow_version, withdraw_hint
            );
            write_clipped(buf, area.x, y, max_w, &run_hdr, ok_style);
            y += 1;

            if narrow {
                for st in &run.stages {
                    if y >= bottom {
                        break;
                    }
                    let word = match st.state {
                        StageState::Pending => "pending",
                        StageState::Running => "running",
                        StageState::Succeeded => "done",
                        StageState::Failed => "failed",
                        StageState::Skipped => "skipped",
                        StageState::Blocked => "blocked",
                    };
                    write_clipped(
                        buf,
                        area.x,
                        y,
                        max_w,
                        &format!("  {}: {}", st.name, word),
                        text_style,
                    );
                    y += 1;
                }
            } else if y < bottom {
                let mut strip = String::new();
                for (idx, st) in run.stages.iter().enumerate() {
                    if idx > 0 {
                        strip.push_str("  ");
                    }
                    let sym = match st.state {
                        StageState::Pending => "○",
                        StageState::Running => "●",
                        StageState::Succeeded => "✓",
                        StageState::Failed => "✗",
                        StageState::Skipped => "–",
                        StageState::Blocked => "!",
                    };
                    strip.push_str(&format!("{} {}", st.name, sym));
                }
                write_clipped(buf, area.x, y, max_w, &strip, text_style);
                y += 1;
            }

            if y < bottom && !short {
                write_clipped(
                    buf,
                    area.x,
                    y,
                    max_w,
                    "○ pending   ● running   ✓ done   ✗ failed   – skipped   ! blocked",
                    dim_style,
                );
                y += 1;
            }

            let prs = self.recorded_prs();
            if (idea.state == IdeaState::Delivered || !prs.is_empty()) && y < bottom {
                if prs.is_empty() {
                    write_clipped(
                        buf,
                        area.x,
                        y,
                        max_w,
                        "Pull request   none created yet",
                        dim_style,
                    );
                    y += 1;
                } else {
                    for pr in prs {
                        if y >= bottom {
                            break;
                        }
                        let state = pr.state.as_deref().unwrap_or("unknown");
                        let checks = pr.checks_summary.as_deref().unwrap_or("checks unknown");
                        let line = if is_github_url(&pr.url) {
                            format!(
                                "Pull request   #{} · {} · {} · {}",
                                pr.number,
                                clean(state),
                                clean(checks),
                                clean(&pr.url)
                            )
                        } else {
                            format!(
                                "Pull request   #{} · {} · {}",
                                pr.number,
                                clean(state),
                                clean(checks)
                            )
                        };
                        write_clipped(buf, area.x, y, max_w, &line, ok_style);
                        y += 1;
                    }
                }
            }

            if (!run.evidence.is_empty() || idea.state == IdeaState::Delivered) && y < bottom {
                let summary = evidence::bundle_summary(&run.evidence);
                let completeness = self.evidence_completeness_label(run);
                let ev_line = format!("Evidence       {}   {}", clean(&summary), completeness);
                write_clipped(buf, area.x, y, max_w, &ev_line, text_style);
                y += 1;
            }
        } else if idea.state == IdeaState::ReadyForDevelopment && y < bottom {
            let banner = self.ready_banner_line(now);
            write_clipped(buf, area.x, y, max_w, &banner, ok_style);
            y += 1;
        }

        let reserve_bottom = 1 + u16::from(!self.notice.is_empty());
        let timeline_bottom = bottom.saturating_sub(reserve_bottom);

        if y < timeline_bottom && (!short || timeline_bottom.saturating_sub(y) >= 2) {
            write_clipped(buf, area.x, y, max_w, "Timeline", header_style);
            y += 1;
            let avail_rows = timeline_bottom.saturating_sub(y) as usize;
            let start_idx = self.events.len().saturating_sub(avail_rows);
            for ev in &self.events[start_idx..] {
                if y >= timeline_bottom {
                    break;
                }
                let when = format!("{} ago", age(now - ev.ts));
                let who = clean(&ev.actor.id);
                let desc = describe_event(ev);
                let row = format!(" {:<8} {:<14} {}", when, truncate_to_width(&who, 14), desc);
                write_clipped(buf, area.x, y, max_w, &row, text_style);
                y += 1;
            }
        }

        if !self.notice.is_empty() && bottom >= 2 {
            let notice_y = bottom.saturating_sub(2);
            write_clipped(
                buf,
                area.x,
                notice_y,
                max_w,
                &clean(&self.notice),
                warn_style,
            );
        }

        if area.height >= 2 {
            let ready_key = display_shortcut(FACTORY_MARK_READY_KEY, &self.platform)
                .unwrap_or_else(|_| FACTORY_MARK_READY_KEY.to_string());
            let action_label = if idea.state == IdeaState::ReadyForDevelopment {
                "revoke"
            } else {
                "mark ready"
            };
            let orch_label = if self.pane_open {
                "orchestrator"
            } else {
                "resume"
            };
            let run_hint = if self.run.is_some() {
                "Enter run   h handoff   e evidence   u audit   r refresh PR status   "
            } else {
                ""
            };
            let hints = if narrow {
                let short_run = if self.run.is_some() { "h/e/r   " } else { "" };
                format!("{ready_key} {action_label}   {short_run}f/p/a read   Esc back")
            } else {
                format!(
                    "{ready_key} {action_label}   {run_hint}f feasibility   p plan   a assumptions   o {orch_label}   Esc back"
                )
            };
            write_clipped(buf, area.x, bottom - 1, max_w, &hints, dim_style);
        }
    }

    fn evidence_completeness_label(&self, run: &Run) -> String {
        let store = FactoryStore::new(&self.workspace);
        let run_wf_path = store.run_dir(&run.id).join("workflow.yaml");
        let raw = if run_wf_path.exists() {
            load_workflow_file(&run_wf_path).ok()
        } else {
            load_default_workflow(&self.workspace)
                .ok()
                .map(|(wf, _)| wf)
        };
        if let Some(raw_wf) = raw {
            let settings = store.settings().unwrap_or_default();
            if let Ok(wf) = validate(&raw_wf, &settings) {
                let missing = evidence::missing(run, &wf);
                if missing.is_empty() {
                    return "complete".to_string();
                }
                let names: Vec<&str> = missing.iter().map(EvidenceKind::as_str).collect();
                return format!("incomplete (missing: {})", names.join(", "));
            }
        }
        "incomplete".to_string()
    }

    fn ready_banner_line(&self, now: f64) -> String {
        for ev in self.events.iter().rev() {
            if let Ok(EventData::IdeaPlanSnapshotFrozen {
                plan_snapshot,
                approved_by,
                via,
            }) = ev.to_event_data()
            {
                let verif = match &self.snapshot_verification {
                    Some(v) if v.verified => "snapshot verified".to_string(),
                    Some(v) => format!(
                        "snapshot unverified: {}",
                        v.error.as_deref().unwrap_or("mismatch")
                    ),
                    None => "snapshot missing".to_string(),
                };
                return format!(
                    "Ready for development · approved by {} via {} ({} ago) · plan v{} ({})",
                    clean(&approved_by),
                    clean(&via),
                    age(now - ev.ts),
                    plan_snapshot.version,
                    verif
                );
            }
        }
        "Ready for development".to_string()
    }
}

fn describe_event(ev: &Event) -> String {
    match ev.to_event_data() {
        Ok(EventData::IdeaCreated { title, .. }) => format!("idea created ({})", clean(&title)),
        Ok(EventData::IdeaOrchestratorBound { cli, model, .. }) => {
            format!("orchestrator bound ({}/{})", clean(&cli), clean(&model))
        }
        Ok(EventData::IdeaDescribed { .. }) => "idea described".to_string(),
        Ok(EventData::ArtifactStored {
            kind,
            artifact,
            verdict,
            ..
        }) => match verdict {
            Some(v) => {
                let vw = match v {
                    FeasibilityVerdict::Feasible => "feasible",
                    FeasibilityVerdict::FeasibleWithRisks => "feasible with risks",
                    FeasibilityVerdict::Infeasible => "infeasible",
                };
                format!("{} v{} stored ({vw})", clean(&kind), artifact.version)
            }
            None => format!("{} v{} stored", clean(&kind), artifact.version),
        },
        Ok(EventData::IdeaDirectionSelected {}) => "direction selected".to_string(),
        Ok(EventData::IdeaAssumptionAdded { text }) => {
            format!("assumption added: {}", clean(&text))
        }
        Ok(EventData::IdeaPlanSnapshotFrozen {
            approved_by, via, ..
        }) => {
            format!(
                "marked ready by {} via {}",
                clean(&approved_by),
                clean(&via)
            )
        }
        Ok(EventData::IdeaReady {}) => "ready for development".to_string(),
        Ok(EventData::IdeaReadyRevoked {}) => "ready revoked".to_string(),
        Ok(EventData::IdeaCancelled { reason }) => format!("cancelled ({})", clean(&reason)),
        _ => clean(&ev.r#type),
    }
}

pub fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let width = max_width.max(1);
    let mut out = Vec::new();
    for raw_line in text.lines() {
        if raw_line.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut current = String::new();
        let mut cur_w = 0usize;
        for word in raw_line.split_inclusive(' ') {
            let w_width = word.width();
            if cur_w + w_width <= width {
                current.push_str(word);
                cur_w += w_width;
            } else if w_width > width {
                if !current.is_empty() {
                    out.push(current.trim_end().to_string());
                    current.clear();
                    cur_w = 0;
                }
                for ch in word.chars() {
                    let cw = ch.to_string().width();
                    if cur_w + cw > width && !current.is_empty() {
                        out.push(current);
                        current = String::new();
                        cur_w = 0;
                    }
                    current.push(ch);
                    cur_w += cw;
                }
            } else {
                out.push(current.trim_end().to_string());
                current = word.to_string();
                cur_w = w_width;
            }
        }
        if !current.is_empty() {
            out.push(current.trim_end().to_string());
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn write_clipped(buf: &mut Buffer, x: u16, y: u16, max_w: usize, text: &str, style: Style) {
    let clipped = truncate_to_width(text, max_w);
    buf.set_stringn(x, y, clipped, max_w, style);
}
