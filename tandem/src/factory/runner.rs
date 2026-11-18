use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde::Serialize;

use crate::coordination::filelock::exclusive_lock;
use crate::factory::FactoryError;
use crate::factory::approval::{ApprovalError, ensure_user_caller, verify_snapshot};
use crate::factory::broker::gate::{check, today_iso_date};
use crate::factory::broker::job::{JobState, jobs_dir, read_job_state, set_job_state};
use crate::factory::events::{Event, append_locked, reject_if_symlink, set_private_file_mode};
use crate::factory::executor::{Executor, StageJob, StageOutcome, last_stage_provider_model};
use crate::factory::ids::{is_valid_idea_id, is_valid_run_id};
use crate::factory::machine::{self, Command, EventData};
use crate::factory::model::{Actor, ActorKind, Idea, KNOWN_PROVIDERS, Run, RunState, StageState};
use crate::factory::routing::{RoutingContext, route};
use crate::factory::run::{RunError, get_run, list_runs};
use crate::factory::stages::plan::SlicesManifest;
use crate::factory::store::{FactoryStore, current_time, write_atomic};
use crate::factory::workflow::parse::load_workflow_file;
use crate::factory::workflow::scheduler::{RunFacts, SchedulerLimits, TriState, next};
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

pub fn stop_request_path(store: &FactoryStore, run_id: &str) -> PathBuf {
    store.run_dir(run_id).join("stop.req")
}

pub fn is_stop_requested(store: &FactoryStore, run_id: &str) -> bool {
    stop_request_path(store, run_id).exists()
}

pub fn acquire_runner_lock(store: &FactoryStore) -> Result<File, RunnerError> {
    store.ensure()?;
    let path = store.root.join(".runner.lock");
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)?;
    set_private_file_mode(&path)?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(RunnerError::AlreadyRunning),
        Err(TryLockError::Error(e)) => Err(RunnerError::Io(e)),
    }
}

fn append_run_events(
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
    let appended = append_locked(&store.events_path(), &pending)?;
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
        }],
        &sys,
    )?;
    Ok(())
}

pub fn drive_run_step(
    store: &FactoryStore,
    workspace: &Path,
    run_id: &str,
    executor: &mut dyn Executor,
) -> Result<(), RunnerError> {
    let Some(mut run) = get_run(store, run_id)? else {
        return Ok(());
    };
    if !matches!(run.state, RunState::Queued | RunState::Running) {
        return Ok(());
    }

    let settings = store.settings()?;
    let wf_path = store.run_dir(run_id).join("workflow.yaml");
    let raw = load_workflow_file(&wf_path).map_err(|e| RunnerError::InvalidInput(e.to_string()))?;
    let mut wf = validate(&raw, &settings).map_err(|diags| {
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

    let is_ui = has_ui_slices(store, run_id);
    let facts = RunFacts {
        ui_changed: if is_ui { TriState::Yes } else { TriState::No },
        deploy_requested: run.deploy_requested,
        approvals: Vec::new(),
    };
    let limits = SchedulerLimits::from_workflow_and_settings(&wf, &settings, &run.stages);
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

    let stage_to_run = decision.start.first().cloned().or_else(|| {
        decision
            .failed
            .first()
            .filter(|f| !f.retries_exhausted)
            .map(|f| f.name.clone())
    });

    let Some(stage_name) = stage_to_run else {
        if let Some(blocked) = decision.blocked.first() {
            return pause_run(store, run, blocked.reason.clone(), vec![]);
        }
        return Ok(());
    };

    let today = today_iso_date();
    let open_providers: Vec<&str> = KNOWN_PROVIDERS
        .iter()
        .copied()
        .filter(|p| check(p, &settings, &today).is_ok())
        .collect();
    if open_providers.is_empty() {
        return pause_run(
            store,
            run,
            "gate closed: no provider consented and allowed".to_string(),
            vec![
                "tandem factory provider consent claude --auth api-key --confirm claude"
                    .to_string(),
            ],
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
    let next_attempt = current_attempts + 1;

    let preferred_provider = store
        .get_idea(&run.idea)?
        .and_then(|i| i.orchestrator)
        .map(|o| o.cli);
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
    };

    let routed = match route(
        stage_def,
        &wf,
        &settings,
        &open_providers,
        next_attempt,
        &r_ctx,
    ) {
        Ok(r) => r,
        Err(unroutable) => {
            return pause_run(store, run, unroutable.to_string(), vec![]);
        }
    };

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
    });
    run = append_run_events(
        store,
        &run.idea.clone(),
        &run.id.clone(),
        Some(run),
        pre_evs,
        &sys,
    )?;

    let outcome = executor.run(&StageJob {
        run: run.id.clone(),
        stage: stage_name.clone(),
        attempt: next_attempt,
    });

    let current_run = match get_run(store, run_id)? {
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
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<PauseRunResult, RunnerError> {
    ensure_user_caller(env)?;
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

fn verify_run_resumable(store: &FactoryStore, run: &Run) -> Result<(), RunnerError> {
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

    let today = today_iso_date();
    let open_providers: Vec<&str> = KNOWN_PROVIDERS
        .iter()
        .copied()
        .filter(|p| check(p, &settings, &today).is_ok())
        .collect();
    if open_providers.is_empty() {
        let preferred = idea
            .orchestrator
            .as_ref()
            .map(|o| o.cli.as_str())
            .unwrap_or("claude");
        let reason = match check(preferred, &settings, &today) {
            Ok(_) => "no provider consented and allowed".to_string(),
            Err(e) => e.to_string(),
        };
        return Err(RunnerError::Gate(format!(
            "Cannot resume: the {preferred} gate is closed ({reason})."
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

    verify_run_resumable(store, &run)?;

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
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<CancelRunResult, RunnerError> {
    ensure_user_caller(env)?;
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
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<CancelIdeaResult, RunnerError> {
    ensure_user_caller(env)?;
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
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Run, RunnerError> {
    ensure_user_caller(env)?;
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

    verify_run_resumable(store, &run)?;

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

pub fn run_loop(workspace: &Path, executor: &mut dyn Executor) -> Result<(), RunnerError> {
    let store = FactoryStore::new(workspace);
    let _lock = acquire_runner_lock(&store)?;
    loop {
        if let Ok(runs) = list_runs(&store, None) {
            for r in runs {
                if matches!(r.state, RunState::Queued | RunState::Running) {
                    let _ = drive_run_step(&store, workspace, &r.id, executor);
                }
            }
        }
        thread::sleep(Duration::from_secs(2));
    }
}
