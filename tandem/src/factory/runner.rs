use std::cmp::Ordering;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde::Serialize;

use crate::coordination::filelock::exclusive_lock;
use crate::factory::FactoryError;
use crate::factory::approval::{
    ApprovalError, Caller, require_user, take_pending_run, verify_snapshot,
};
use crate::factory::broker::git::{AllowedGitSubcommand, run_git};
use crate::factory::broker::job::{JobState, job_dir, jobs_dir, read_job_state, set_job_state};
use crate::factory::broker::result::JobResult;
use crate::factory::budget::{
    BudgetBreach, BudgetCapKind, BudgetCheck, RunBudget, check as check_budget,
    effective_stage_budget, spent as compute_spent,
};
use crate::factory::events::{
    Event, append_locked, read_all, redact_events, reject_if_symlink, set_private_file_mode,
};
use crate::factory::executor::{Executor, StageJob, StageOutcome, last_stage_provider_model};
use crate::factory::ids::{is_valid_idea_id, is_valid_run_id};
use crate::factory::login::interactive_ready;
use crate::factory::login::{HostLoginProbe, LOGIN_SPECS, LoginProbe};
use crate::factory::machine::{self, Command, EventData, Facts};
use crate::factory::model::{
    Actor, ActorKind, FactorySettings, Idea, IdeaState, Run, RunState, StageState,
};
use crate::factory::redact::RedactContext;
use crate::factory::routing::RoutingContext;
use crate::factory::run::{RunError, get_run, list_runs};
use crate::factory::stages::plan::SlicesManifest;
use crate::factory::store::{FactoryStore, current_time, write_atomic};
use crate::factory::telemetry::{RecordJobUsageInput, record_interactive_job_usage};
use crate::factory::workflow::parse::load_workflow_file;
use crate::factory::workflow::scheduler::{
    RunFacts, SchedulerLimits, TriState, next, stage_has_writer_tool,
};
use crate::factory::workflow::validate::validate;
use crate::factory::workspace::{WorkspaceError, prepare_worktree};

#[derive(Debug, thiserror::Error)]
pub enum RunnerError {
    #[error("a factory runner is already active in this workspace")]
    AlreadyRunning,
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    Gate(String),
    #[error(transparent)]
    Approval(#[from] ApprovalError),
    #[error(transparent)]
    Run(#[from] RunError),
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    #[error(transparent)]
    Factory(#[from] FactoryError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl RunnerError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Approval(e) => e.exit_code(),
            Self::Gate(_) => 3,
            _ => 1,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Approval(e) => e.code(),
            Self::Gate(_) => "gate_not_met",
            Self::InvalidInput(_) => "invalid_input",
            Self::Run(e) => e.code(),
            Self::AlreadyRunning => "already_running",
            Self::Workspace(_) | Self::Factory(_) | Self::Io(_) => "internal_error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoppedJobInfo {
    pub stage: String,
    pub attempt: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct PauseRunResult {
    pub run: Run,
    pub stopped_jobs: Vec<StoppedJobInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CancelRunResult {
    pub run: Run,
    pub idea: Idea,
    pub stopped_jobs: Vec<StoppedJobInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CancelIdeaResult {
    pub idea: Idea,
    pub cancelled_runs: Vec<Run>,
    pub stopped_jobs: Vec<StoppedJobInfo>,
}

pub fn has_ui_slices(store: &FactoryStore, run_id: &str) -> bool {
    let slices_path = store.run_dir(run_id).join("artifacts/slices.json");
    if !slices_path.exists() || reject_if_symlink(&slices_path).is_err() {
        return false;
    }
    let Ok(raw) = fs::read_to_string(&slices_path) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_str::<SlicesManifest>(&raw) else {
        return false;
    };
    manifest.slices.iter().any(|s| s.ui)
}

fn check_run_slices_exclusions(
    store: &FactoryStore,
    run_id: &str,
    settings: &FactorySettings,
) -> Option<(String, String)> {
    let slices_path = store.run_dir(run_id).join("artifacts/slices.json");
    if !slices_path.exists() || reject_if_symlink(&slices_path).is_err() {
        return None;
    }
    let raw = fs::read_to_string(&slices_path).ok()?;
    let manifest: crate::factory::stages::plan::SlicesManifest = serde_json::from_str(&raw).ok()?;
    let paths: Vec<String> = manifest
        .slices
        .iter()
        .flat_map(|s| s.paths.iter().cloned())
        .collect();
    crate::factory::data_policy::check_slices_against_exclusions(&paths, settings)
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star_p, mut star_t) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_p = Some(pi);
            star_t = ti;
            pi += 1;
        } else if let Some(sp) = star_p {
            pi = sp + 1;
            star_t += 1;
            ti = star_t;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn path_matches_ui_pattern(path: &str, raw_pattern: &str) -> bool {
    let norm_path = path.trim().replace('\\', "/");
    let norm_path = norm_path.trim_start_matches("./");
    let pat = raw_pattern.trim().replace('\\', "/");
    let pat = pat.trim_start_matches("./").trim_end_matches('/');
    if pat.is_empty() {
        return false;
    }
    if pat.contains('*') || pat.contains('?') {
        if let Some(prefix) = pat.strip_suffix("/**")
            && (norm_path == prefix || norm_path.starts_with(&format!("{prefix}/")))
        {
            return true;
        }
        return glob_match(pat, norm_path);
    }
    norm_path == pat || norm_path.starts_with(&format!("{pat}/"))
}

pub fn compute_ui_changed(store: &FactoryStore, run: &Run, settings: &FactorySettings) -> TriState {
    let develop_done = run
        .stages
        .iter()
        .any(|s| s.name == "develop" && s.state == StageState::Succeeded);
    if !develop_done {
        return TriState::Unknown;
    }
    if has_ui_slices(store, &run.id) {
        return TriState::Yes;
    }
    let target_name = run.target.as_deref().unwrap_or("workspace");
    let Some(checks) = settings.checks.get(target_name) else {
        return TriState::No;
    };
    if checks.ui_paths.is_empty() {
        return TriState::No;
    }
    let Some(wt_str) = run.worktree_path.as_deref() else {
        return TriState::No;
    };
    let wt = Path::new(wt_str);
    if !wt.exists() {
        return TriState::No;
    }
    let range = run
        .base_commit
        .as_deref()
        .filter(|b| !b.trim().is_empty())
        .map(|b| format!("{b}..HEAD"));
    let args: Vec<&str> = match range.as_deref() {
        Some(r) => vec!["--name-only", r],
        None => vec!["--name-only", "HEAD"],
    };
    let Ok(out) = run_git(
        AllowedGitSubcommand::Diff,
        &args,
        wt,
        Duration::from_secs(15),
    ) else {
        return TriState::No;
    };
    let touched_ui = out.lines().any(|file| {
        let f = file.trim();
        !f.is_empty()
            && checks
                .ui_paths
                .iter()
                .any(|p| path_matches_ui_pattern(f, p))
    });
    if touched_ui {
        TriState::Yes
    } else {
        TriState::No
    }
}

pub fn stop_request_path(store: &FactoryStore, run_id: &str) -> PathBuf {
    store.run_dir(run_id).join("stop.req")
}

pub fn is_stop_requested(store: &FactoryStore, run_id: &str) -> bool {
    stop_request_path(store, run_id).exists()
}

pub fn acquire_runner_lock(store: &FactoryStore) -> Result<File, RunnerError> {
    store.ensure()?;
    let path = store.root.join(".runner.lock");
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)?;
    set_private_file_mode(&path)?;
    match file.try_lock() {
        Ok(()) => {
            use std::io::{Seek, SeekFrom, Write};
            file.set_len(0)?;
            file.seek(SeekFrom::Start(0))?;
            writeln!(file, "{}", super::broker::current_process_id())?;
            file.sync_all()?;
            Ok(file)
        }
        Err(TryLockError::WouldBlock) => Err(RunnerError::AlreadyRunning),
        Err(TryLockError::Error(e)) => Err(RunnerError::Io(e)),
    }
}

pub fn append_run_events(
    store: &FactoryStore,
    idea_id: &str,
    run_id: &str,
    run_opt: Option<Run>,
    events: Vec<EventData>,
    actor: &Actor,
) -> Result<Run, RunnerError> {
    if events.is_empty() {
        return run_opt.ok_or_else(|| RunnerError::InvalidInput("missing run".to_string()));
    }
    store.ensure()?;
    let lock_path = store.lock_path();
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    set_private_file_mode(&lock_path)?;
    let _guard = exclusive_lock(&lock_file)?;

    let ts = current_time();
    let mut pending = Vec::with_capacity(events.len());
    for ev_data in &events {
        pending.push(Event::from_data(
            ts,
            actor.clone(),
            idea_id.to_string(),
            Some(run_id.to_string()),
            ev_data,
        )?);
    }
    let mut next_run = get_run(store, run_id)?.or(run_opt);
    let mut next_idea = store.get_idea(idea_id)?;
    let redacted = redact_events(&pending, &RedactContext::from_env());
    let appended = append_locked(&store.events_path(), &redacted)?;
    for (ev, ev_data) in appended.iter().zip(events.iter()) {
        next_run = machine::apply_run(next_run, ev_data, idea_id, ev.ts);
        next_idea = machine::apply(next_idea, ev_data, ev.ts);
    }
    let updated_run = next_run
        .ok_or_else(|| RunnerError::InvalidInput("events did not produce run state".to_string()))?;
    let run_bytes = serde_json::to_vec_pretty(&updated_run).map_err(FactoryError::from)?;
    write_atomic(&store.run_json_path(run_id), &run_bytes)?;
    if let Some(updated_idea) = next_idea {
        let idea_bytes = serde_json::to_vec_pretty(&updated_idea).map_err(FactoryError::from)?;
        write_atomic(&store.idea_path(idea_id), &idea_bytes)?;
    }
    Ok(updated_run)
}

fn pause_run(
    store: &FactoryStore,
    run: Run,
    reason: String,
    commands: Vec<String>,
) -> Result<(), RunnerError> {
    let sys = Actor {
        kind: ActorKind::System,
        id: "runner".to_string(),
    };
    let idea_id = run.idea.clone();
    let run_id = run.id.clone();
    append_run_events(
        store,
        &idea_id,
        &run_id,
        Some(run),
        vec![EventData::RunPaused {
            run: run_id.clone(),
            reason,
            commands,
            cap: None,
            spent: None,
            limit: None,
        }],
        &sys,
    )?;
    Ok(())
}

fn pause_run_budget_breach(
    store: &FactoryStore,
    run: Run,
    breach: BudgetBreach,
    stop_running: bool,
) -> Result<(), RunnerError> {
    let sys = Actor {
        kind: ActorKind::System,
        id: "runner".to_string(),
    };
    let mut evs = if stop_running {
        interrupt_running_jobs(store, &run)?.1
    } else {
        Vec::new()
    };
    let idea_id = run.idea.clone();
    let run_id = run.id.clone();
    evs.push(EventData::RunPaused {
        run: run_id.clone(),
        reason: breach.message,
        commands: vec![breach.action_command],
        cap: Some(breach.cap_name),
        spent: Some(breach.spent),
        limit: Some(breach.limit),
    });
    append_run_events(store, &idea_id, &run_id, Some(run), evs, &sys)?;
    Ok(())
}

pub fn deliver_if_pr_created(
    store: &FactoryStore,
    workspace: &Path,
    run_id: &str,
) -> Result<bool, RunnerError> {
    let Some(run) = get_run(store, run_id)? else {
        return Ok(false);
    };
    if run.state != RunState::Review {
        return Ok(false);
    }
    let Some(idea) = store.get_idea(&run.idea)? else {
        return Ok(false);
    };
    if idea.state != IdeaState::Review {
        return Ok(false);
    }
    let prs = if run.prs.is_empty() {
        idea.prs.clone()
    } else {
        run.prs.clone()
    };
    if prs.is_empty() {
        return Ok(false);
    }

    let sys = Actor {
        kind: ActorKind::System,
        id: "runner".to_string(),
    };
    let events = machine::decide(
        Some(&idea),
        &Command::Deliver {
            run: run_id.to_string(),
            prs,
        },
        &sys,
        &Facts::default(),
    )
    .map_err(FactoryError::from)?;

    append_run_events(store, &idea.id, run_id, Some(run), events, &sys)?;
    super::handoff::write_and_record_handoff(store, workspace, run_id, &sys)
        .map_err(|e| RunnerError::InvalidInput(e.to_string()))?;
    Ok(true)
}

pub fn drive_run_step(
    store: &FactoryStore,
    workspace: &Path,
    run_id: &str,
    executor: &mut dyn Executor,
) -> Result<(), RunnerError> {
    drive_run_step_with_gh(store, workspace, run_id, executor, Path::new("gh"))
}

pub fn drive_run_step_with_gh(
    store: &FactoryStore,
    workspace: &Path,
    run_id: &str,
    executor: &mut dyn Executor,
    gh_binary: &Path,
) -> Result<(), RunnerError> {
    let Some(mut run) = get_run(store, run_id)? else {
        return Ok(());
    };
    if run.state == RunState::Review {
        let sys = Actor {
            kind: ActorKind::System,
            id: "runner".to_string(),
        };
        if run.prs.is_empty() {
            let outcome =
                super::pr::open_pull_request_with_gh(store, workspace, run_id, &sys, gh_binary)
                    .map_err(|e| RunnerError::InvalidInput(e.to_string()))?;
            if matches!(outcome, super::pr::PrOutcome::Paused { .. }) {
                return Ok(());
            }
        }
        let _ = deliver_if_pr_created(store, workspace, run_id)?;
        return Ok(());
    }
    if !matches!(run.state, RunState::Queued | RunState::Running) {
        return Ok(());
    }

    let settings = store.settings()?;
    if run.state == RunState::Queued
        && let Ok(Some(idea)) = store.get_idea(&run.idea)
        && idea_within_grace_period(&idea, settings.approval_grace_secs, current_time())
    {
        return Ok(());
    }
    let wf_path = store.run_dir(run_id).join("workflow.yaml");
    let raw = load_workflow_file(&wf_path).map_err(|e| RunnerError::InvalidInput(e.to_string()))?;
    let mut validation_settings = settings.clone();
    validation_settings.max_parallel_agents = validation_settings
        .max_parallel_agents
        .max(crate::factory::settings::MAX_PARALLEL_AGENTS);
    let mut wf = validate(&raw, &validation_settings).map_err(|diags| {
        let msg = diags
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        RunnerError::InvalidInput(msg)
    })?;
    for s in &mut wf.stages {
        s.budget.max_retries.get_or_insert(2);
    }

    match prepare_worktree(store, workspace, run_id) {
        Ok(_) => {
            if let Some(r) = get_run(store, run_id)? {
                run = r;
            }
        }
        Err(WorkspaceError::AmbiguousTarget(choices)) => {
            let cmds: Vec<String> = choices
                .iter()
                .map(|c| format!("tandem factory run target {run_id} {c}"))
                .collect();
            return pause_run(
                store,
                run,
                format!("choose a target repository: {}", choices.join(", ")),
                cmds,
            );
        }
        Err(e) => return Err(RunnerError::Workspace(e)),
    }

    let sys = Actor {
        kind: ActorKind::System,
        id: "runner".to_string(),
    };

    if run.baseline_passed.is_none()
        && run.stages.iter().all(|s| s.attempts == 0)
        && let Some(wt_str) = run.worktree_path.as_deref()
    {
        let target_name = run.target.as_deref().unwrap_or("workspace");
        if let Ok(confirmed) = super::checks::require_confirmed_checks(&settings, target_name) {
            let artifact_dir = store.run_dir(run_id).join("artifacts");
            let _ = fs::create_dir_all(&artifact_dir);
            let report = super::stages::test::run_test_stage_checks(
                &confirmed,
                Path::new(wt_str),
                &artifact_dir,
                0,
            )
            .map_err(|e| RunnerError::InvalidInput(e.to_string()))?;
            let failed_checks: Vec<String> = report
                .outcomes
                .iter()
                .filter(|o| !o.passed())
                .map(|o| o.argv.join(" "))
                .collect();
            run = append_run_events(
                store,
                &run.idea.clone(),
                &run.id.clone(),
                Some(run),
                vec![EventData::RunBaseline {
                    run: run_id.to_string(),
                    passed: report.all_passed,
                    failed_checks,
                }],
                &sys,
            )?;
            if !report.all_passed {
                return pause_run(
                    store,
                    run,
                    "checks fail on the base branch".to_string(),
                    vec![format!("tandem factory run resume {run_id}")],
                );
            }
        }
    }

    let ui_changed = compute_ui_changed(store, &run, &settings);
    let is_ui = matches!(ui_changed, TriState::Yes) || has_ui_slices(store, run_id);
    let now = current_time();
    let run_approvals = super::protected::load_run_approvals(store, run_id)?;
    let facts = RunFacts {
        ui_changed,
        deploy_requested: run.deploy_requested,
        approvals: super::protected::to_scheduler_approvals(&run_approvals, &run, now),
    };
    let workspace_running = count_workspace_running_jobs(store);
    let limits = SchedulerLimits::from_workflow_and_settings(&wf, &settings, &run.stages)
        .with_workspace_running(workspace_running);
    let decision = next(&wf, &run.stages, &facts, &limits);

    if !decision.skip.is_empty() {
        let evs: Vec<EventData> = decision
            .skip
            .iter()
            .map(|s| EventData::StageSkipped {
                run: run.id.clone(),
                stage: s.name.clone(),
                reason: s.reason.clone(),
            })
            .collect();
        run = append_run_events(
            store,
            &run.idea.clone(),
            &run.id.clone(),
            Some(run),
            evs,
            &sys,
        )?;
    }

    if let Some(failed) = decision.failed.first()
        && failed.retries_exhausted
    {
        return pause_run(
            store,
            run,
            format!("stage \"{}\" failed and retries are used up", failed.name),
            vec![format!(
                "tandem factory run retry {} {} --hypothesis \"<new hypothesis>\"",
                run_id, failed.name
            )],
        );
    }

    if decision.finished == Some(RunState::Succeeded) {
        append_run_events(
            store,
            &run.idea.clone(),
            &run.id.clone(),
            Some(run),
            vec![EventData::RunStagesComplete {
                run: run_id.to_string(),
            }],
            &sys,
        )?;
        return Ok(());
    }

    let writer_busy = wf.stages.iter().any(|stage_def| {
        run.stages
            .iter()
            .any(|s| s.name == stage_def.name && s.state == StageState::Running)
            && stage_has_writer_tool(stage_def)
    });
    let stage_to_run = decision.start.first().cloned().or_else(|| {
        if limits.available_slots() == 0 {
            return None;
        }
        decision
            .failed
            .iter()
            .filter(|f| !f.retries_exhausted)
            .find(|f| {
                let is_writer = wf
                    .stages
                    .iter()
                    .find(|s| s.name == f.name)
                    .is_some_and(stage_has_writer_tool);
                !(is_writer && writer_busy)
            })
            .map(|f| f.name.clone())
    });

    let resumed_finished = run.stages.iter().find_map(|s| {
        if s.state == StageState::Running
            && s.attempts > 0
            && matches!(
                read_job_state(&job_dir(store, run_id, &s.name, s.attempts)),
                Ok(JobState::Finished)
            )
            && super::broker::result::parse_and_validate_result_strict(
                &job_dir(store, run_id, &s.name, s.attempts).join("result.json"),
                s.attempts > 1,
            )
            .is_ok()
        {
            Some((s.name.clone(), s.attempts))
        } else {
            None
        }
    });

    let (stage_name, resumed_attempt) = if let Some((name, attempt)) = resumed_finished {
        (name, Some(attempt))
    } else {
        let Some(stage_name) = stage_to_run else {
            if let Some(blocked) = decision.blocked.first() {
                let reason = blocked.reason.replace("<run>", run_id);
                let mut cmds = Vec::new();
                if let Some(sdef) = wf.stages.iter().find(|s| s.name == blocked.name)
                    && sdef
                        .requires
                        .contains(&super::workflow::schema::StageRequirement::HumanApproval)
                {
                    let action =
                        super::protected::action_for_stage(sdef, &wf.policy.require_human_approval);
                    cmds.push(format!(
                        "tandem factory run approve {run_id} --action {action} --confirm {run_id}"
                    ));
                }
                return pause_run(store, run, reason, cmds);
            }
            return Ok(());
        };
        (stage_name, None)
    };

    if stage_name == "demo" {
        let dep_status = crate::web_deps::screenshot_tool_status(
            &crate::web_deps::DepsEnv::from_process(),
            &store.root,
        );
        if !dep_status.available_for_demo() {
            let reason = "screenshot tool not installed".to_string();
            let _ = super::evidence::record_demo_skipped_no_tool(
                store, &run.idea, &run.id, &reason, &sys,
            );
            append_run_events(
                store,
                &run.idea.clone(),
                &run.id.clone(),
                Some(run),
                vec![EventData::StageSkipped {
                    run: run_id.to_string(),
                    stage: "demo".to_string(),
                    reason,
                }],
                &sys,
            )?;
            return Ok(());
        }
    }

    let open_providers: Vec<&str> = executor.ready_clis().iter().map(|r| r.cli).collect();
    if open_providers.is_empty() {
        return pause_run(
            store,
            run,
            "no CLI is installed and signed in".to_string(),
            vec!["tandem factory login open claude".to_string()],
        );
    }

    let stage_def = wf
        .stages
        .iter()
        .find(|s| s.name == stage_name)
        .ok_or_else(|| RunnerError::InvalidInput(format!("stage {stage_name} missing")))?;

    let needs_checks = stage_def.name == "test"
        || stage_def.name == "check"
        || stage_def
            .tools
            .iter()
            .any(|t| matches!(t, super::workflow::schema::WorkflowTool::LintRunner));
    if needs_checks {
        let target_name = run.target.as_deref().unwrap_or("workspace").to_string();
        if super::checks::require_confirmed_checks(&settings, &target_name).is_err() {
            return pause_run(
                store,
                run,
                "test and lint commands are not confirmed".to_string(),
                vec![format!(
                    "tandem factory checks discover --target {target_name}"
                )],
            );
        }
    }
    let current_attempts = run
        .stages
        .iter()
        .find(|s| s.name == stage_name)
        .map(|s| s.attempts)
        .unwrap_or(0);
    let next_attempt = resumed_attempt.unwrap_or(current_attempts + 1);

    let preferred_provider = if settings.default_worker_cli != "auto" {
        Some(settings.default_worker_cli.clone())
    } else {
        store
            .get_idea(&run.idea)?
            .and_then(|i| i.orchestrator)
            .map(|o| o.cli)
    };
    let (avoid_provider, avoid_model) = if stage_name == "review" {
        last_stage_provider_model(store, &run, "develop")
    } else {
        (None, None)
    };
    let r_ctx = RoutingContext {
        preferred_provider,
        is_ui_work: is_ui,
        avoid_provider,
        avoid_model,
        ..RoutingContext::default()
    };

    let target_name = run
        .target
        .clone()
        .unwrap_or_else(|| "workspace".to_string());
    if stage_name == "deploy" {
        if run.prs.is_empty() {
            return pause_run(
                store,
                run,
                "a pull request is required before deploy".to_string(),
                vec![format!("tandem factory run prs {run_id} --refresh")],
            );
        }
        if super::checks::require_confirmed_deploy(&settings, &target_name).is_err() {
            return pause_run(
                store,
                run,
                format!("no deploy command is confirmed for {target_name}"),
                vec![format!(
                    "tandem factory checks set --target {target_name} --deploy '[\"<cmd>\"]' --confirm {target_name}"
                )],
            );
        }
    }

    let matching_approval = super::protected::find_matching_approval(
        &run_approvals,
        &run.id,
        &stage_name,
        &run.workflow_sha256,
        now,
    )
    .cloned();
    let tool_grant = match crate::factory::permissions::grant(
        stage_def.agent,
        stage_def,
        &target_name,
        &wf,
        &settings,
        matching_approval.is_some(),
    ) {
        Ok(g) => g,
        Err(denied) => {
            let cmds = denied.action_commands(&run.id);
            return pause_run(store, run, denied.to_string(), cmds);
        }
    };

    let routed_res: Result<crate::factory::routing::Route, String> =
        super::team::resolve_stage_assignment(
            store,
            &run,
            stage_def,
            &tool_grant,
            &wf,
            &settings,
            &open_providers,
            next_attempt,
            &r_ctx,
        )
        .map(|resolved| resolved.route)
        .map_err(|e| e.to_string());
    let routed = match routed_res {
        Ok(r) => r,
        Err(reason) => {
            let mut cmds = Vec::new();
            if !settings.orchestrator_may_choose
                && settings.default_worker_cli != "auto"
                && reason.contains("configured default_worker_cli")
            {
                cmds.push(format!(
                    "tandem factory login open {}",
                    settings.default_worker_cli
                ));
            }
            let sys = Actor {
                kind: ActorKind::System,
                id: "engine".to_string(),
            };
            let idea_id = run.idea.clone();
            let r_id = run.id.clone();
            let run_after_block = append_run_events(
                store,
                &idea_id,
                &r_id,
                Some(run),
                vec![EventData::StageBlocked {
                    run: r_id.clone(),
                    stage: stage_name.clone(),
                    reason: reason.clone(),
                }],
                &sys,
            )?;
            return pause_run(store, run_after_block, reason, cmds);
        }
    };

    if let Some((excluded_file, rule)) = check_run_slices_exclusions(store, &run.id, &settings) {
        let reason = format!(
            "slice requires excluded file '{excluded_file}' ({rule}); update data policy or slice paths"
        );
        let sys = Actor {
            kind: ActorKind::System,
            id: "engine".to_string(),
        };
        let idea_id = run.idea.clone();
        let r_id = run.id.clone();
        let run_after_block = append_run_events(
            store,
            &idea_id,
            &r_id,
            Some(run),
            vec![EventData::StageBlocked {
                run: r_id.clone(),
                stage: stage_name.clone(),
                reason: reason.clone(),
            }],
            &sys,
        )?;
        return pause_run(store, run_after_block, reason, vec![]);
    }

    let run_budget = RunBudget::from_run_or_settings(&run, &settings);
    let pre_log = read_all(&store.events_path())?;
    let pre_spent = compute_spent(&run, &pre_log.events, current_time());
    if let BudgetCheck::Breach(breach) = check_budget(&run.id, &run_budget, &pre_spent, true) {
        return pause_run_budget_breach(store, run, breach, false);
    }
    let _eff_stage_budget = effective_stage_budget(&stage_def.budget, &run_budget, &pre_spent);

    if resumed_attempt.is_none() {
        let mut pre_evs = Vec::new();
        let idea_is_queued = store
            .get_idea(&run.idea)?
            .is_some_and(|i| i.state == crate::factory::model::IdeaState::Queued);
        if run.state == RunState::Queued || idea_is_queued {
            pre_evs.push(EventData::RunStarted {
                run: run.id.clone(),
            });
        }
        if let Some(from_tier) = routed.upgraded_from {
            pre_evs.push(EventData::StageModelUpgraded {
                run: run.id.clone(),
                stage: stage_name.clone(),
                from: format!("{from_tier:?}").to_lowercase(),
                to: format!("{:?}", routed.tier).to_lowercase(),
            });
        }
        pre_evs.push(EventData::StageStarted {
            run: run.id.clone(),
            stage: stage_name.clone(),
            attempt: next_attempt,
            grant: Some(tool_grant),
        });
        if let Some(approval) = matching_approval {
            pre_evs.push(EventData::ApprovalConsumed {
                run: run.id.clone(),
                stage: stage_name.clone(),
                action: approval.action,
                approval_seq: approval.seq,
            });
        }
        run = append_run_events(
            store,
            &run.idea.clone(),
            &run.id.clone(),
            Some(run),
            pre_evs,
            &sys,
        )?;
    }

    let job_t0 = std::time::Instant::now();
    let outcome = executor.run(&StageJob {
        run: run.id.clone(),
        stage: stage_name.clone(),
        attempt: next_attempt,
    });
    let job_duration_secs = job_t0.elapsed().as_secs_f64();

    let usage_already_recorded = pre_log.events.iter().any(|ev| {
        matches!(
            ev.to_event_data(),
            Ok(EventData::JobUsage {
                ref run,
                ref stage,
                attempt,
                ..
            }) if run == run_id && stage == &stage_name && attempt == next_attempt
        )
    });

    if !matches!(outcome, StageOutcome::Blocked { .. }) && !usage_already_recorded {
        let result_path = job_dir(store, run_id, &stage_name, next_attempt).join("result.json");
        let parsed_usage = fs::read_to_string(&result_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<JobResult>(&raw).ok())
            .and_then(|r| r.usage);
        let usage_input = RecordJobUsageInput {
            store,
            idea_id: &run.idea,
            run_id,
            stage: &stage_name,
            attempt: next_attempt,
            provider: &routed.provider,
            model: &routed.model,
            tier: routed.tier,
            duration_secs: job_duration_secs,
            usage: parsed_usage.as_ref(),
            price_table: &settings.price_table,
            actor: &sys,
        };
        record_interactive_job_usage(usage_input)?;
    }

    let mut current_run = match get_run(store, run_id)? {
        Some(r) => r,
        None => return Ok(()),
    };
    if current_run.state == RunState::Cancelled {
        return Ok(());
    }

    match outcome {
        StageOutcome::Succeeded => {
            if stage_name == "review" {
                let artifact_dir = store.run_dir(run_id).join("artifacts");
                if artifact_dir.join("findings.json").exists() {
                    let report = match super::stages::review::load_findings_report(&artifact_dir) {
                        Ok(r) => r,
                        Err(failure) => {
                            let reason = failure.to_string();
                            let run_after_fail = append_run_events(
                                store,
                                &current_run.idea.clone(),
                                &current_run.id.clone(),
                                Some(current_run),
                                vec![EventData::StageFailed {
                                    run: run_id.to_string(),
                                    stage: stage_name,
                                    reason: reason.clone(),
                                }],
                                &sys,
                            )?;
                            return pause_run(
                                store,
                                run_after_fail,
                                reason,
                                vec![format!("tandem factory run resume {run_id}")],
                            );
                        }
                    };
                    let round = current_run
                        .stages
                        .iter()
                        .find(|s| s.name == "review")
                        .map_or(1, |s| s.attempts.max(1));
                    let record =
                        super::stages::review::record_review_round(&artifact_dir, round, &report)
                            .map_err(|e| RunnerError::InvalidInput(e.to_string()))?;
                    if let Ok(findings_bytes) = serde_json::to_vec_pretty(&report) {
                        let summary = if report.findings.is_empty() {
                            format!("review round {round}: clean")
                        } else {
                            format!("review round {round}: {} finding(s)", report.findings.len())
                        };
                        if let Ok(ev_item) = super::evidence::record_review_findings(
                            store,
                            &current_run.idea,
                            &current_run.id,
                            round,
                            &findings_bytes,
                            summary,
                            &sys,
                        ) {
                            current_run.evidence.push(ev_item);
                        }
                    }

                    if record.derived_verdict
                        == super::stages::review::ReviewVerdict::ChangesRequested
                    {
                        let findings_summary =
                            super::stages::review::format_blocking_findings_summary(
                                &report.findings,
                            );
                        if round < settings.max_review_rounds {
                            append_run_events(
                                store,
                                &current_run.idea.clone(),
                                &current_run.id.clone(),
                                Some(current_run),
                                vec![
                                    EventData::StageSucceeded {
                                        run: run_id.to_string(),
                                        stage: stage_name,
                                    },
                                    EventData::RunStagesComplete {
                                        run: run_id.to_string(),
                                    },
                                    EventData::RunChangesRequested {
                                        run: run_id.to_string(),
                                    },
                                    EventData::StageReopened {
                                        run: run_id.to_string(),
                                        stage: "develop".to_string(),
                                        reason: findings_summary,
                                    },
                                ],
                                &sys,
                            )?;
                        } else {
                            let reason = format!(
                                "review requested changes after {round} round(s); max_review_rounds ({}) exhausted:\n{findings_summary}",
                                settings.max_review_rounds
                            );
                            let run_after_fail = append_run_events(
                                store,
                                &current_run.idea.clone(),
                                &current_run.id.clone(),
                                Some(current_run),
                                vec![EventData::StageFailed {
                                    run: run_id.to_string(),
                                    stage: stage_name,
                                    reason: reason.clone(),
                                }],
                                &sys,
                            )?;
                            return pause_run(
                                store,
                                run_after_fail,
                                reason,
                                vec![format!("tandem factory run resume {run_id}")],
                            );
                        }
                        return Ok(());
                    }
                }
            }
            append_run_events(
                store,
                &current_run.idea.clone(),
                &current_run.id.clone(),
                Some(current_run),
                vec![EventData::StageSucceeded {
                    run: run_id.to_string(),
                    stage: stage_name,
                }],
                &sys,
            )?;
        }
        StageOutcome::Interrupted => {
            let still_running = current_run
                .stages
                .iter()
                .any(|s| s.name == stage_name && s.state == StageState::Running);
            if still_running {
                append_run_events(
                    store,
                    &current_run.idea.clone(),
                    &current_run.id.clone(),
                    Some(current_run),
                    vec![EventData::StageInterrupted {
                        run: run_id.to_string(),
                        stage: stage_name,
                    }],
                    &sys,
                )?;
            }
        }
        StageOutcome::Failed { reason } => {
            let prev_reason = current_run
                .stages
                .iter()
                .find(|s| s.name == stage_name)
                .and_then(|s| s.rationale.as_deref())
                .map(|r| r.trim().to_lowercase());
            let norm = reason.trim().to_lowercase();
            let has_new_hypothesis = !norm.is_empty()
                && norm != "no structured result"
                && prev_reason.as_deref() != Some(norm.as_str());
            let already_paused = current_run.state == RunState::Paused;

            let run_after_fail = append_run_events(
                store,
                &current_run.idea.clone(),
                &current_run.id.clone(),
                Some(current_run),
                vec![EventData::StageFailed {
                    run: run_id.to_string(),
                    stage: stage_name.clone(),
                    reason: reason.clone(),
                }],
                &sys,
            )?;

            if stage_name == "test"
                && let Some(dev_def) = wf.stages.iter().find(|s| s.name == "develop")
            {
                let dev_max_retries = dev_def.budget.max_retries.unwrap_or(0);
                let dev_retries_used = run_after_fail
                    .stages
                    .iter()
                    .find(|s| s.name == "develop")
                    .map_or(0, |s| s.attempts.saturating_sub(1 + s.interrupted_attempts));
                if dev_retries_used < dev_max_retries {
                    append_run_events(
                        store,
                        &run_after_fail.idea.clone(),
                        &run_after_fail.id.clone(),
                        Some(run_after_fail),
                        vec![EventData::StageReopened {
                            run: run_id.to_string(),
                            stage: "develop".to_string(),
                            reason,
                        }],
                        &sys,
                    )?;
                } else if !already_paused {
                    pause_run(
                        store,
                        run_after_fail,
                        format!(
                            "stage \"develop\" retries are used up after test failure: {reason}"
                        ),
                        vec![format!(
                            "tandem factory run retry {run_id} develop --hypothesis \"<new hypothesis>\""
                        )],
                    )?;
                }
            } else if !has_new_hypothesis && !already_paused {
                pause_run(
                    store,
                    run_after_fail,
                    format!("stage \"{stage_name}\" failed without a new hypothesis: {reason}"),
                    vec![format!(
                        "tandem factory run retry {run_id} {stage_name} --hypothesis \"<new hypothesis>\""
                    )],
                )?;
            }
        }
        StageOutcome::Blocked {
            reason,
            action_commands,
        } => {
            let idea_id = current_run.idea.clone();
            let run_after_block = append_run_events(
                store,
                &idea_id,
                run_id,
                Some(current_run),
                vec![EventData::StageBlocked {
                    run: run_id.to_string(),
                    stage: stage_name,
                    reason: reason.clone(),
                }],
                &sys,
            )?;
            pause_run(store, run_after_block, reason, action_commands)?;
        }
    }

    if let Some(after_run) = get_run(store, run_id)?
        && matches!(after_run.state, RunState::Queued | RunState::Running)
    {
        let post_log = read_all(&store.events_path())?;
        let post_spent = compute_spent(&after_run, &post_log.events, current_time());
        if let BudgetCheck::Breach(breach) =
            check_budget(&after_run.id, &run_budget, &post_spent, false)
        {
            let stop_running = breach.cap == BudgetCapKind::Duration;
            pause_run_budget_breach(store, after_run, breach, stop_running)?;
        }
    }

    Ok(())
}

fn interrupt_running_jobs(
    store: &FactoryStore,
    run: &Run,
) -> Result<(Vec<StoppedJobInfo>, Vec<EventData>), RunnerError> {
    let run_dir = store.run_dir(&run.id);
    if run_dir.exists() {
        let _ = write_atomic(&stop_request_path(store, &run.id), b"stop\n");
    }

    let j_root = jobs_dir(store, &run.id);
    if j_root.is_dir()
        && let Ok(entries) = fs::read_dir(&j_root)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && matches!(read_job_state(&path), Ok(JobState::Running)) {
                let _ = set_job_state(&path, JobState::Interrupted);
            }
        }
    }

    let mut stopped = Vec::new();
    let mut events = Vec::new();
    for st in &run.stages {
        if st.state == StageState::Running {
            stopped.push(StoppedJobInfo {
                stage: st.name.clone(),
                attempt: st.attempts,
            });
            events.push(EventData::StageInterrupted {
                run: run.id.clone(),
                stage: st.name.clone(),
            });
        }
    }
    Ok((stopped, events))
}

pub fn pause_run_by_user(
    store: &FactoryStore,
    run_id: &str,
    now: bool,
    actor: &Actor,
    caller: &Caller<'_>,
) -> Result<PauseRunResult, RunnerError> {
    require_user(caller)?;
    if actor.kind != ActorKind::User {
        return Err(RunnerError::Approval(ApprovalError::AgentCaller));
    }
    if !is_valid_run_id(run_id) {
        return Err(RunnerError::InvalidInput(format!(
            "invalid run id: {run_id}"
        )));
    }
    let run = get_run(store, run_id)?
        .ok_or_else(|| RunnerError::Run(RunError::NotFound(format!("run {run_id}"))))?;
    if !matches!(run.state, RunState::Queued | RunState::Running) {
        return Err(RunnerError::InvalidInput(format!(
            "run {run_id} is in state {:?} and cannot be paused",
            run.state
        )));
    }

    let (stopped_jobs, mut evs) = if now {
        interrupt_running_jobs(store, &run)?
    } else {
        (Vec::new(), Vec::new())
    };
    let reason = if now {
        "paused by user (--now)".to_string()
    } else {
        "paused by user".to_string()
    };
    evs.push(EventData::RunPaused {
        run: run_id.to_string(),
        reason,
        commands: vec![format!("tandem factory run resume {run_id}")],
        cap: None,
        spent: None,
        limit: None,
    });

    let updated = append_run_events(
        store,
        &run.idea.clone(),
        &run.id.clone(),
        Some(run),
        evs,
        actor,
    )?;
    Ok(PauseRunResult {
        run: updated,
        stopped_jobs,
    })
}

fn verify_run_resumable(
    store: &FactoryStore,
    run: &Run,
    probe: &dyn LoginProbe,
) -> Result<(), RunnerError> {
    let idea = store
        .get_idea(&run.idea)?
        .ok_or_else(|| RunnerError::InvalidInput(format!("idea {} not found", run.idea)))?;
    let snap = verify_snapshot(store, &idea)
        .ok_or_else(|| RunnerError::Gate("Cannot resume: plan snapshot is missing.".to_string()))?;
    if !snap.verified {
        return Err(RunnerError::Gate(format!(
            "Cannot resume: plan snapshot verification failed ({}).",
            snap.error.unwrap_or_else(|| "modified".to_string())
        )));
    }
    if !run.plan_sha256.is_empty() && snap.sha256 != run.plan_sha256 {
        return Err(RunnerError::Gate(
            "Cannot resume: plan snapshot hash does not match run snapshot.".to_string(),
        ));
    }

    let settings = store.settings()?;
    let wf_path = store.run_dir(&run.id).join("workflow.yaml");
    let raw = load_workflow_file(&wf_path).map_err(|e| {
        RunnerError::Gate(format!("Cannot resume: run workflow copy invalid ({e})."))
    })?;
    if !run.workflow_sha256.is_empty() && raw.sha256 != run.workflow_sha256 {
        return Err(RunnerError::Gate(
            "Cannot resume: run workflow copy hash does not match.".to_string(),
        ));
    }
    validate(&raw, &settings).map_err(|diags| {
        let msgs: Vec<String> = diags.iter().map(ToString::to_string).collect();
        RunnerError::Gate(format!(
            "Cannot resume: run workflow copy failed validation ({}).",
            msgs.join("; ")
        ))
    })?;

    let ready_clis: Vec<&str> = LOGIN_SPECS
        .iter()
        .map(|s| s.cli)
        .filter(|cli| interactive_ready(cli, probe).ready)
        .collect();
    if ready_clis.is_empty() {
        let preferred = idea
            .orchestrator
            .as_ref()
            .map(|o| o.cli.as_str())
            .unwrap_or("claude");
        let reason = interactive_ready(preferred, probe)
            .reason
            .unwrap_or_else(|| "no CLI installed and signed in".to_string());
        return Err(RunnerError::Gate(format!(
            "Cannot resume: {preferred} is not ready ({reason})."
        )));
    }
    Ok(())
}

fn collect_resume_events(store: &FactoryStore, run: &Run) -> Vec<EventData> {
    let stop_path = stop_request_path(store, &run.id);
    if stop_path.exists() {
        let _ = fs::remove_file(&stop_path);
    }

    let mut evs = Vec::new();
    for st in &run.stages {
        if st.state == StageState::Running {
            evs.push(EventData::StageInterrupted {
                run: run.id.clone(),
                stage: st.name.clone(),
            });
        }
    }
    evs
}

pub fn resume_paused_run(
    store: &FactoryStore,
    run_id: &str,
    actor: &Actor,
) -> Result<Run, RunnerError> {
    resume_paused_run_with_probe(store, run_id, actor, &HostLoginProbe)
}

pub fn resume_paused_run_with_probe(
    store: &FactoryStore,
    run_id: &str,
    actor: &Actor,
    probe: &dyn LoginProbe,
) -> Result<Run, RunnerError> {
    if actor.kind != ActorKind::User {
        return Err(RunnerError::Approval(ApprovalError::AgentCaller));
    }
    if !is_valid_run_id(run_id) {
        return Err(RunnerError::InvalidInput(format!(
            "invalid run id: {run_id}"
        )));
    }
    let run = get_run(store, run_id)?
        .ok_or_else(|| RunnerError::Run(RunError::NotFound(format!("run {run_id}"))))?;
    if run.state != RunState::Paused {
        return Err(RunnerError::InvalidInput(format!(
            "run {run_id} is not paused"
        )));
    }

    verify_run_resumable(store, &run, probe)?;

    let mut evs = collect_resume_events(store, &run);
    evs.push(EventData::RunResumed {
        run: run_id.to_string(),
    });

    append_run_events(
        store,
        &run.idea.clone(),
        &run.id.clone(),
        Some(run),
        evs,
        actor,
    )
}

pub fn cancel_run(
    store: &FactoryStore,
    run_id: &str,
    confirm: &str,
    actor: &Actor,
    caller: &Caller<'_>,
) -> Result<CancelRunResult, RunnerError> {
    require_user(caller)?;
    if actor.kind != ActorKind::User {
        return Err(RunnerError::Approval(ApprovalError::AgentCaller));
    }
    if !is_valid_run_id(run_id) {
        return Err(RunnerError::InvalidInput(format!(
            "invalid run id: {run_id}"
        )));
    }
    if confirm != run_id {
        return Err(RunnerError::InvalidInput(format!(
            "confirmation mismatch: pass --confirm {run_id}"
        )));
    }

    let run = get_run(store, run_id)?
        .ok_or_else(|| RunnerError::Run(RunError::NotFound(format!("run {run_id}"))))?;
    if !matches!(
        run.state,
        RunState::Pending
            | RunState::Queued
            | RunState::Running
            | RunState::Paused
            | RunState::Review
    ) {
        return Err(RunnerError::InvalidInput(format!(
            "run {run_id} is in state {:?} and cannot be cancelled",
            run.state
        )));
    }

    let (stopped_jobs, mut evs) = interrupt_running_jobs(store, &run)?;
    evs.push(EventData::RunCancelled {
        run: run_id.to_string(),
    });

    let idea_id = run.idea.clone();
    let updated_run = append_run_events(store, &idea_id, run_id, Some(run), evs, actor)?;
    let updated_idea = store
        .get_idea(&idea_id)?
        .ok_or_else(|| RunnerError::InvalidInput(format!("idea {idea_id} not found")))?;

    Ok(CancelRunResult {
        run: updated_run,
        idea: updated_idea,
        stopped_jobs,
    })
}

pub fn cancel_idea(
    store: &FactoryStore,
    idea_id: &str,
    confirm: &str,
    actor: &Actor,
    caller: &Caller<'_>,
) -> Result<CancelIdeaResult, RunnerError> {
    require_user(caller)?;
    if actor.kind != ActorKind::User {
        return Err(RunnerError::Approval(ApprovalError::AgentCaller));
    }
    if !is_valid_idea_id(idea_id) {
        return Err(RunnerError::InvalidInput(format!(
            "invalid idea id: {idea_id}"
        )));
    }
    if confirm != idea_id {
        return Err(RunnerError::InvalidInput(format!(
            "confirmation mismatch: pass --confirm {idea_id}"
        )));
    }

    let idea = store
        .get_idea(idea_id)?
        .ok_or_else(|| RunnerError::InvalidInput(format!("idea not found: {idea_id}")))?;
    if idea.state.is_terminal() {
        return Err(RunnerError::InvalidInput(format!(
            "idea {idea_id} is already {}",
            idea.state.label()
        )));
    }

    let runs = list_runs(store, Some(idea_id))?;
    let mut cancelled_runs = Vec::new();
    let mut stopped_jobs = Vec::new();
    for run in runs {
        if matches!(
            run.state,
            RunState::Pending
                | RunState::Queued
                | RunState::Running
                | RunState::Paused
                | RunState::Review
        ) {
            let run_id = run.id.clone();
            let (stopped, mut evs) = interrupt_running_jobs(store, &run)?;
            stopped_jobs.extend(stopped);
            evs.push(EventData::RunCancelled {
                run: run_id.clone(),
            });
            let updated_run = append_run_events(store, idea_id, &run_id, Some(run), evs, actor)?;
            cancelled_runs.push(updated_run);
        }
    }

    let updated_idea = store.execute(
        idea_id,
        Command::Cancel {
            reason: "cancelled by user".to_string(),
        },
        actor,
    )?;

    Ok(CancelIdeaResult {
        idea: updated_idea,
        cancelled_runs,
        stopped_jobs,
    })
}

pub fn retry_paused_stage(
    store: &FactoryStore,
    run_id: &str,
    stage: &str,
    hypothesis: &str,
    actor: &Actor,
    caller: &Caller<'_>,
) -> Result<Run, RunnerError> {
    retry_paused_stage_with_probe(
        store,
        run_id,
        stage,
        hypothesis,
        actor,
        caller,
        &HostLoginProbe,
    )
}

pub fn retry_paused_stage_with_probe(
    store: &FactoryStore,
    run_id: &str,
    stage: &str,
    hypothesis: &str,
    actor: &Actor,
    caller: &Caller<'_>,
    probe: &dyn LoginProbe,
) -> Result<Run, RunnerError> {
    require_user(caller)?;
    if actor.kind != ActorKind::User {
        return Err(RunnerError::Approval(ApprovalError::AgentCaller));
    }
    if !is_valid_run_id(run_id) {
        return Err(RunnerError::InvalidInput(format!(
            "invalid run id: {run_id}"
        )));
    }
    let trimmed_hypothesis = hypothesis.trim();
    if trimmed_hypothesis.is_empty() {
        return Err(RunnerError::InvalidInput(
            "hypothesis must not be empty".to_string(),
        ));
    }
    let run = get_run(store, run_id)?
        .ok_or_else(|| RunnerError::Run(RunError::NotFound(format!("run {run_id}"))))?;
    if run.state != RunState::Paused {
        return Err(RunnerError::InvalidInput(format!(
            "run {run_id} is not paused"
        )));
    }
    if !run.stages.iter().any(|s| s.name == stage) {
        return Err(RunnerError::InvalidInput(format!(
            "stage {stage} not found in run {run_id}"
        )));
    }

    verify_run_resumable(store, &run, probe)?;

    let mut evs = collect_resume_events(store, &run);
    evs.push(EventData::StageRetryRequested {
        run: run_id.to_string(),
        stage: stage.to_string(),
        hypothesis: trimmed_hypothesis.to_string(),
    });
    evs.push(EventData::RunResumed {
        run: run_id.to_string(),
    });

    append_run_events(
        store,
        &run.idea.clone(),
        &run.id.clone(),
        Some(run),
        evs,
        actor,
    )
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeBudgetResult {
    pub run: Run,
    pub old: RunBudget,
    pub new: RunBudget,
}

pub fn change_run_budget(
    store: &FactoryStore,
    run_id: &str,
    max_cost_usd: Option<f64>,
    max_duration_secs: Option<u64>,
    max_retries: Option<u32>,
    actor: &Actor,
    caller: &Caller<'_>,
) -> Result<ChangeBudgetResult, RunnerError> {
    require_user(caller)?;
    if actor.kind != ActorKind::User {
        return Err(RunnerError::Approval(ApprovalError::AgentCaller));
    }
    if !is_valid_run_id(run_id) {
        return Err(RunnerError::InvalidInput(format!(
            "invalid run id: {run_id}"
        )));
    }
    if max_cost_usd.is_none() && max_duration_secs.is_none() && max_retries.is_none() {
        return Err(RunnerError::InvalidInput(
            "pass at least one of --max-cost, --max-duration, or --max-retries".to_string(),
        ));
    }
    let run = get_run(store, run_id)?
        .ok_or_else(|| RunnerError::Run(RunError::NotFound(format!("run {run_id}"))))?;
    let settings = store.settings()?;
    let old = RunBudget::from_run_or_settings(&run, &settings);
    let new = old.with_overrides(max_cost_usd, max_duration_secs, max_retries, &settings)?;

    let updated = append_run_events(
        store,
        &run.idea.clone(),
        &run.id.clone(),
        Some(run),
        vec![EventData::BudgetChanged {
            run: run_id.to_string(),
            old: old.clone().into(),
            new: new.clone().into(),
        }],
        actor,
    )?;

    Ok(ChangeBudgetResult {
        run: updated,
        old,
        new,
    })
}

pub fn count_run_running_jobs(store: &FactoryStore, run: &Run) -> usize {
    let jdir = jobs_dir(store, &run.id);
    let mut job_running = 0usize;
    let mut saw_job_dir = false;
    if jdir.is_dir()
        && let Ok(entries) = fs::read_dir(&jdir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            saw_job_dir = true;
            if matches!(read_job_state(&path), Ok(JobState::Running)) {
                job_running += 1;
            }
        }
    }
    let stage_running = run
        .stages
        .iter()
        .filter(|s| s.state == StageState::Running)
        .count();
    if saw_job_dir {
        job_running.max(stage_running)
    } else {
        stage_running
    }
}

pub fn count_workspace_running_jobs(store: &FactoryStore) -> usize {
    let Ok(runs) = list_runs(store, None) else {
        return 0;
    };
    runs.iter()
        .filter(|r| {
            !matches!(
                r.state,
                RunState::Cancelled | RunState::Succeeded | RunState::Failed | RunState::Delivered
            )
        })
        .map(|r| count_run_running_jobs(store, r))
        .sum()
}

pub fn idea_within_grace_period(
    idea: &crate::factory::model::Idea,
    grace_secs: u64,
    now: f64,
) -> bool {
    if grace_secs == 0 {
        return false;
    }
    let ready_at = idea
        .ready_at
        .or_else(|| idea.plan_snapshot.as_ref().map(|s| s.created))
        .unwrap_or(idea.updated);
    now < ready_at + grace_secs as f64
}

pub fn drive_workspace_step(
    store: &FactoryStore,
    workspace: &Path,
    executor: &mut dyn Executor,
) -> Result<(), RunnerError> {
    let settings = store.settings()?;
    let now = current_time();
    if let Ok(ideas) = store.list_ideas() {
        let sys = Actor {
            kind: ActorKind::System,
            id: "engine".to_string(),
        };
        for idea in ideas {
            if idea.state == crate::factory::model::IdeaState::ReadyForDevelopment
                && !idea_within_grace_period(&idea, settings.approval_grace_secs, now)
            {
                let pending = take_pending_run(store, &idea.id);
                let opts = match pending {
                    Some(cfg) => super::run::CreateRunOptions {
                        deploy_requested: cfg.deploy_requested,
                        workflow: cfg.workflow,
                        budget: cfg.budget,
                    },
                    None => super::run::CreateRunOptions::default(),
                };
                let _ = super::run::create_run(store, workspace, &idea.id, opts, &sys);
            }
        }
    }

    let mut runs = list_runs(store, None)?;
    runs.sort_by(|a, b| {
        a.created
            .partial_cmp(&b.created)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    for r in runs {
        if matches!(
            r.state,
            RunState::Queued | RunState::Running | RunState::Review
        ) {
            let _ = drive_run_step(store, workspace, &r.id, executor);
        }
    }
    Ok(())
}

pub fn run_loop(workspace: &Path, executor: &mut dyn Executor) -> Result<(), RunnerError> {
    let store = FactoryStore::new(workspace);
    if let Ok(lines) = super::recovery::run_startup_repairs(&store, workspace) {
        for line in lines {
            println!("{line}");
        }
    }
    let _lock = acquire_runner_lock(&store)?;
    loop {
        let _ = drive_workspace_step(&store, workspace, executor);
        thread::sleep(Duration::from_secs(2));
    }
}
