use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;

use crate::factory::broker::gate::{AuthType, consent_provider, today_iso_date};
use crate::factory::broker::job::{JobInput, JobState, job_dir, jobs_dir, read_job_state};
use crate::factory::broker::result::JobResult;
use crate::factory::broker::terms::{PROVIDER_TERMS, TermsVerdict, lookup};
use crate::factory::checks::{CheckKind, CheckProposal, discover_checks, set_checks};
use crate::factory::events::{self, Event};
use crate::factory::evidence::{self, load_review_rounds};
use crate::factory::machine::EventData;
use crate::factory::model::{Actor, ActorKind, Idea, KNOWN_PROVIDERS, Run, RunState, StageState};
use crate::factory::run::get_run;
use crate::factory::runner::{
    cancel_run, pause_run_by_user, resume_paused_run, retry_paused_stage,
};
use crate::factory::stages::review::ReviewVerdict;
use crate::factory::store::FactoryStore;
use crate::factory::workflow::schema::EvidenceKind;
use crate::factory::workspace::{TargetResolution, resolve_target, set_run_target};
use crate::sanitize::clean;
use crate::snapshot::RunSummary;
use crate::status::render::age;
use crate::tmux::keys::{
    FACTORY_CANCEL_RUN_KEY, FACTORY_PAUSE_RUN_KEY, display_shortcut, matches_crossterm_event,
};
use crate::ui::input::TextInput;
use crate::ui::scroll::ScrollView;
use crate::ui::text::truncate_to_width;

pub const LOG_TAIL_BYTES: usize = 65_536;

const AUTH_TYPES: [AuthType; 3] = [AuthType::Subscription, AuthType::ApiKey, AuthType::Cloud];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunKeyOutcome {
    Ignored,
    Consumed,
    BackToDetail,
    Refresh,
}

#[derive(Debug, Clone)]
pub struct StageJobInfo {
    pub input: Option<JobInput>,
    pub result: Option<JobResult>,
    pub job_state: Option<JobState>,
    pub redactions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunSubView {
    Main,
    StageDetail {
        stage_idx: usize,
    },
    LogReader {
        stage_name: String,
        attempt: u32,
        log_path: PathBuf,
        raw_text: String,
        truncated: bool,
        scroll: ScrollView,
    },
    ConfirmCancel {
        input: TextInput,
        error: Option<String>,
    },
    RetryPrompt {
        stage_name: String,
        input: TextInput,
        error: Option<String>,
    },
    ConsentDialog {
        provider_idx: usize,
        providers: Vec<String>,
        auth_idx: usize,
        input: TextInput,
        error: Option<String>,
    },
    TargetPicker {
        choices: Vec<String>,
        selected: usize,
        error: Option<String>,
    },
    EvidenceList {
        selected: usize,
    },
    EvidenceReader {
        title: String,
        raw_text: String,
        scroll: ScrollView,
    },
    ConfirmChecks {
        target: String,
        proposals: Vec<CheckProposal>,
        input: TextInput,
        error: Option<String>,
    },
}

#[derive(Debug, Clone)]
enum PauseResolution {
    TargetAmbiguous {
        choices: Vec<String>,
    },
    ChecksUnconfirmed {
        target: String,
        proposals: Vec<CheckProposal>,
    },
    GateClosed {
        provider: Option<String>,
        consent_possible: bool,
    },
    RetriesOrFailed {
        stage: String,
        rationale: Option<String>,
    },
    UserPaused,
}

#[derive(Debug, Clone)]
pub struct FactoryRun {
    pub workspace: PathBuf,
    pub idea_id: String,
    pub run_id: String,
    pub platform: String,
    pub owner: String,
    pub idea: Option<Idea>,
    pub run: Option<Run>,
    pub events: Vec<Event>,
    pub stage_jobs: Vec<Option<StageJobInfo>>,
    pub exclusions: Vec<crate::factory::data_policy::ExcludedPathEntry>,
    pub running_jobs: usize,
    pub job_limit: usize,
    pub cost_usd: Option<f64>,
    pub cost: crate::factory::telemetry::Cost,
    pub selected_stage: usize,
    pub sub_view: RunSubView,
    pub notice: String,
    pub error: Option<String>,
    pub loaded_updated: Option<f64>,
}

impl FactoryRun {
    pub fn load(workspace: &Path, idea_id: &str, run_id: &str, platform: &str) -> Self {
        let owner = std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .unwrap_or_else(|_| "you".to_string());
        let mut view = Self {
            workspace: workspace.to_path_buf(),
            idea_id: idea_id.to_string(),
            run_id: run_id.to_string(),
            platform: platform.to_string(),
            owner,
            idea: None,
            run: None,
            events: Vec::new(),
            stage_jobs: Vec::new(),
            exclusions: Vec::new(),
            running_jobs: 0,
            job_limit: 3,
            cost_usd: None,
            cost: crate::factory::telemetry::Cost::unknown(),
            selected_stage: 0,
            sub_view: RunSubView::Main,
            notice: String::new(),
            error: None,
            loaded_updated: None,
        };
        view.reload();
        if let Some(ref run) = view.run
            && let Some(idx) = run.stages.iter().position(|s| {
                matches!(
                    s.state,
                    StageState::Running | StageState::Failed | StageState::Blocked
                )
            })
        {
            view.selected_stage = idx;
        }
        view
    }

    pub fn reload(&mut self) {
        let store = FactoryStore::new(&self.workspace);
        self.error = None;
        match store.get_idea(&self.idea_id) {
            Ok(idea) => self.idea = idea,
            Err(err) => self.error = Some(clean(&err.to_string())),
        }
        if let Ok(settings) = store.settings() {
            self.job_limit = settings.max_parallel_agents.max(1);
        }
        match get_run(&store, &self.run_id) {
            Ok(Some(run)) => {
                self.loaded_updated = Some(run.updated);
                let mut stage_jobs = Vec::with_capacity(run.stages.len());
                for st in &run.stages {
                    if st.attempts > 0 {
                        let dir = job_dir(&store, &run.id, &st.name, st.attempts);
                        let input = fs::read_to_string(dir.join("input.json"))
                            .ok()
                            .and_then(|s| serde_json::from_str::<JobInput>(&s).ok());
                        let result = fs::read_to_string(dir.join("result.json"))
                            .ok()
                            .and_then(|s| serde_json::from_str::<JobResult>(&s).ok());
                        let job_state = read_job_state(&dir).ok();
                        let redactions = fs::read_to_string(dir.join("prompt.md"))
                            .map(|text| text.matches("«redacted:").count())
                            .unwrap_or(0);
                        stage_jobs.push(Some(StageJobInfo {
                            input,
                            result,
                            job_state,
                            redactions,
                        }));
                    } else {
                        stage_jobs.push(None);
                    }
                }
                self.stage_jobs = stage_jobs;
                self.exclusions = crate::factory::data_policy::load_exclusions(&store, &run.id);

                let jdir = jobs_dir(&store, &run.id);
                let mut total_cost: Option<f64> = None;
                if jdir.is_dir()
                    && let Ok(entries) = fs::read_dir(&jdir)
                {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if !path.is_dir() {
                            continue;
                        }
                        if let Ok(raw) = fs::read_to_string(path.join("result.json"))
                            && let Ok(res) = serde_json::from_str::<JobResult>(&raw)
                            && let Some(c) = res.usage.and_then(|u| u.cost_usd)
                        {
                            total_cost = Some(total_cost.unwrap_or(0.0) + c);
                        }
                    }
                }
                self.running_jobs = crate::factory::runner::count_workspace_running_jobs(&store);
                self.cost_usd = total_cost;
                if !run.stages.is_empty() && self.selected_stage >= run.stages.len() {
                    self.selected_stage = run.stages.len() - 1;
                }
                self.run = Some(run);
            }
            Ok(None) => self.run = None,
            Err(err) => self.error = Some(clean(&err.to_string())),
        }

        if let Ok(res) = events::read_all(&store.root.join("events.jsonl")) {
            let mut filtered: Vec<Event> = res
                .events
                .into_iter()
                .filter(|ev| ev.run.as_deref() == Some(self.run_id.as_str()))
                .collect();
            let has_usage_events = filtered.iter().any(|ev| {
                matches!(
                    ev.r#type.as_str(),
                    "job.usage" | "telemetry.usage" | "stage.usage"
                )
            });
            if has_usage_events {
                let c = crate::factory::telemetry::total_cost_from_events(&self.run_id, &filtered);
                self.cost_usd = if c.basis == crate::factory::telemetry::CostBasis::Unknown {
                    None
                } else {
                    c.value
                };
                self.cost = c;
            } else {
                self.cost = match self.cost_usd {
                    Some(v) => crate::factory::telemetry::Cost {
                        value: Some(v),
                        basis: crate::factory::telemetry::CostBasis::Actual,
                    },
                    None => crate::factory::telemetry::Cost::unknown(),
                };
            }
            filtered.reverse();
            self.events = filtered;
        }
    }

    pub fn sync_if_changed(&mut self, summary: Option<&RunSummary>) {
        let Some(rs) = summary else {
            return;
        };
        if rs.id != self.run_id {
            self.run_id = rs.id.clone();
            self.reload();
            return;
        }
        let changed = self
            .run
            .as_ref()
            .is_none_or(|r| r.state != rs.state || r.pause_reason != rs.pause_reason)
            || self.running_jobs != rs.running_jobs
            || self.cost_usd != rs.cost_usd;
        if changed {
            self.reload();
        }
    }

    pub fn captures_input(&self) -> bool {
        matches!(
            self.sub_view,
            RunSubView::ConfirmCancel { .. }
                | RunSubView::RetryPrompt { .. }
                | RunSubView::ConsentDialog { .. }
                | RunSubView::TargetPicker { .. }
                | RunSubView::ConfirmChecks { .. }
        )
    }

    fn user_actor(&self) -> Actor {
        Actor {
            kind: ActorKind::User,
            id: self.owner.clone(),
        }
    }

    fn pause_resolution(&self) -> Option<PauseResolution> {
        let run = self.run.as_ref()?;
        if run.state != RunState::Paused {
            return None;
        }
        let reason = run.pause_reason.as_deref().unwrap_or("");
        if reason.contains("target repository")
            || run
                .action_commands
                .iter()
                .any(|c| c.contains("run target "))
        {
            let choices = match resolve_target(&self.workspace) {
                TargetResolution::Ambiguous { choices } => choices,
                TargetResolution::Resolved { name, .. } => vec![name],
            };
            return Some(PauseResolution::TargetAmbiguous { choices });
        }
        if reason.contains("test and lint commands are not confirmed")
            || run
                .action_commands
                .iter()
                .any(|c| c.contains("checks discover"))
        {
            let target = run
                .target
                .clone()
                .unwrap_or_else(|| "workspace".to_string());
            let target_path = match resolve_target(&self.workspace) {
                TargetResolution::Resolved { path, .. } => path,
                TargetResolution::Ambiguous { .. } => self.workspace.clone(),
            };
            let proposals = discover_checks(&target, &target_path, None)
                .map(|r| r.proposals)
                .unwrap_or_default();
            return Some(PauseResolution::ChecksUnconfirmed { target, proposals });
        }
        if reason.contains("gate") || reason.contains("consent") {
            let today = today_iso_date();
            let mentioned = KNOWN_PROVIDERS
                .iter()
                .find(|p| reason.contains(*p))
                .map(|p| (*p).to_string());
            let verdict_note = if reason.contains("unclear") {
                Some("unclear".to_string())
            } else if reason.contains("not-allowed") {
                Some("not-allowed".to_string())
            } else {
                None
            };
            let consent_possible = match mentioned.as_deref() {
                Some(p) if verdict_note.is_some() => {
                    let _ = p;
                    false
                }
                Some(p) => PROVIDER_TERMS.iter().any(|row| {
                    row.provider == p
                        && row.verdict.allows_headless()
                        && today.as_str() <= row.reverify_by
                }),
                None => PROVIDER_TERMS
                    .iter()
                    .any(|row| row.verdict.allows_headless() && today.as_str() <= row.reverify_by),
            };
            return Some(PauseResolution::GateClosed {
                provider: mentioned,
                consent_possible,
            });
        }
        if reason.contains("retries are used up")
            || reason.contains("failed without a new hypothesis")
        {
            let stage = extract_quoted(reason)
                .or_else(|| {
                    run.stages
                        .iter()
                        .find(|s| s.state == StageState::Failed)
                        .map(|s| s.name.clone())
                })
                .unwrap_or_else(|| {
                    run.stages
                        .get(self.selected_stage)
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| "develop".to_string())
                });
            let rationale = run
                .stages
                .iter()
                .position(|s| s.name == stage)
                .and_then(|idx| {
                    self.stage_jobs
                        .get(idx)
                        .and_then(|j| j.as_ref())
                        .and_then(|j| j.result.as_ref())
                        .and_then(|r| r.rationale.clone())
                        .or_else(|| run.stages[idx].rationale.clone())
                });
            return Some(PauseResolution::RetriesOrFailed { stage, rationale });
        }
        Some(PauseResolution::UserPaused)
    }

    pub fn handle_key(&mut self, event: &KeyEvent) -> RunKeyOutcome {
        let actor = self.user_actor();
        let owner = self.owner.clone();
        match &mut self.sub_view {
            RunSubView::StageDetail { .. } => match event.code {
                KeyCode::Esc => {
                    self.sub_view = RunSubView::Main;
                    RunKeyOutcome::Consumed
                }
                KeyCode::Char('l') => {
                    self.open_selected_log();
                    RunKeyOutcome::Consumed
                }
                _ => RunKeyOutcome::Consumed,
            },
            RunSubView::LogReader {
                raw_text,
                scroll,
                log_path,
                truncated,
                ..
            } => {
                let total = raw_text.lines().count().max(1);
                if scroll.handle_key(event.code, total, 18) {
                    return RunKeyOutcome::Consumed;
                }
                match event.code {
                    KeyCode::Char('r') => {
                        let (new_text, new_trunc) = read_log_tail_sanitized(log_path);
                        *raw_text = new_text;
                        *truncated = new_trunc;
                        scroll.clamp(raw_text.lines().count().max(1), 18);
                        RunKeyOutcome::Consumed
                    }
                    KeyCode::Esc => {
                        self.sub_view = RunSubView::Main;
                        RunKeyOutcome::Consumed
                    }
                    _ => RunKeyOutcome::Consumed,
                }
            }
            RunSubView::ConfirmCancel { input, error } => match event.code {
                KeyCode::Esc => {
                    self.sub_view = RunSubView::Main;
                    RunKeyOutcome::Consumed
                }
                KeyCode::Enter => {
                    let typed = input.value.trim().to_string();
                    if typed != self.run_id {
                        *error = Some(format!("Type {} to confirm cancellation.", self.run_id));
                        return RunKeyOutcome::Consumed;
                    }
                    let store = FactoryStore::new(&self.workspace);
                    let env_fn = |k: &str| std::env::var(k).ok();
                    let no_pane = |_: &str| None;
                    let caller = crate::factory::approval::Caller::tui(&env_fn, &no_pane);
                    match cancel_run(&store, &self.run_id, &typed, &actor, &caller) {
                        Ok(_) => {
                            self.sub_view = RunSubView::Main;
                            self.notice = format!("Run {} cancelled.", self.run_id);
                            self.reload();
                            RunKeyOutcome::Refresh
                        }
                        Err(err) => {
                            *error = Some(clean(&err.to_string()));
                            RunKeyOutcome::Consumed
                        }
                    }
                }
                _ => {
                    input.handle_key(event);
                    RunKeyOutcome::Consumed
                }
            },
            RunSubView::RetryPrompt {
                stage_name,
                input,
                error,
            } => match event.code {
                KeyCode::Esc => {
                    self.sub_view = RunSubView::Main;
                    RunKeyOutcome::Consumed
                }
                KeyCode::Enter => {
                    let hyp = input.value.trim().to_string();
                    if hyp.is_empty() {
                        *error = Some("Hypothesis must not be empty.".to_string());
                        return RunKeyOutcome::Consumed;
                    }
                    let store = FactoryStore::new(&self.workspace);
                    let stage = stage_name.clone();
                    let env_fn = |k: &str| std::env::var(k).ok();
                    let no_pane = |_: &str| None;
                    let caller = crate::factory::approval::Caller::tui(&env_fn, &no_pane);
                    match retry_paused_stage(&store, &self.run_id, &stage, &hyp, &actor, &caller) {
                        Ok(_) => {
                            self.sub_view = RunSubView::Main;
                            self.notice = format!("Retrying stage {stage}.");
                            self.reload();
                            RunKeyOutcome::Refresh
                        }
                        Err(err) => {
                            *error = Some(clean(&err.to_string()));
                            RunKeyOutcome::Consumed
                        }
                    }
                }
                _ => {
                    input.handle_key(event);
                    RunKeyOutcome::Consumed
                }
            },
            RunSubView::ConsentDialog {
                provider_idx,
                providers,
                auth_idx,
                input,
                error,
            } => match event.code {
                KeyCode::Esc => {
                    self.sub_view = RunSubView::Main;
                    RunKeyOutcome::Consumed
                }
                KeyCode::Left => {
                    if *auth_idx == 0 {
                        *auth_idx = AUTH_TYPES.len() - 1;
                    } else {
                        *auth_idx -= 1;
                    }
                    *error = None;
                    RunKeyOutcome::Consumed
                }
                KeyCode::Right => {
                    *auth_idx = (*auth_idx + 1) % AUTH_TYPES.len();
                    *error = None;
                    RunKeyOutcome::Consumed
                }
                KeyCode::Up => {
                    if !providers.is_empty() {
                        *provider_idx = if *provider_idx == 0 {
                            providers.len() - 1
                        } else {
                            *provider_idx - 1
                        };
                        *error = None;
                    }
                    RunKeyOutcome::Consumed
                }
                KeyCode::Down => {
                    if !providers.is_empty() {
                        *provider_idx = (*provider_idx + 1) % providers.len();
                        *error = None;
                    }
                    RunKeyOutcome::Consumed
                }
                KeyCode::Enter => {
                    let provider = providers[*provider_idx].clone();
                    let auth = AUTH_TYPES[*auth_idx];
                    let confirm = input.value.trim().to_string();
                    if confirm != provider {
                        *error = Some(format!("Type {provider} to confirm consent."));
                        return RunKeyOutcome::Consumed;
                    }
                    let store = FactoryStore::new(&self.workspace);
                    let env_fn = |k: &str| std::env::var(k).ok();
                    let no_pane = |_: &str| None;
                    let caller = crate::factory::approval::Caller::tui(&env_fn, &no_pane);
                    match consent_provider(&store, &provider, auth, &confirm, &owner, &caller) {
                        Ok(_) => {
                            let _ = resume_paused_run(&store, &self.run_id, &actor);
                            self.sub_view = RunSubView::Main;
                            self.notice = format!("Consented to {provider} ({}).", auth.as_str());
                            self.reload();
                            RunKeyOutcome::Refresh
                        }
                        Err(err) => {
                            *error = Some(clean(&err.to_string()));
                            RunKeyOutcome::Consumed
                        }
                    }
                }
                _ => {
                    input.handle_key(event);
                    RunKeyOutcome::Consumed
                }
            },
            RunSubView::TargetPicker {
                choices,
                selected,
                error,
            } => match event.code {
                KeyCode::Esc => {
                    self.sub_view = RunSubView::Main;
                    RunKeyOutcome::Consumed
                }
                KeyCode::Up => {
                    *selected = selected.saturating_sub(1);
                    RunKeyOutcome::Consumed
                }
                KeyCode::Down => {
                    if !choices.is_empty() {
                        *selected = (*selected + 1).min(choices.len() - 1);
                    }
                    RunKeyOutcome::Consumed
                }
                KeyCode::Enter => {
                    let Some(target) = choices.get(*selected).cloned() else {
                        return RunKeyOutcome::Consumed;
                    };
                    let store = FactoryStore::new(&self.workspace);
                    let env_fn = |k: &str| std::env::var(k).ok();
                    let no_pane = |_: &str| None;
                    let caller = crate::factory::approval::Caller::tui(&env_fn, &no_pane);
                    match set_run_target(&store, &self.workspace, &self.run_id, &target, &caller) {
                        Ok(_) => {
                            let _ = resume_paused_run(&store, &self.run_id, &actor);
                            self.sub_view = RunSubView::Main;
                            self.notice = format!("Target set to {target}.");
                            self.reload();
                            RunKeyOutcome::Refresh
                        }
                        Err(err) => {
                            *error = Some(clean(&err.to_string()));
                            RunKeyOutcome::Consumed
                        }
                    }
                }
                _ => RunKeyOutcome::Consumed,
            },
            RunSubView::EvidenceList { selected } => {
                let count = self.run.as_ref().map(|r| r.evidence.len()).unwrap_or(0);
                match event.code {
                    KeyCode::Esc => {
                        self.sub_view = RunSubView::Main;
                        RunKeyOutcome::Consumed
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        *selected = selected.saturating_sub(1);
                        RunKeyOutcome::Consumed
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if count > 0 {
                            *selected = (*selected + 1).min(count - 1);
                        }
                        RunKeyOutcome::Consumed
                    }
                    KeyCode::Enter => {
                        let idx = *selected;
                        self.open_evidence_item(idx);
                        RunKeyOutcome::Consumed
                    }
                    _ => RunKeyOutcome::Consumed,
                }
            }
            RunSubView::EvidenceReader {
                raw_text, scroll, ..
            } => {
                let total = raw_text.lines().count().max(1);
                if scroll.handle_key(event.code, total, 18) {
                    return RunKeyOutcome::Consumed;
                }
                if event.code == KeyCode::Esc {
                    self.sub_view = RunSubView::EvidenceList { selected: 0 };
                    return RunKeyOutcome::Consumed;
                }
                RunKeyOutcome::Consumed
            }
            RunSubView::ConfirmChecks {
                target,
                proposals,
                input,
                error,
            } => match event.code {
                KeyCode::Esc => {
                    self.sub_view = RunSubView::Main;
                    RunKeyOutcome::Consumed
                }
                KeyCode::Enter => {
                    let typed = input.value.trim().to_string();
                    if typed != *target {
                        *error = Some(format!("Type \"{target}\" to confirm."));
                        return RunKeyOutcome::Consumed;
                    }
                    let test_cmds: Vec<Vec<String>> = proposals
                        .iter()
                        .filter(|p| p.kind == CheckKind::Test)
                        .map(|p| p.argv.clone())
                        .collect();
                    let lint_cmds: Vec<Vec<String>> = proposals
                        .iter()
                        .filter(|p| p.kind == CheckKind::Lint)
                        .map(|p| p.argv.clone())
                        .collect();
                    let store = FactoryStore::new(&self.workspace);
                    let env_fn = |var: &str| std::env::var(var).ok();
                    let no_pane = |_: &str| None;
                    let caller = crate::factory::approval::Caller::tui(&env_fn, &no_pane);
                    match set_checks(
                        &store,
                        target,
                        test_cmds,
                        lint_cmds,
                        None,
                        Vec::new(),
                        &typed,
                        &owner,
                        &caller,
                    ) {
                        Ok(_) => {
                            let _ = resume_paused_run(&store, &self.run_id, &actor);
                            self.notice = format!("Check commands confirmed for {target}.");
                            self.sub_view = RunSubView::Main;
                            self.reload();
                            RunKeyOutcome::Refresh
                        }
                        Err(err) => {
                            *error = Some(clean(&err.to_string()));
                            RunKeyOutcome::Consumed
                        }
                    }
                }
                _ => {
                    input.handle_key(event);
                    *error = None;
                    RunKeyOutcome::Consumed
                }
            },
            RunSubView::Main => {
                if matches_crossterm_event(FACTORY_PAUSE_RUN_KEY, event, &self.platform) {
                    return self.toggle_pause_or_resume();
                }
                if matches_crossterm_event(FACTORY_CANCEL_RUN_KEY, event, &self.platform) {
                    return self.open_cancel_confirm();
                }
                match event.code {
                    KeyCode::Esc => RunKeyOutcome::BackToDetail,
                    KeyCode::Left | KeyCode::Up => {
                        self.selected_stage = self.selected_stage.saturating_sub(1);
                        RunKeyOutcome::Consumed
                    }
                    KeyCode::Right | KeyCode::Down => {
                        if let Some(ref run) = self.run
                            && !run.stages.is_empty()
                        {
                            self.selected_stage =
                                (self.selected_stage + 1).min(run.stages.len() - 1);
                        }
                        RunKeyOutcome::Consumed
                    }
                    KeyCode::Enter => {
                        if self.run.as_ref().is_some_and(|r| !r.stages.is_empty()) {
                            self.sub_view = RunSubView::StageDetail {
                                stage_idx: self.selected_stage,
                            };
                        }
                        RunKeyOutcome::Consumed
                    }
                    KeyCode::Char('l') => {
                        self.open_selected_log();
                        RunKeyOutcome::Consumed
                    }
                    KeyCode::Char('e') => {
                        self.open_evidence_view();
                        RunKeyOutcome::Consumed
                    }
                    KeyCode::Char('k') => {
                        if let Some(PauseResolution::ChecksUnconfirmed { target, proposals }) =
                            self.pause_resolution()
                        {
                            let mut input = TextInput::with_placeholder(&target);
                            input.focused = true;
                            self.sub_view = RunSubView::ConfirmChecks {
                                target,
                                proposals,
                                input,
                                error: None,
                            };
                            RunKeyOutcome::Consumed
                        } else {
                            RunKeyOutcome::Ignored
                        }
                    }
                    KeyCode::Char('r') => self.handle_paused_r(),
                    KeyCode::Char('c') => {
                        if self
                            .run
                            .as_ref()
                            .is_some_and(|r| r.state == RunState::Paused)
                        {
                            self.open_cancel_confirm()
                        } else {
                            RunKeyOutcome::Ignored
                        }
                    }
                    KeyCode::Char('a') => {
                        if matches!(
                            self.pause_resolution(),
                            Some(PauseResolution::GateClosed {
                                consent_possible: true,
                                ..
                            })
                        ) {
                            self.open_consent_dialog();
                            RunKeyOutcome::Consumed
                        } else {
                            RunKeyOutcome::Ignored
                        }
                    }
                    KeyCode::Char('t') => {
                        if let Some(PauseResolution::TargetAmbiguous { choices }) =
                            self.pause_resolution()
                        {
                            self.sub_view = RunSubView::TargetPicker {
                                choices,
                                selected: 0,
                                error: None,
                            };
                            RunKeyOutcome::Consumed
                        } else {
                            RunKeyOutcome::Ignored
                        }
                    }
                    _ => RunKeyOutcome::Ignored,
                }
            }
        }
    }

    fn toggle_pause_or_resume(&mut self) -> RunKeyOutcome {
        let Some(ref run) = self.run else {
            return RunKeyOutcome::Consumed;
        };
        let store = FactoryStore::new(&self.workspace);
        let actor = self.user_actor();
        match run.state {
            RunState::Queued | RunState::Running => {
                let env_fn = |k: &str| std::env::var(k).ok();
                let no_pane = |_: &str| None;
                let caller = crate::factory::approval::Caller::tui(&env_fn, &no_pane);
                match pause_run_by_user(&store, &self.run_id, false, &actor, &caller) {
                    Ok(_) => {
                        self.notice = format!("Run {} paused.", self.run_id);
                        self.reload();
                        RunKeyOutcome::Refresh
                    }
                    Err(err) => {
                        self.notice = clean(&err.to_string());
                        RunKeyOutcome::Consumed
                    }
                }
            }
            RunState::Paused => match resume_paused_run(&store, &self.run_id, &actor) {
                Ok(_) => {
                    self.notice = format!("Run {} resumed.", self.run_id);
                    self.reload();
                    RunKeyOutcome::Refresh
                }
                Err(err) => {
                    self.notice = clean(&err.to_string());
                    RunKeyOutcome::Consumed
                }
            },
            _ => RunKeyOutcome::Consumed,
        }
    }

    fn handle_paused_r(&mut self) -> RunKeyOutcome {
        match self.pause_resolution() {
            Some(PauseResolution::RetriesOrFailed { stage, .. }) => {
                let mut input = TextInput::with_placeholder("new hypothesis");
                input.focused = true;
                self.sub_view = RunSubView::RetryPrompt {
                    stage_name: stage,
                    input,
                    error: None,
                };
                RunKeyOutcome::Consumed
            }
            Some(PauseResolution::UserPaused) => {
                let store = FactoryStore::new(&self.workspace);
                let actor = self.user_actor();
                match resume_paused_run(&store, &self.run_id, &actor) {
                    Ok(_) => {
                        self.notice = format!("Run {} resumed.", self.run_id);
                        self.reload();
                        RunKeyOutcome::Refresh
                    }
                    Err(err) => {
                        self.notice = clean(&err.to_string());
                        RunKeyOutcome::Consumed
                    }
                }
            }
            _ => RunKeyOutcome::Ignored,
        }
    }

    fn open_cancel_confirm(&mut self) -> RunKeyOutcome {
        let can_cancel = self.run.as_ref().is_some_and(|r| {
            matches!(
                r.state,
                RunState::Pending
                    | RunState::Queued
                    | RunState::Running
                    | RunState::Paused
                    | RunState::Review
            )
        });
        if !can_cancel {
            self.notice = "Run cannot be cancelled in its current state.".to_string();
            return RunKeyOutcome::Consumed;
        }
        let mut input = TextInput::with_placeholder(&self.run_id);
        input.focused = true;
        self.sub_view = RunSubView::ConfirmCancel { input, error: None };
        RunKeyOutcome::Consumed
    }

    fn open_consent_dialog(&mut self) {
        let providers: Vec<String> = KNOWN_PROVIDERS.iter().map(|s| (*s).to_string()).collect();
        let default_idx = if let Some(PauseResolution::GateClosed {
            provider: Some(ref p),
            ..
        }) = self.pause_resolution()
        {
            providers.iter().position(|x| x == p).unwrap_or(0)
        } else {
            0
        };
        let mut input = TextInput::with_placeholder(&providers[default_idx]);
        input.focused = true;
        self.sub_view = RunSubView::ConsentDialog {
            provider_idx: default_idx,
            providers,
            auth_idx: 0,
            input,
            error: None,
        };
    }

    fn open_selected_log(&mut self) {
        let Some(ref run) = self.run else {
            return;
        };
        let Some(stage) = run.stages.get(self.selected_stage) else {
            return;
        };
        let store = FactoryStore::new(&self.workspace);
        let attempt = stage.attempts.max(1);
        let log_path = job_dir(&store, &self.run_id, &stage.name, attempt).join("log.txt");
        let (raw_text, truncated) = read_log_tail_sanitized(&log_path);
        self.sub_view = RunSubView::LogReader {
            stage_name: stage.name.clone(),
            attempt,
            log_path,
            raw_text,
            truncated,
            scroll: ScrollView::new(),
        };
    }

    pub fn open_evidence_view(&mut self) {
        self.sub_view = RunSubView::EvidenceList { selected: 0 };
    }

    fn open_evidence_item(&mut self, idx: usize) {
        let Some(ref run) = self.run else {
            return;
        };
        let Some(item) = run.evidence.get(idx) else {
            return;
        };
        let title = format!(
            "{} · {} (attempt {})",
            item.id,
            item.kind.as_str(),
            item.attempt
        );
        if item.kind == EvidenceKind::Screenshot {
            let raw_text = format!(
                "Screenshot: {}\nStage: {} (attempt {})\nPath: {}\nSize: {} bytes\nSHA-256: {}\n\nScreenshots are rendered as thumbnails in tandem-web.",
                clean(&item.summary),
                clean(&item.stage),
                item.attempt,
                clean(&item.path),
                item.bytes,
                item.sha256
            );
            self.sub_view = RunSubView::EvidenceReader {
                title,
                raw_text,
                scroll: ScrollView::new(),
            };
            return;
        }
        let store = FactoryStore::new(&self.workspace);
        let raw_text = match evidence::read_item(&store, &self.run_id, &item.id) {
            Ok(text) => clean(&text),
            Err(err) => clean(&err.to_string()),
        };
        self.sub_view = RunSubView::EvidenceReader {
            title,
            raw_text,
            scroll: ScrollView::new(),
        };
    }

    pub fn render(&mut self, now: f64, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        match &mut self.sub_view {
            RunSubView::StageDetail { stage_idx } => {
                let idx = *stage_idx;
                self.render_stage_detail(idx, area, buf);
            }
            RunSubView::LogReader {
                stage_name,
                attempt,
                raw_text,
                truncated,
                scroll,
                ..
            } => {
                render_log_reader(
                    stage_name, *attempt, raw_text, *truncated, scroll, area, buf,
                );
            }
            RunSubView::ConfirmCancel { input, error } => {
                render_confirm_cancel(&self.run_id, input, error.as_deref(), area, buf);
            }
            RunSubView::RetryPrompt {
                stage_name,
                input,
                error,
            } => {
                render_retry_prompt(stage_name, input, error.as_deref(), area, buf);
            }
            RunSubView::ConsentDialog {
                provider_idx,
                providers,
                auth_idx,
                input,
                error,
            } => {
                render_consent_dialog(
                    &providers[*provider_idx],
                    AUTH_TYPES[*auth_idx],
                    input,
                    error.as_deref(),
                    area,
                    buf,
                );
            }
            RunSubView::TargetPicker {
                choices,
                selected,
                error,
            } => {
                render_target_picker(choices, *selected, error.as_deref(), area, buf);
            }
            RunSubView::EvidenceList { selected } => {
                let sel = *selected;
                self.render_evidence_list(sel, area, buf);
            }
            RunSubView::EvidenceReader {
                title,
                raw_text,
                scroll,
            } => {
                render_evidence_reader(title, raw_text, scroll, area, buf);
            }
            RunSubView::ConfirmChecks {
                target,
                proposals,
                input,
                error,
            } => {
                render_confirm_checks(target, proposals, input, error.as_deref(), area, buf);
            }
            RunSubView::Main => {
                self.render_main(now, area, buf);
            }
        }
    }

    fn render_main(&self, now: f64, area: Rect, buf: &mut Buffer) {
        let max_w = area.width as usize;
        let narrow = area.width < 80;
        let short = area.height <= 8;
        let merge_agents_into_header = area.height <= 6;

        let header_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        let text_style = Style::default().fg(Color::Gray);
        let dim_style = Style::default().fg(Color::DarkGray);
        let warn_style = Style::default().fg(Color::Yellow);
        let err_style = Style::default().fg(Color::Red);

        let Some(ref run) = self.run else {
            let msg = self.error.as_deref().unwrap_or("Run not found.");
            write_clipped(buf, area.x, area.y, max_w, msg, err_style);
            return;
        };

        let title = self
            .idea
            .as_ref()
            .map(|i| clean(&i.title))
            .unwrap_or_else(|| self.idea_id.clone());
        let state_label = run_state_label(run.state);

        let start_ts = run
            .stages
            .iter()
            .filter_map(|s| s.started)
            .fold(None, |acc: Option<f64>, t| {
                Some(acc.map_or(t, |a| a.min(t)))
            })
            .unwrap_or(run.created);
        let end_ts = if matches!(
            run.state,
            RunState::Queued | RunState::Running | RunState::Paused
        ) && now > 0.0
        {
            now
        } else {
            run.updated
        };
        let elapsed = age((end_ts - start_ts).max(0.0));
        let cost = self.cost.format();

        let mut y = area.y;
        let bottom = area.y + area.height;

        let agents_text = format!("Agents {} of {}", self.running_jobs, self.job_limit);
        let hdr = if merge_agents_into_header {
            format!(
                "◀ Idea   Run {}  {}   {}   elapsed {}   cost {}   {}",
                run.id,
                truncate_to_width(&title, 24),
                state_label,
                elapsed,
                cost,
                agents_text
            )
        } else {
            format!(
                "◀ Idea   Run {}  {}   {}   elapsed {}   cost {}",
                run.id,
                truncate_to_width(&title, if narrow { 18 } else { 32 }),
                state_label,
                elapsed,
                cost
            )
        };
        write_clipped(buf, area.x, y, max_w, &hdr, header_style);
        y += 1;

        if let Some(resolution) = self.pause_resolution()
            && y < bottom.saturating_sub(1)
        {
            let reason_raw = run.pause_reason.as_deref().unwrap_or("paused");
            let reason_line = format!("! Paused: {}", clean(reason_raw));
            write_clipped(buf, area.x, y, max_w, &reason_line, warn_style);
            y += 1;

            if y < bottom.saturating_sub(1) {
                let action_line = match resolution {
                    PauseResolution::TargetAmbiguous { .. } => {
                        "  t choose target   c cancel".to_string()
                    }
                    PauseResolution::ChecksUnconfirmed {
                        ref target,
                        ref proposals,
                    } => {
                        let summary = if proposals.is_empty() {
                            "no commands discovered".to_string()
                        } else {
                            proposals
                                .iter()
                                .take(2)
                                .map(|p| p.argv.join(" "))
                                .collect::<Vec<_>>()
                                .join(", ")
                        };
                        if !short && y + 1 < bottom.saturating_sub(1) {
                            write_clipped(
                                buf,
                                area.x,
                                y,
                                max_w,
                                &format!("  Proposed checks for {target}: {summary}"),
                                dim_style,
                            );
                            y += 1;
                        }
                        "  k confirm check commands   c cancel".to_string()
                    }
                    PauseResolution::GateClosed {
                        consent_possible: true,
                        ..
                    } => "  a allow provider (consent)   c cancel".to_string(),
                    PauseResolution::GateClosed {
                        consent_possible: false,
                        ..
                    } => "  Choose another provider or cancel (c cancel).".to_string(),
                    PauseResolution::RetriesOrFailed { rationale, .. } => {
                        if let Some(rat) = rationale
                            && !short
                            && y + 1 < bottom.saturating_sub(1)
                        {
                            write_clipped(
                                buf,
                                area.x,
                                y,
                                max_w,
                                &format!("  Last rationale: {}", clean(&rat)),
                                dim_style,
                            );
                            y += 1;
                        }
                        "  r retry with a new hypothesis   c cancel".to_string()
                    }
                    PauseResolution::UserPaused => "  r resume   c cancel".to_string(),
                };
                write_clipped(buf, area.x, y, max_w, &action_line, text_style);
                y += 1;
            }
        }

        if y < bottom.saturating_sub(1) {
            if narrow {
                for (idx, st) in run.stages.iter().enumerate() {
                    if y >= bottom.saturating_sub(1) {
                        break;
                    }
                    let marker = stage_marker(st.state);
                    let pointer = if idx == self.selected_stage {
                        "▸"
                    } else {
                        " "
                    };
                    let extra = self.stage_running_detail(idx, st);
                    let line = if extra.is_empty() {
                        format!("{pointer} {} {marker}", st.name)
                    } else {
                        format!("{pointer} {} {marker} {extra}", st.name)
                    };
                    let st_style = if idx == self.selected_stage {
                        header_style
                    } else {
                        text_style
                    };
                    write_clipped(buf, area.x, y, max_w, &line, st_style);
                    y += 1;
                }
            } else {
                let mut strip = String::new();
                for (idx, st) in run.stages.iter().enumerate() {
                    if idx > 0 {
                        strip.push_str("   ");
                    }
                    let marker = stage_marker(st.state);
                    let extra = self.stage_running_detail(idx, st);
                    let item = if extra.is_empty() {
                        format!("{} {marker}", st.name)
                    } else {
                        format!("{} {marker} {extra}", st.name)
                    };
                    if idx == self.selected_stage {
                        strip.push_str(&format!("[{item}]"));
                    } else {
                        strip.push_str(&item);
                    }
                }
                write_clipped(buf, area.x, y, max_w, &strip, text_style);
                y += 1;
            }
        }

        if !merge_agents_into_header && y < bottom.saturating_sub(1) {
            write_clipped(buf, area.x, y, max_w, &agents_text, text_style);
            y += 1;
        }

        let reserve_bottom = 1 + u16::from(!self.notice.is_empty());
        let activity_bottom = bottom.saturating_sub(reserve_bottom);
        self.render_data_section(
            self.selected_stage,
            &mut y,
            activity_bottom,
            area,
            narrow,
            buf,
        );
        for ev in &self.events {
            if y >= activity_bottom {
                break;
            }
            let row = format_activity_row(ev, narrow);
            write_clipped(buf, area.x, y, max_w, &row, text_style);
            y += 1;
        }

        if !self.notice.is_empty() && bottom >= 2 {
            write_clipped(
                buf,
                area.x,
                bottom - 2,
                max_w,
                &clean(&self.notice),
                warn_style,
            );
        }

        if area.height >= 2 {
            let pause_key = display_shortcut(FACTORY_PAUSE_RUN_KEY, &self.platform)
                .unwrap_or_else(|_| FACTORY_PAUSE_RUN_KEY.to_string());
            let cancel_key = display_shortcut(FACTORY_CANCEL_RUN_KEY, &self.platform)
                .unwrap_or_else(|_| FACTORY_CANCEL_RUN_KEY.to_string());
            let pause_word = if run.state == RunState::Paused {
                "resume"
            } else {
                "pause"
            };
            let hints = if narrow {
                format!(
                    "{pause_key} {pause_word}  {cancel_key} cancel  l log  e evidence  Esc back"
                )
            } else {
                format!(
                    "{pause_key} {pause_word}   {cancel_key} cancel   Enter stage   l log   e evidence   Esc back"
                )
            };
            write_clipped(buf, area.x, bottom - 1, max_w, &hints, dim_style);
        }
    }

    fn stage_running_detail(&self, idx: usize, st: &crate::factory::model::Stage) -> String {
        if st.state != StageState::Running {
            return String::new();
        }
        let info = self.stage_jobs.get(idx).and_then(|o| o.as_ref());
        let provider_model = info
            .and_then(|j| j.input.as_ref())
            .map(|inp| format!("{} / {}", clean(&inp.provider), clean(&inp.model)));
        match provider_model {
            Some(pm) if st.attempts > 0 => format!("{pm} · attempt {}", st.attempts),
            Some(pm) => pm,
            None if st.attempts > 0 => format!("attempt {}", st.attempts),
            None => String::new(),
        }
    }

    fn render_stage_detail(&self, stage_idx: usize, area: Rect, buf: &mut Buffer) {
        let max_w = area.width as usize;
        let header_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        let text_style = Style::default().fg(Color::Gray);
        let dim_style = Style::default().fg(Color::DarkGray);
        let err_style = Style::default().fg(Color::Red);

        let Some(ref run) = self.run else {
            return;
        };
        let Some(st) = run.stages.get(stage_idx) else {
            return;
        };
        let job = self.stage_jobs.get(stage_idx).and_then(|o| o.as_ref());

        let mut y = area.y;
        let bottom = area.y + area.height;
        write_clipped(
            buf,
            area.x,
            y,
            max_w,
            &format!(
                "Stage: {}   state: {}   attempt: {}",
                st.name,
                stage_word(st.state),
                st.attempts
            ),
            header_style,
        );
        y += 1;

        if let Some(inp) = job.and_then(|j| j.input.as_ref())
            && y < bottom.saturating_sub(1)
        {
            let route_line = format!(
                "Route: {} · {} / {} · tools: {}",
                clean(&inp.agent),
                clean(&inp.provider),
                clean(&inp.model),
                inp.tool_grants.join(", ")
            );
            write_clipped(buf, area.x, y, max_w, &route_line, text_style);
            y += 1;
        } else if y < bottom.saturating_sub(1) {
            write_clipped(
                buf,
                area.x,
                y,
                max_w,
                &format!("Route: {}", clean(&st.agent)),
                text_style,
            );
            y += 1;
        }

        if let Some(res) = job.and_then(|j| j.result.as_ref()) {
            if !res.summary.is_empty() && y < bottom.saturating_sub(1) {
                write_clipped(
                    buf,
                    area.x,
                    y,
                    max_w,
                    &format!("Summary: {}", clean(&res.summary)),
                    text_style,
                );
                y += 1;
            }
            if let Some(ref rat) = res.rationale
                && y < bottom.saturating_sub(1)
            {
                write_clipped(
                    buf,
                    area.x,
                    y,
                    max_w,
                    &format!("Rationale: {}", clean(rat)),
                    err_style,
                );
                y += 1;
            }
            if let Some(ref hyp) = res.hypothesis
                && y < bottom.saturating_sub(1)
            {
                write_clipped(
                    buf,
                    area.x,
                    y,
                    max_w,
                    &format!("Hypothesis: {}", clean(hyp)),
                    text_style,
                );
                y += 1;
            }
        } else if let Some(ref rat) = st.rationale
            && y < bottom.saturating_sub(1)
        {
            write_clipped(
                buf,
                area.x,
                y,
                max_w,
                &format!("Rationale: {}", clean(rat)),
                err_style,
            );
            y += 1;
        }

        self.render_data_section(
            stage_idx,
            &mut y,
            bottom.saturating_sub(1),
            area,
            area.width < 80,
            buf,
        );

        if area.height >= 2 {
            write_clipped(
                buf,
                area.x,
                bottom - 1,
                max_w,
                "l log   Esc back",
                dim_style,
            );
        }
    }

    fn render_data_section(
        &self,
        stage_idx: usize,
        y: &mut u16,
        limit_y: u16,
        area: Rect,
        narrow: bool,
        buf: &mut Buffer,
    ) {
        let Some(job) = self.stage_jobs.get(stage_idx).and_then(|o| o.as_ref()) else {
            return;
        };
        let Some(ref inp) = job.input else {
            return;
        };
        if *y >= limit_y {
            return;
        }

        let max_w = area.width as usize;
        let header_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        let text_style = Style::default().fg(Color::Gray);
        let dim_style = Style::default().fg(Color::DarkGray);

        let header = format!(
            "Data · {} attempt {} · {} / {}",
            clean(&inp.stage),
            inp.attempt,
            clean(&inp.provider),
            clean(&inp.model)
        );
        write_clipped(buf, area.x, *y, max_w, &header, header_style);
        *y += 1;
        if *y >= limit_y {
            return;
        }

        let sent_line = if narrow {
            format!(
                "Sent: {} files · {} · {} redactions",
                inp.context_manifest.items.len(),
                crate::factory::data_policy::format_bytes(inp.context_manifest.total_bytes),
                job.redactions
            )
        } else {
            format!(
                "Sent by tandem     {} files · {} · {} redactions",
                inp.context_manifest.items.len(),
                crate::factory::data_policy::format_bytes(inp.context_manifest.total_bytes),
                job.redactions
            )
        };
        write_clipped(buf, area.x, *y, max_w, &sent_line, text_style);
        *y += 1;
        if *y >= limit_y {
            return;
        }

        let scope = inp
            .context_manifest
            .readable_scope
            .clone()
            .unwrap_or_else(|| {
                crate::factory::data_policy::format_readable_scope(
                    self.run.as_ref().and_then(|r| r.worktree_path.as_deref()),
                    &inp.cwd,
                )
            });
        let tools = if inp.tool_grants.is_empty() {
            "none".to_string()
        } else {
            inp.tool_grants.join(", ")
        };
        let readable_line = if narrow {
            format!("Readable: {scope} with {tools}")
        } else {
            format!("Readable by job    {scope} with {tools}")
        };
        write_clipped(buf, area.x, *y, max_w, &readable_line, text_style);
        *y += 1;
        if *y >= limit_y {
            return;
        }

        let excluded: Vec<String> = if !inp.context_manifest.excluded_paths.is_empty() {
            inp.context_manifest.excluded_paths.clone()
        } else {
            self.exclusions.iter().map(|e| e.path.clone()).collect()
        };
        let excluded_summary = if excluded.is_empty() {
            "none".to_string()
        } else {
            format!("{} paths: {}", excluded.len(), excluded.join(", "))
        };
        let excluded_line = if narrow {
            format!("Excluded: {excluded_summary}")
        } else {
            format!("Excluded           {excluded_summary}")
        };
        write_clipped(buf, area.x, *y, max_w, &excluded_line, text_style);
        *y += 1;
        if *y >= limit_y {
            return;
        }

        write_clipped(
            buf,
            area.x,
            *y,
            max_w,
            crate::factory::data_policy::PROVIDER_LIMITATION_SENTENCE,
            dim_style,
        );
        *y += 1;
    }
}

fn render_log_reader(
    stage_name: &str,
    attempt: u32,
    raw_text: &str,
    truncated: bool,
    scroll: &mut ScrollView,
    area: Rect,
    buf: &mut Buffer,
) {
    let max_w = area.width as usize;
    let header_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let dim_style = Style::default().fg(Color::DarkGray);
    let tail_note = if truncated { " (last 64 KiB)" } else { "" };
    write_clipped(
        buf,
        area.x,
        area.y,
        max_w,
        &format!("Log: {stage_name} attempt {attempt}{tail_note}"),
        header_style,
    );
    if area.height <= 2 {
        return;
    }
    let body = Rect::new(area.x, area.y + 1, area.width, area.height - 2);
    let lines: Vec<Line> = raw_text
        .lines()
        .map(|l| Line::from(l.to_string()))
        .collect();
    scroll.render_lines(&lines, body, buf);
    write_clipped(
        buf,
        area.x,
        area.y + area.height - 1,
        max_w,
        "↑↓/PgUp/PgDn scroll   r refresh   Esc back",
        dim_style,
    );
}

fn render_confirm_cancel(
    run_id: &str,
    input: &TextInput,
    error: Option<&str>,
    area: Rect,
    buf: &mut Buffer,
) {
    let max_w = area.width as usize;
    let header_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let text_style = Style::default().fg(Color::Gray);
    let err_style = Style::default().fg(Color::Red);
    let dim_style = Style::default().fg(Color::DarkGray);
    let mut y = area.y;
    let bottom = area.y + area.height;

    write_clipped(
        buf,
        area.x,
        y,
        max_w,
        &format!("Cancel run {run_id}"),
        header_style,
    );
    y += 1;
    if y < bottom {
        write_clipped(
            buf,
            area.x,
            y,
            max_w,
            &format!("Type the run id ({run_id}) to confirm: {}", input.value),
            text_style,
        );
        y += 1;
    }
    if let Some(err) = error
        && y < bottom
    {
        write_clipped(buf, area.x, y, max_w, err, err_style);
    }
    if area.height >= 2 {
        write_clipped(
            buf,
            area.x,
            bottom - 1,
            max_w,
            "Enter confirm   Esc back",
            dim_style,
        );
    }
}

fn render_retry_prompt(
    stage_name: &str,
    input: &TextInput,
    error: Option<&str>,
    area: Rect,
    buf: &mut Buffer,
) {
    let max_w = area.width as usize;
    let header_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let text_style = Style::default().fg(Color::Gray);
    let err_style = Style::default().fg(Color::Red);
    let dim_style = Style::default().fg(Color::DarkGray);
    let mut y = area.y;
    let bottom = area.y + area.height;

    write_clipped(
        buf,
        area.x,
        y,
        max_w,
        &format!("Retry stage {stage_name}"),
        header_style,
    );
    y += 1;
    if y < bottom {
        write_clipped(
            buf,
            area.x,
            y,
            max_w,
            &format!("Hypothesis: {}", input.value),
            text_style,
        );
        y += 1;
    }
    if let Some(err) = error
        && y < bottom
    {
        write_clipped(buf, area.x, y, max_w, err, err_style);
    }
    if area.height >= 2 {
        write_clipped(
            buf,
            area.x,
            bottom - 1,
            max_w,
            "Enter retry   Esc back",
            dim_style,
        );
    }
}

fn render_consent_dialog(
    provider: &str,
    auth: AuthType,
    input: &TextInput,
    error: Option<&str>,
    area: Rect,
    buf: &mut Buffer,
) {
    let max_w = area.width as usize;
    let header_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let text_style = Style::default().fg(Color::Gray);
    let warn_style = Style::default().fg(Color::Yellow);
    let err_style = Style::default().fg(Color::Red);
    let dim_style = Style::default().fg(Color::DarkGray);
    let mut y = area.y;
    let bottom = area.y + area.height;

    write_clipped(
        buf,
        area.x,
        y,
        max_w,
        &format!("Factory Mode will run {provider} in headless mode for this workspace."),
        header_style,
    );
    y += 1;

    if y < bottom {
        if let Some(row) = lookup(provider, auth) {
            let verdict_words = match row.verdict {
                TermsVerdict::Allowed => "allowed",
                TermsVerdict::AllowedWithConditions => "allowed with conditions",
                TermsVerdict::NotAllowed => "not allowed",
                TermsVerdict::Unclear => "unclear",
            };
            write_clipped(
                buf,
                area.x,
                y,
                max_w,
                &format!(
                    "Terms record: {verdict_words} (verified {}, re-verify by {}).",
                    row.verified_on, row.reverify_by
                ),
                if row.verdict.allows_headless() {
                    text_style
                } else {
                    warn_style
                },
            );
        } else {
            write_clipped(
                buf,
                area.x,
                y,
                max_w,
                "Terms record: no verified terms row.",
                warn_style,
            );
        }
        y += 1;
    }

    if y < bottom {
        write_clipped(
            buf,
            area.x,
            y,
            max_w,
            &format!("It uses your own {provider} login. tandem handles no credentials."),
            text_style,
        );
        y += 1;
    }

    if y < bottom {
        let mut auth_line = String::from("Authentication  ");
        for a in AUTH_TYPES {
            if a == auth {
                auth_line.push_str(&format!(" ▸ {}", a.as_str()));
            } else {
                auth_line.push_str(&format!("   {}", a.as_str()));
            }
        }
        write_clipped(buf, area.x, y, max_w, &auth_line, text_style);
        y += 1;
    }

    if y < bottom {
        write_clipped(
            buf,
            area.x,
            y,
            max_w,
            &format!("Type the provider name to allow: {}", input.value),
            text_style,
        );
        y += 1;
    }

    if let Some(err) = error
        && y < bottom
    {
        write_clipped(buf, area.x, y, max_w, err, err_style);
    }

    if area.height >= 2 {
        write_clipped(
            buf,
            area.x,
            bottom - 1,
            max_w,
            "↑↓ provider   ←→ auth   Enter allow   Esc back",
            dim_style,
        );
    }
}

fn render_target_picker(
    choices: &[String],
    selected: usize,
    error: Option<&str>,
    area: Rect,
    buf: &mut Buffer,
) {
    let max_w = area.width as usize;
    let header_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let text_style = Style::default().fg(Color::Gray);
    let err_style = Style::default().fg(Color::Red);
    let dim_style = Style::default().fg(Color::DarkGray);
    let mut y = area.y;
    let bottom = area.y + area.height;

    write_clipped(
        buf,
        area.x,
        y,
        max_w,
        "Choose a target repository",
        header_style,
    );
    y += 1;

    for (idx, choice) in choices.iter().enumerate() {
        if y >= bottom.saturating_sub(1) {
            break;
        }
        let prefix = if idx == selected { "▸ " } else { "  " };
        let style = if idx == selected {
            header_style
        } else {
            text_style
        };
        write_clipped(
            buf,
            area.x,
            y,
            max_w,
            &format!("{prefix}{}", clean(choice)),
            style,
        );
        y += 1;
    }

    if let Some(err) = error
        && y < bottom.saturating_sub(1)
    {
        write_clipped(buf, area.x, y, max_w, err, err_style);
    }

    if area.height >= 2 {
        write_clipped(
            buf,
            area.x,
            bottom - 1,
            max_w,
            "↑↓ choose   Enter select   Esc back",
            dim_style,
        );
    }
}

pub fn read_log_tail_sanitized(path: &Path) -> (String, bool) {
    if events::reject_if_symlink(path).is_err() {
        return ("No log output yet.".to_string(), false);
    }
    let Ok(mut file) = File::open(path) else {
        return ("No log output yet.".to_string(), false);
    };
    let Ok(meta) = file.metadata() else {
        return ("No log output yet.".to_string(), false);
    };
    let len = meta.len();
    if len == 0 {
        return ("No log output yet.".to_string(), false);
    }
    let truncated = len > LOG_TAIL_BYTES as u64;
    if truncated && file.seek(SeekFrom::End(-(LOG_TAIL_BYTES as i64))).is_err() {
        return ("No log output yet.".to_string(), false);
    }
    let mut buf = Vec::with_capacity(LOG_TAIL_BYTES.min(len as usize));
    if file.read_to_end(&mut buf).is_err() {
        return ("No log output yet.".to_string(), false);
    }
    let raw = String::from_utf8_lossy(&buf);
    let aligned = if truncated {
        raw.trim_start_matches('\u{FFFD}')
    } else {
        raw.as_ref()
    };
    let cleaned = clean(aligned);
    if cleaned.trim().is_empty() {
        return ("No log output yet.".to_string(), truncated);
    }
    (cleaned, truncated)
}

fn format_activity_row(ev: &Event, narrow: bool) -> String {
    let hhmm = format_hhmm(ev.ts);
    let (stage, state, detail) = match ev.to_event_data() {
        Ok(EventData::RunCreated { workflow, .. }) => {
            ("run".to_string(), "created".to_string(), clean(&workflow))
        }
        Ok(EventData::RunStarted { .. }) => {
            ("run".to_string(), "started".to_string(), String::new())
        }
        Ok(EventData::StageStarted { stage, attempt, .. }) => (
            clean(&stage),
            "started".to_string(),
            format!("attempt {attempt}"),
        ),
        Ok(EventData::StageSucceeded { stage, .. }) => {
            (clean(&stage), "succeeded".to_string(), String::new())
        }
        Ok(EventData::StageFailed { stage, reason, .. }) => {
            (clean(&stage), "failed".to_string(), clean(&reason))
        }
        Ok(EventData::StageSkipped { stage, reason, .. }) => {
            (clean(&stage), "skipped".to_string(), clean(&reason))
        }
        Ok(EventData::StageBlocked { stage, reason, .. }) => {
            (clean(&stage), "blocked".to_string(), clean(&reason))
        }
        Ok(EventData::StageInterrupted { stage, .. }) => {
            (clean(&stage), "interrupted".to_string(), String::new())
        }
        Ok(EventData::RunPaused { reason, .. }) => {
            ("run".to_string(), "paused".to_string(), clean(&reason))
        }
        Ok(EventData::RunResumed { .. }) => {
            ("run".to_string(), "resumed".to_string(), String::new())
        }
        Ok(EventData::StageRetryRequested {
            stage, hypothesis, ..
        }) => (clean(&stage), "retry".to_string(), clean(&hypothesis)),
        Ok(EventData::RunCancelled { .. }) => {
            ("run".to_string(), "cancelled".to_string(), String::new())
        }
        Ok(EventData::RunTargetSelected { target, .. }) => {
            ("target".to_string(), "selected".to_string(), clean(&target))
        }
        _ => ("run".to_string(), clean(&ev.r#type), String::new()),
    };

    if narrow || detail.is_empty() {
        format!("{hhmm}  {stage:<12}  {state}")
    } else {
        format!("{hhmm}  {stage:<12}  {state:<10}  {detail}")
    }
}

fn format_hhmm(ts: f64) -> String {
    let secs = ts.max(0.0) as i64;
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|dt| dt.format("%H:%M").to_string())
        .unwrap_or_else(|| "00:00".to_string())
}

fn extract_quoted(s: &str) -> Option<String> {
    let start = s.find('"')? + 1;
    let rest = &s[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

impl FactoryRun {
    fn render_evidence_list(&self, selected: usize, area: Rect, buf: &mut Buffer) {
        let max_w = area.width as usize;
        let narrow = area.width < 80;
        let header_style = Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
        let text_style = Style::default().fg(Color::Gray);
        let dim_style = Style::default().fg(Color::DarkGray);
        let ok_style = Style::default().fg(Color::Green);

        let mut y = area.y;
        let bottom = area.y + area.height;

        write_clipped(
            buf,
            area.x,
            y,
            max_w,
            &format!("Evidence for {}", self.run_id),
            header_style,
        );
        y += 1;

        let Some(ref run) = self.run else {
            return;
        };

        if run.evidence.is_empty() && y < bottom.saturating_sub(1) {
            write_clipped(
                buf,
                area.x,
                y,
                max_w,
                "  No evidence recorded yet.",
                dim_style,
            );
            y += 1;
        } else {
            for (idx, item) in run.evidence.iter().enumerate() {
                if y >= bottom.saturating_sub(1) {
                    break;
                }
                let pointer = if idx == selected { "▸" } else { " " };
                let row = if narrow {
                    format!(
                        "{pointer} {:<10} a{} {}",
                        clean(&item.stage),
                        item.attempt,
                        item.kind.as_str()
                    )
                } else {
                    format!(
                        "{pointer} {:<10} attempt {}   {:<15}   {}",
                        clean(&item.stage),
                        item.attempt,
                        item.kind.as_str(),
                        clean(&item.summary)
                    )
                };
                let style = if idx == selected {
                    header_style
                } else {
                    text_style
                };
                write_clipped(buf, area.x, y, max_w, &row, style);
                y += 1;
            }
        }

        let store = FactoryStore::new(&self.workspace);
        let rounds = load_review_rounds(&store, &self.run_id);
        for rd in &rounds.rounds {
            if y >= bottom.saturating_sub(1) {
                break;
            }
            let verdict = match rd.stated_verdict {
                ReviewVerdict::Approve => "approved",
                ReviewVerdict::ChangesRequested => "changes requested",
            };
            let open_count = rd.findings.len();
            let fixed_count = rd.fixed_from_previous.len();
            let line = format!(
                "  review   round {}   {} open, {} fixed   ({verdict})",
                rd.round, open_count, fixed_count
            );
            write_clipped(buf, area.x, y, max_w, &line, ok_style);
            y += 1;

            for f in &rd.findings {
                if y >= bottom.saturating_sub(1) {
                    break;
                }
                let fline = format!(
                    "    [open] {:?} {}:{} {}",
                    f.severity,
                    clean(&f.file),
                    f.line,
                    clean(&f.title)
                );
                write_clipped(buf, area.x, y, max_w, &fline, text_style);
                y += 1;
            }
        }

        let has_screenshots = run
            .evidence
            .iter()
            .any(|i| i.kind == EvidenceKind::Screenshot);
        if !has_screenshots && y < bottom.saturating_sub(1) {
            write_clipped(
                buf,
                area.x,
                y,
                max_w,
                "  demo     no screenshots (not a UI change or skipped)",
                dim_style,
            );
        }

        if area.height >= 2 {
            write_clipped(
                buf,
                area.x,
                bottom - 1,
                max_w,
                "↑/↓ select   Enter open item   Esc back",
                dim_style,
            );
        }
    }
}

fn render_evidence_reader(
    title: &str,
    raw_text: &str,
    scroll: &mut ScrollView,
    area: Rect,
    buf: &mut Buffer,
) {
    let max_w = area.width as usize;
    let header_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD);
    let text_style = Style::default().fg(Color::Gray);
    let dim_style = Style::default().fg(Color::DarkGray);

    write_clipped(buf, area.x, area.y, max_w, &clean(title), header_style);
    let body_rows = area.height.saturating_sub(2) as usize;
    let lines: Vec<&str> = raw_text.lines().collect();
    scroll.clamp(lines.len().max(1), body_rows.max(1));
    for row in 0..body_rows {
        let idx = scroll.offset + row;
        if let Some(line) = lines.get(idx) {
            write_clipped(
                buf,
                area.x,
                area.y + 1 + row as u16,
                max_w,
                line,
                text_style,
            );
        }
    }
    if area.height >= 2 {
        write_clipped(
            buf,
            area.x,
            area.y + area.height - 1,
            max_w,
            "↑/↓ scroll   Esc back",
            dim_style,
        );
    }
}

fn render_confirm_checks(
    target: &str,
    proposals: &[CheckProposal],
    input: &TextInput,
    error: Option<&str>,
    area: Rect,
    buf: &mut Buffer,
) {
    let max_w = area.width as usize;
    let header_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let text_style = Style::default().fg(Color::White);
    let dim_style = Style::default().fg(Color::DarkGray);
    let err_style = Style::default().fg(Color::Red);

    let mut y = area.y;
    let bottom = area.y + area.height;
    write_clipped(
        buf,
        area.x,
        y,
        max_w,
        &format!("Confirm check commands for target \"{target}\""),
        header_style,
    );
    y += 1;

    if proposals.is_empty() && y < bottom.saturating_sub(2) {
        write_clipped(
            buf,
            area.x,
            y,
            max_w,
            "  No check commands discovered.",
            dim_style,
        );
        y += 1;
    } else {
        for prop in proposals {
            if y >= bottom.saturating_sub(2) {
                break;
            }
            let kind_word = match prop.kind {
                CheckKind::Test => "test",
                CheckKind::Lint => "lint",
                CheckKind::Deploy => "deploy",
            };
            write_clipped(
                buf,
                area.x,
                y,
                max_w,
                &format!("  [{kind_word}] {}", prop.argv.join(" ")),
                text_style,
            );
            y += 1;
        }
    }

    if y < bottom.saturating_sub(1) {
        write_clipped(
            buf,
            area.x,
            y,
            max_w,
            &format!(
                "Type the target name ({target}) to confirm: {}_",
                clean(&input.value)
            ),
            text_style,
        );
        y += 1;
    }
    if let Some(err) = error
        && y < bottom.saturating_sub(1)
    {
        write_clipped(buf, area.x, y, max_w, err, err_style);
    }
    if area.height >= 2 {
        write_clipped(
            buf,
            area.x,
            bottom - 1,
            max_w,
            "Enter confirm   Esc cancel",
            dim_style,
        );
    }
}

fn run_state_label(state: RunState) -> &'static str {
    match state {
        RunState::Pending => "Pending",
        RunState::Queued => "Queued",
        RunState::Running => "In development",
        RunState::Paused => "Paused",
        RunState::Review => "Review",
        RunState::Succeeded => "Succeeded",
        RunState::Delivered => "Delivered",
        RunState::Failed => "Failed",
        RunState::Cancelled => "Cancelled",
        RunState::Withdrawn => "Withdrawn",
    }
}

fn stage_marker(state: StageState) -> &'static str {
    match state {
        StageState::Pending => "○",
        StageState::Running => "●",
        StageState::Succeeded => "✓",
        StageState::Failed => "✗",
        StageState::Skipped => "–",
        StageState::Blocked => "!",
    }
}

fn stage_word(state: StageState) -> &'static str {
    match state {
        StageState::Pending => "pending",
        StageState::Running => "running",
        StageState::Succeeded => "succeeded",
        StageState::Failed => "failed",
        StageState::Skipped => "skipped",
        StageState::Blocked => "blocked",
    }
}

fn write_clipped(buf: &mut Buffer, x: u16, y: u16, max_w: usize, text: &str, style: Style) {
    let clipped = truncate_to_width(text, max_w);
    buf.set_stringn(x, y, clipped, max_w, style);
}
