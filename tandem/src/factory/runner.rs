use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::Path;
use std::thread;
use std::time::Duration;

use crate::factory::FactoryError;
use crate::factory::broker::gate::{check, today_iso_date};
use crate::factory::events::{Event, append_locked, set_private_file_mode};
use crate::factory::executor::{Executor, StageJob, StageOutcome};
use crate::factory::machine::{self, EventData};
use crate::factory::model::{Actor, ActorKind, KNOWN_PROVIDERS, Run, RunState};
use crate::factory::routing::{RoutingContext, route};
use crate::factory::run::{RunError, get_run, list_runs};
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
    #[error(transparent)]
    Run(#[from] RunError),
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    #[error(transparent)]
    Factory(#[from] FactoryError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
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
    let appended = append_locked(&store.events_path(), &pending)?;
    let mut next_run = run_opt;
    let mut next_idea = store.get_idea(idea_id)?;
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

    let facts = RunFacts {
        ui_changed: TriState::No,
        deploy_requested: run.deploy_requested,
        approvals: Vec::new(),
    };
    let limits = SchedulerLimits::from_workflow_and_settings(&wf, &settings, &run.stages);
    let decision = next(&wf, &run.stages, &facts, &limits);
    let sys = Actor {
        kind: ActorKind::System,
        id: "runner".to_string(),
    };

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
    let r_ctx = RoutingContext {
        preferred_provider,
        is_ui_work: false,
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

    match outcome {
        StageOutcome::Succeeded => {
            append_run_events(
                store,
                &run.idea.clone(),
                &run.id.clone(),
                Some(run),
                vec![EventData::StageSucceeded {
                    run: run_id.to_string(),
                    stage: stage_name,
                }],
                &sys,
            )?;
        }
        StageOutcome::Failed { reason } => {
            let prev_reason = run
                .stages
                .iter()
                .find(|s| s.name == stage_name)
                .and_then(|s| s.rationale.as_deref())
                .map(|r| r.trim().to_lowercase());
            let norm = reason.trim().to_lowercase();
            let has_new_hypothesis = !norm.is_empty()
                && norm != "no structured result"
                && prev_reason.as_deref() != Some(norm.as_str());

            let run_after_fail = append_run_events(
                store,
                &run.idea.clone(),
                &run.id.clone(),
                Some(run),
                vec![EventData::StageFailed {
                    run: run_id.to_string(),
                    stage: stage_name.clone(),
                    reason: reason.clone(),
                }],
                &sys,
            )?;

            if !has_new_hypothesis {
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
    }

    Ok(())
}

pub fn resume_paused_run(
    store: &FactoryStore,
    run_id: &str,
    actor: &Actor,
) -> Result<Run, RunnerError> {
    let run = get_run(store, run_id)?
        .ok_or_else(|| RunnerError::Run(RunError::NotFound(format!("run {run_id}"))))?;
    if run.state != RunState::Paused {
        return Err(RunnerError::InvalidInput(format!(
            "run {run_id} is not paused"
        )));
    }
    append_run_events(
        store,
        &run.idea.clone(),
        &run.id.clone(),
        Some(run),
        vec![EventData::RunResumed {
            run: run_id.to_string(),
        }],
        actor,
    )
}

pub fn retry_paused_stage(
    store: &FactoryStore,
    run_id: &str,
    stage: &str,
    hypothesis: &str,
    actor: &Actor,
) -> Result<Run, RunnerError> {
    if hypothesis.trim().is_empty() {
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
    let updated = append_run_events(
        store,
        &run.idea.clone(),
        &run.id.clone(),
        Some(run),
        vec![EventData::RunResumed {
            run: run_id.to_string(),
        }],
        actor,
    )?;
    let _ = fs::metadata(store.run_dir(run_id));
    Ok(updated)
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
