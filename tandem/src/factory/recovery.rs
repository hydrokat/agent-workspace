use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::FactoryError;
use super::approval::{ApprovalError, Caller, require_user};
use super::audit;
use super::broker::job::{JobState, jobs_dir, read_job_state, set_job_state};
use super::broker::result::parse_and_validate_result_strict;
use super::events::{self};
use super::ids::is_valid_run_id;
use super::machine::EventData;
use super::model::{Actor, ActorKind, Run, RunState, StageState};
use super::run::{RunError, get_run};
use super::runner::append_run_events;
use super::store::{FactoryStore, current_time, write_atomic};
use super::workflow::DEFAULT_WORKFLOW_YAML;
use super::workspace::{
    WorkspaceError, find_orphan_branches, find_orphan_worktrees, remove_orphan_worktree,
};
use crate::tmux::client::TmuxClient;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Ok,
    Fixed,
    Warn,
    Info,
}

impl FindingSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Fixed => "fixed",
            Self::Warn => "warn",
            Self::Info => "info",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProblemKind {
    EventLogOk,
    StaleRunnerLock,
    InterruptedRunningJob,
    HalfCreatedRun,
    MaterializedDrift,
    TruncatedLogLine,
    HashChainBreak,
    OrphanWorktree,
    OrphanBranch,
    OrphanWorkerPane,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrphanWorkerPaneInfo {
    pub pane_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli: Option<String>,
    pub killed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrphanWorktreeInfo {
    pub run_id: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_state: Option<String>,
    pub uncommitted_files: usize,
    pub clean: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub target_repo: String,
    pub removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrphanBranchInfo {
    pub branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_state: Option<String>,
    pub target_repo: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorFinding {
    pub severity: FindingSeverity,
    pub kind: ProblemKind,
    pub subject: String,
    pub message: String,
    pub repaired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorReport {
    pub findings: Vec<DoctorFinding>,
    pub orphan_worktrees: Vec<OrphanWorktreeInfo>,
    pub orphan_branches: Vec<OrphanBranchInfo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub orphan_worker_panes: Vec<OrphanWorkerPaneInfo>,
    pub safe_repairs_applied: usize,
    pub worktrees_removed: usize,
    pub truncated_log: bool,
    pub chain_intact: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error(transparent)]
    Approval(#[from] ApprovalError),
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    #[error(transparent)]
    Factory(#[from] FactoryError),
    #[error(transparent)]
    Run(#[from] RunError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Message(String),
}

fn runner_lock_path(store: &FactoryStore) -> std::path::PathBuf {
    store.root.join(".runner.lock")
}

fn is_pid_alive(pid: u32) -> bool {
    pid != 0 && crate::web_manager::is_pid_alive(pid)
}

pub fn is_runner_live(store: &FactoryStore) -> bool {
    let lock_path = runner_lock_path(store);
    if !lock_path.exists() {
        return false;
    }
    let Ok(file) = OpenOptions::new().read(true).write(true).open(&lock_path) else {
        return false;
    };
    match file.try_lock() {
        Ok(()) => {
            drop(file);
            false
        }
        Err(std::fs::TryLockError::WouldBlock) => true,
        Err(_) => false,
    }
}

pub fn is_runner_lock_stale(store: &FactoryStore) -> bool {
    let lock_path = runner_lock_path(store);
    if !lock_path.exists() {
        return false;
    }
    if is_runner_live(store) {
        return false;
    }
    match fs::read_to_string(&lock_path) {
        Ok(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return true;
            }
            match trimmed.parse::<u32>() {
                Ok(pid) => !is_pid_alive(pid),
                Err(_) => true,
            }
        }
        Err(_) => true,
    }
}

fn parse_job_dir_name(dir_name: &str, run_id: &str) -> Option<(String, u32)> {
    let rest = dir_name.strip_prefix(&format!("{run_id}."))?;
    let (stage, attempt_str) = rest.rsplit_once('.')?;
    let attempt = attempt_str.parse::<u32>().ok()?;
    Some((stage.to_string(), attempt))
}

fn list_run_dir_ids(store: &FactoryStore) -> Result<Vec<String>, FactoryError> {
    let runs_dir = store.runs_dir();
    if !runs_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut ids = Vec::new();
    for entry in fs::read_dir(&runs_dir)? {
        let entry = entry?;
        if !entry.path().is_dir() {
            continue;
        }
        if let Some(name) = entry.file_name().to_str()
            && is_valid_run_id(name)
        {
            ids.push(name.to_string());
        }
    }
    ids.sort();
    Ok(ids)
}

fn collect_running_jobs_without_runner(
    store: &FactoryStore,
) -> Result<Vec<(String, String, u32, std::path::PathBuf)>, FactoryError> {
    if is_runner_live(store) {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for run_id in list_run_dir_ids(store)? {
        let j_root = jobs_dir(store, &run_id);
        if !j_root.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&j_root)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if matches!(read_job_state(&path), Ok(JobState::Running))
                && let Some(name) = entry.file_name().to_str()
                && let Some((stage, attempt)) = parse_job_dir_name(name, &run_id)
            {
                out.push((run_id.clone(), stage, attempt, path));
            }
        }
    }
    Ok(out)
}

fn collect_half_created_runs(store: &FactoryStore) -> Result<Vec<String>, FactoryError> {
    let mut out = Vec::new();
    for run_id in list_run_dir_ids(store)? {
        let dir = store.run_dir(&run_id);
        let has_run_json = dir.join("run.json").is_file();
        let has_workflow = dir.join("workflow.yaml").is_file();
        if !has_run_json || !has_workflow {
            if has_run_json
                && let Ok(Some(run)) = get_run(store, &run_id)
                && matches!(
                    run.state,
                    RunState::Failed | RunState::Cancelled | RunState::Delivered
                )
            {
                continue;
            }
            out.push(run_id);
        }
    }
    Ok(out)
}

fn append_log_and_orphan_findings(
    store: &FactoryStore,
    workspace: &Path,
    findings: &mut Vec<DoctorFinding>,
) -> Result<(Vec<OrphanWorktreeInfo>, Vec<OrphanBranchInfo>, bool, bool), RecoveryError> {
    let events_path = store.events_path();
    let read_res = events::read_all(&events_path)?;
    let verify_res = audit::verify(&events_path)?;

    if read_res.truncated {
        findings.push(DoctorFinding {
            severity: FindingSeverity::Warn,
            kind: ProblemKind::TruncatedLogLine,
            subject: "event log".to_string(),
            message: "last log line is half written; new appends continue from last complete line"
                .to_string(),
            repaired: false,
        });
    }
    if !verify_res.intact {
        let detail = verify_res
            .first_break
            .as_ref()
            .map(|b| format!("hash chain break at seq {}: {}", b.seq, b.reason))
            .unwrap_or_else(|| "hash chain break detected".to_string());
        findings.push(DoctorFinding {
            severity: FindingSeverity::Warn,
            kind: ProblemKind::HashChainBreak,
            subject: "event log".to_string(),
            message: detail,
            repaired: false,
        });
    } else if !read_res.truncated {
        let chain_info = verify_res
            .chain_start_seq
            .map(|s| {
                format!(
                    "{} events, chain verified from {s}",
                    verify_res.total_events
                )
            })
            .unwrap_or_else(|| format!("{} events, chain intact", verify_res.total_events));
        findings.push(DoctorFinding {
            severity: FindingSeverity::Ok,
            kind: ProblemKind::EventLogOk,
            subject: "event log".to_string(),
            message: chain_info,
            repaired: false,
        });
    }

    let orphan_worktrees = find_orphan_worktrees(store, workspace)?;
    for wt in &orphan_worktrees {
        let state_label = wt.run_state.as_deref().unwrap_or("unknown");
        let msg = if wt.clean {
            format!("{} (run {state_label}, clean)   remove with --fix", wt.path)
        } else {
            format!(
                "{} (run {state_label}, {} uncommitted files)   kept",
                wt.path, wt.uncommitted_files
            )
        };
        findings.push(DoctorFinding {
            severity: FindingSeverity::Warn,
            kind: ProblemKind::OrphanWorktree,
            subject: "orphan worktree".to_string(),
            message: msg,
            repaired: false,
        });
    }

    let orphan_branches = find_orphan_branches(store, workspace)?;
    for br in &orphan_branches {
        let state_label = br.run_state.as_deref().unwrap_or("unknown");
        findings.push(DoctorFinding {
            severity: FindingSeverity::Info,
            kind: ProblemKind::OrphanBranch,
            subject: "orphan branch".to_string(),
            message: format!(
                "{} (run {state_label})   never deleted by tandem",
                br.branch
            ),
            repaired: false,
        });
    }

    Ok((
        orphan_worktrees,
        orphan_branches,
        read_res.truncated,
        verify_res.intact,
    ))
}

pub fn diagnose(store: &FactoryStore, workspace: &Path) -> Result<DoctorReport, RecoveryError> {
    diagnose_with_tmux(
        store,
        workspace,
        &TmuxClient::new(crate::tmux::socket_for(workspace), None),
    )
}

pub fn diagnose_with_tmux(
    store: &FactoryStore,
    workspace: &Path,
    tmux: &TmuxClient,
) -> Result<DoctorReport, RecoveryError> {
    let mut findings = Vec::new();

    if is_runner_lock_stale(store) {
        findings.push(DoctorFinding {
            severity: FindingSeverity::Warn,
            kind: ProblemKind::StaleRunnerLock,
            subject: "runner lock".to_string(),
            message: "stale .runner.lock (process dead)".to_string(),
            repaired: false,
        });
    }

    for (run_id, stage, attempt, _) in collect_running_jobs_without_runner(store)? {
        findings.push(DoctorFinding {
            severity: FindingSeverity::Warn,
            kind: ProblemKind::InterruptedRunningJob,
            subject: format!("job {run_id}.{stage}.{attempt}"),
            message: "running with no live runner".to_string(),
            repaired: false,
        });
    }

    for run_id in collect_half_created_runs(store)? {
        findings.push(DoctorFinding {
            severity: FindingSeverity::Warn,
            kind: ProblemKind::HalfCreatedRun,
            subject: format!("run {run_id}"),
            message: "missing run.json or workflow.yaml".to_string(),
            repaired: false,
        });
    }

    if store.materialized_differs_from_log()? {
        findings.push(DoctorFinding {
            severity: FindingSeverity::Warn,
            kind: ProblemKind::MaterializedDrift,
            subject: "materialized state".to_string(),
            message: "differs from event log".to_string(),
            repaired: false,
        });
    }

    let runner_live = is_runner_live(store);
    let orphan_worker_panes: Vec<OrphanWorkerPaneInfo> =
        super::worker::find_orphaned_worker_panes(tmux, store, runner_live)
            .into_iter()
            .map(|p| {
                findings.push(DoctorFinding {
                    severity: FindingSeverity::Warn,
                    kind: ProblemKind::OrphanWorkerPane,
                    subject: format!("worker pane {}", p.pane_id),
                    message: format!(
                        "orphaned worker pane (run {}, job {})",
                        p.run_id.as_deref().unwrap_or("unknown"),
                        p.job_id.as_deref().unwrap_or("unknown")
                    ),
                    repaired: false,
                });
                OrphanWorkerPaneInfo {
                    pane_id: p.pane_id,
                    run_id: p.run_id,
                    job_id: p.job_id,
                    stage: p.stage,
                    cli: p.cli,
                    killed: false,
                }
            })
            .collect();

    let (orphan_worktrees, orphan_branches, truncated_log, chain_intact) =
        append_log_and_orphan_findings(store, workspace, &mut findings)?;

    Ok(DoctorReport {
        findings,
        orphan_worktrees,
        orphan_branches,
        orphan_worker_panes,
        safe_repairs_applied: 0,
        worktrees_removed: 0,
        truncated_log,
        chain_intact,
    })
}

fn restore_workflow_if_missing(store: &FactoryStore, run: &Run) -> Result<bool, FactoryError> {
    let wf_dest = store.run_dir(&run.id).join("workflow.yaml");
    if wf_dest.is_file() {
        return Ok(true);
    }
    let wf_dir_candidate = store
        .root
        .join("workflows")
        .join(format!("{}.yaml", run.workflow));
    if wf_dir_candidate.is_file() {
        let bytes = fs::read(&wf_dir_candidate)?;
        write_atomic(&wf_dest, &bytes)?;
        return Ok(true);
    }
    if run.workflow == "default" || run.workflow == "default-delivery" {
        write_atomic(&wf_dest, DEFAULT_WORKFLOW_YAML.as_bytes())?;
        return Ok(true);
    }
    Ok(false)
}

fn mark_half_created_run_failed(
    store: &FactoryStore,
    run_id: &str,
    reason: &str,
) -> Result<(), RecoveryError> {
    let dir = store.run_dir(run_id);
    if !dir.is_dir() {
        fs::create_dir_all(&dir)?;
    }
    let existing = get_run(store, run_id)?;
    let now = current_time();
    let failed_run = match existing {
        Some(mut r) => {
            r.state = RunState::Failed;
            r.pause_reason = Some(reason.to_string());
            r.updated = now;
            r
        }
        None => Run {
            id: run_id.to_string(),
            idea: String::new(),
            workflow: String::new(),
            workflow_version: String::new(),
            workflow_sha256: String::new(),
            workflow_source: String::new(),
            plan_sha256: String::new(),
            deploy_requested: false,
            target: None,
            branch: None,
            base_commit: None,
            worktree_path: None,
            pause_reason: Some(reason.to_string()),
            action_commands: Vec::new(),
            baseline_passed: None,
            state: RunState::Failed,
            stages: Vec::new(),
            evidence: Vec::new(),
            prs: Vec::new(),
            budget: None,
            cost: None,
            created: now,
            updated: now,
        },
    };
    let bytes = serde_json::to_vec_pretty(&failed_run)?;
    write_atomic(&dir.join("run.json"), &bytes)?;
    Ok(())
}

pub fn run_safe_repairs(
    store: &FactoryStore,
    workspace: &Path,
) -> Result<DoctorReport, RecoveryError> {
    run_safe_repairs_with_tmux(
        store,
        workspace,
        &TmuxClient::new(crate::tmux::socket_for(workspace), None),
    )
}

pub fn run_safe_repairs_with_tmux(
    store: &FactoryStore,
    workspace: &Path,
    tmux: &TmuxClient,
) -> Result<DoctorReport, RecoveryError> {
    let mut findings = Vec::new();
    let mut safe_repairs_applied = 0usize;

    if is_runner_lock_stale(store) {
        let lock_path = runner_lock_path(store);
        if lock_path.exists() {
            fs::remove_file(&lock_path)?;
            safe_repairs_applied += 1;
            findings.push(DoctorFinding {
                severity: FindingSeverity::Fixed,
                kind: ProblemKind::StaleRunnerLock,
                subject: "runner lock".to_string(),
                message: "removed stale .runner.lock".to_string(),
                repaired: true,
            });
        }
    }

    if store.materialized_differs_from_log()? {
        store.rebuild()?;
        safe_repairs_applied += 1;
        findings.push(DoctorFinding {
            severity: FindingSeverity::Fixed,
            kind: ProblemKind::MaterializedDrift,
            subject: "materialized state".to_string(),
            message: "rebuilt materialized files from event log".to_string(),
            repaired: true,
        });
    }

    let half_created = collect_half_created_runs(store)?;
    if !half_created.is_empty() {
        let replayed = store.replay_all()?;
        for run_id in half_created {
            let dir = store.run_dir(&run_id);
            let run_json = dir.join("run.json");
            if !run_json.is_file()
                && let Some(run) = replayed.runs.get(&run_id)
            {
                let bytes = serde_json::to_vec_pretty(run)?;
                write_atomic(&run_json, &bytes)?;
            }

            let msg = if let Some(run) = get_run(store, &run_id)? {
                if restore_workflow_if_missing(store, &run)? {
                    "rebuilt missing run files from event log".to_string()
                } else {
                    let reason = "half-created run: missing workflow.yaml";
                    mark_half_created_run_failed(store, &run_id, reason)?;
                    format!("marked run failed ({reason})")
                }
            } else {
                let reason = "half-created run: missing RunCreated event in log";
                mark_half_created_run_failed(store, &run_id, reason)?;
                format!("marked run failed ({reason})")
            };
            safe_repairs_applied += 1;
            findings.push(DoctorFinding {
                severity: FindingSeverity::Fixed,
                kind: ProblemKind::HalfCreatedRun,
                subject: format!("run {run_id}"),
                message: msg,
                repaired: true,
            });
        }
    }

    let sys = Actor {
        kind: ActorKind::System,
        id: "doctor".to_string(),
    };
    for (run_id, stage, attempt, jdir) in collect_running_jobs_without_runner(store)? {
        let result_path = jdir.join("result.json");
        let has_valid_result = result_path.is_file()
            && parse_and_validate_result_strict(&result_path, attempt > 1).is_ok();

        if has_valid_result {
            set_job_state(&jdir, JobState::Finished)
                .map_err(|e| RecoveryError::Message(e.to_string()))?;
            safe_repairs_applied += 1;
            findings.push(DoctorFinding {
                severity: FindingSeverity::Fixed,
                kind: ProblemKind::InterruptedRunningJob,
                subject: format!("job {run_id}.{stage}.{attempt}"),
                message: "had stored result.json; marked finished for resume".to_string(),
                repaired: true,
            });
        } else {
            set_job_state(&jdir, JobState::Interrupted)
                .map_err(|e| RecoveryError::Message(e.to_string()))?;
            if let Some(run) = get_run(store, &run_id)?
                && run
                    .stages
                    .iter()
                    .any(|s| s.name == stage && s.state == StageState::Running)
            {
                let _ = append_run_events(
                    store,
                    &run.idea.clone(),
                    &run.id.clone(),
                    Some(run),
                    vec![EventData::StageInterrupted {
                        run: run_id.clone(),
                        stage: stage.clone(),
                    }],
                    &sys,
                );
            }
            safe_repairs_applied += 1;
            findings.push(DoctorFinding {
                severity: FindingSeverity::Fixed,
                kind: ProblemKind::InterruptedRunningJob,
                subject: format!("job {run_id}.{stage}.{attempt}"),
                message: "was running with no runner; marked interrupted (resume the run)"
                    .to_string(),
                repaired: true,
            });
        }
    }

    let runner_live = is_runner_live(store);
    let mut orphan_worker_panes = Vec::new();
    for p in super::worker::find_orphaned_worker_panes(tmux, store, runner_live) {
        let killed = tmux.kill_pane(&p.pane_id).is_ok();
        if killed {
            safe_repairs_applied += 1;
            findings.push(DoctorFinding {
                severity: FindingSeverity::Fixed,
                kind: ProblemKind::OrphanWorkerPane,
                subject: format!("worker pane {}", p.pane_id),
                message: format!(
                    "killed orphaned worker pane (run {}, job {})",
                    p.run_id.as_deref().unwrap_or("unknown"),
                    p.job_id.as_deref().unwrap_or("unknown")
                ),
                repaired: true,
            });
        }
        orphan_worker_panes.push(OrphanWorkerPaneInfo {
            pane_id: p.pane_id,
            run_id: p.run_id,
            job_id: p.job_id,
            stage: p.stage,
            cli: p.cli,
            killed,
        });
    }

    let (orphan_worktrees, orphan_branches, truncated_log, chain_intact) =
        append_log_and_orphan_findings(store, workspace, &mut findings)?;

    Ok(DoctorReport {
        findings,
        orphan_worktrees,
        orphan_branches,
        orphan_worker_panes,
        safe_repairs_applied,
        worktrees_removed: 0,
        truncated_log,
        chain_intact,
    })
}

pub fn run_startup_repairs(
    store: &FactoryStore,
    workspace: &Path,
) -> Result<Vec<String>, RecoveryError> {
    if !store.exists() {
        return Ok(Vec::new());
    }
    let report = run_safe_repairs(store, workspace)?;
    Ok(report
        .findings
        .into_iter()
        .filter(|f| f.severity == FindingSeverity::Fixed)
        .map(|f| format!("{:<7} {:<23} {}", f.severity.as_str(), f.subject, f.message))
        .collect())
}

pub fn doctor_fix(
    store: &FactoryStore,
    workspace: &Path,
    caller: &Caller<'_>,
    stdin: &mut dyn Read,
    prompt_out: &mut dyn Write,
) -> Result<DoctorReport, RecoveryError> {
    doctor_fix_with_tmux(
        store,
        workspace,
        caller,
        stdin,
        prompt_out,
        &TmuxClient::new(crate::tmux::socket_for(workspace), None),
    )
}

pub fn doctor_fix_with_tmux(
    store: &FactoryStore,
    workspace: &Path,
    caller: &Caller<'_>,
    stdin: &mut dyn Read,
    prompt_out: &mut dyn Write,
    tmux: &TmuxClient,
) -> Result<DoctorReport, RecoveryError> {
    require_user(caller)?;
    let mut report = run_safe_repairs_with_tmux(store, workspace, tmux)?;

    let mut reader = BufReader::new(stdin);
    for wt in &mut report.orphan_worktrees {
        if !wt.clean {
            continue;
        }
        let _ = write!(
            prompt_out,
            "Remove {}? It has no uncommitted changes. Type its run id to confirm: ",
            wt.path
        );
        let _ = prompt_out.flush();

        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            continue;
        }
        if line.trim() == wt.run_id {
            remove_orphan_worktree(workspace, wt)?;
            wt.removed = true;
            report.worktrees_removed += 1;
            if let Some(ref branch) = wt.branch {
                let _ = writeln!(prompt_out, "Removed. The branch {branch} is kept.");
            } else {
                let _ = writeln!(prompt_out, "Removed.");
            }
        } else {
            let _ = writeln!(prompt_out, "Skipped {}.", wt.path);
        }
    }

    Ok(report)
}

pub fn format_doctor_report(report: &DoctorReport) -> String {
    let mut lines = Vec::new();
    for f in &report.findings {
        lines.push(format!(
            "{:<7} {:<23} {}",
            f.severity.as_str(),
            f.subject,
            f.message
        ));
    }
    lines.join("\n")
}
