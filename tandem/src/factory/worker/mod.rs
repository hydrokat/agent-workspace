pub mod argv;
pub mod watch;

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::adapters::base::{AdapterError, ToolFlag, WorkerLaunchRequest};
use crate::factory::broker::job::{JobState, correlation_id, job_dir, read_job_state};
use crate::factory::broker::result::JobResult;
use crate::factory::broker::spawn::build_env;
use crate::factory::events::set_private_file_mode;
use crate::factory::model::RunState;
use crate::factory::run::get_run;
use crate::factory::store::{FactoryStore, write_atomic};
use crate::tmux::SESSION;
use crate::tmux::client::{PaneKind, TmuxClient, TmuxError, ToSViolation};
use crate::tmux::tabs::parse_panes;

pub use argv::build_worker_command;
pub use watch::{DEFAULT_WATCH_INTERVAL, watch_with_interval};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerAssignment {
    pub cli: String,
    pub model: String,
    pub read_only: bool,
    pub tools: Vec<ToolFlag>,
    pub agent: String,
    pub role: String,
}

#[derive(Debug, Clone)]
pub struct WorkerLaunchSpec {
    pub run_id: String,
    pub stage: String,
    pub attempt: u32,
    pub job_dir: PathBuf,
    pub prompt_file: PathBuf,
    pub result_path: PathBuf,
    pub worktree_path: PathBuf,
    pub assignment: WorkerAssignment,
    pub env: BTreeMap<String, String>,
    pub time_limit_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchedWorker {
    pub pane_id: String,
    pub run_id: String,
    pub stage: String,
    pub attempt: u32,
    pub cli: String,
    pub model: String,
    pub job_dir: PathBuf,
    pub result_path: PathBuf,
    pub time_limit_secs: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkerWatchOutcome {
    Completed(JobResult),
    ExitedWithoutResult { detail: String },
    Interrupted,
    TimedOut { limit_secs: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanWorkerPane {
    pub pane_id: String,
    pub run_id: Option<String>,
    pub job_id: Option<String>,
    pub stage: Option<String>,
    pub cli: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error(transparent)]
    Adapter(#[from] AdapterError),
    #[error(transparent)]
    Tmux(#[from] TmuxError),
    #[error(transparent)]
    ToS(#[from] ToSViolation),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
}

fn resolve_git_exclude_path(worktree: &Path) -> Option<PathBuf> {
    let dot_git = worktree.join(".git");
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else if dot_git.is_file() {
        let raw = fs::read_to_string(&dot_git).ok()?;
        let rel = raw.strip_prefix("gitdir:")?.trim();
        let cand = PathBuf::from(rel);
        if cand.is_absolute() {
            cand
        } else {
            worktree.join(cand)
        }
    } else {
        return None;
    };
    Some(git_dir.join("info").join("exclude"))
}

fn ensure_claude_stop_hook(worktree: &Path) -> Result<(), std::io::Error> {
    let claude_dir = worktree.join(".claude");
    fs::create_dir_all(&claude_dir)?;
    let settings_path = claude_dir.join("settings.local.json");
    let hook_cmd = r#"sh -c 'if [ -n "${TANDEM_JOB_DIR:-}" ] && [ -d "$TANDEM_JOB_DIR" ]; then if [ ! -t 0 ]; then cat >/dev/null 2>&1 || true; fi; ts=$(date +%s); stage=${TANDEM_STAGE:-unknown}; printf "{\"last_stop_at\":%s,\"state\":\"idle\",\"cli\":\"claude\",\"stage\":\"%s\"}\n" "$ts" "$stage" > "$TANDEM_JOB_DIR/activity.json.tmp" && mv "$TANDEM_JOB_DIR/activity.json.tmp" "$TANDEM_JOB_DIR/activity.json"; fi'"#;
    let mut root: serde_json::Value = if settings_path.is_file() {
        fs::read_to_string(&settings_path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_else(|| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    if !root.is_object() {
        root = serde_json::json!({});
    }
    let hooks_obj = root
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    if !hooks_obj.is_object() {
        *hooks_obj = serde_json::json!({});
    }
    hooks_obj.as_object_mut().unwrap().insert(
        "Stop".to_string(),
        serde_json::json!([
            {
                "hooks": [
                    {
                        "type": "command",
                        "command": hook_cmd
                    }
                ]
            }
        ]),
    );
    let body = serde_json::to_string_pretty(&root).map_err(std::io::Error::other)?;
    write_atomic(&settings_path, format!("{body}\n").as_bytes())
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    if let Some(exclude_path) = resolve_git_exclude_path(worktree) {
        if let Some(parent) = exclude_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let existing = fs::read_to_string(&exclude_path).unwrap_or_default();
        let entry = ".claude/settings.local.json";
        if !existing.lines().any(|l| l.trim() == entry) {
            let mut next = existing;
            if !next.is_empty() && !next.ends_with('\n') {
                next.push('\n');
            }
            next.push_str(entry);
            next.push('\n');
            let _ = write_atomic(&exclude_path, next.as_bytes());
        }
    }
    Ok(())
}

pub fn launch(tmux: &TmuxClient, spec: &WorkerLaunchSpec) -> Result<LaunchedWorker, WorkerError> {
    if spec.assignment.cli == "claude" && spec.worktree_path.is_dir() {
        let _ = ensure_claude_stop_hook(&spec.worktree_path);
    }
    let req = WorkerLaunchRequest {
        model: spec.assignment.model.clone(),
        tools: spec.assignment.tools.clone(),
        read_only: spec.assignment.read_only,
        prompt_file: spec.prompt_file.clone(),
    };
    let cmd_spec = build_worker_command(&spec.assignment.cli, &req)?;

    let job_id = correlation_id(&spec.run_id, &spec.stage, spec.attempt);
    let mut factory_env = BTreeMap::new();
    factory_env.insert("TANDEM_FACTORY".to_string(), "1".to_string());
    factory_env.insert("TANDEM_RUN_ID".to_string(), spec.run_id.clone());
    factory_env.insert("TANDEM_STAGE".to_string(), spec.stage.clone());
    factory_env.insert("TANDEM_ATTEMPT".to_string(), spec.attempt.to_string());
    factory_env.insert(
        "TANDEM_JOB_DIR".to_string(),
        spec.job_dir.to_string_lossy().into_owned(),
    );
    factory_env.insert("TANDEM_ROLE".to_string(), spec.assignment.role.clone());
    factory_env.insert("TANDEM_CLI".to_string(), spec.assignment.cli.clone());

    let mut parent_env = spec.env.clone();
    for (k, v) in &cmd_spec.env {
        parent_env.insert(k.clone(), v.clone());
    }
    let child_env = build_env(&spec.assignment.cli, &parent_env, &factory_env);

    let window_name = format!("fw-{}-{}", spec.run_id, spec.stage);
    let cwd = spec.worktree_path.to_string_lossy().into_owned();
    let mut launch_argv: Vec<String> = vec![
        "new-window".to_string(),
        "-P".to_string(),
        "-F".to_string(),
        "#{pane_id}".to_string(),
        "-t".to_string(),
        SESSION.to_string(),
        "-n".to_string(),
        window_name,
        "-c".to_string(),
        cwd,
    ];
    if !child_env.is_empty() {
        launch_argv.push("env".to_string());
        for (k, v) in &child_env {
            launch_argv.push(format!("{k}={v}"));
        }
    }
    launch_argv.extend(cmd_spec.argv);

    let arg_refs: Vec<&str> = launch_argv.iter().map(String::as_str).collect();
    let pane_id = tmux.run_checked(&arg_refs)?.trim().to_string();
    if pane_id.is_empty() {
        return Err(WorkerError::Invalid(
            "tmux new-window returned empty pane id".to_string(),
        ));
    }

    let attempt_str = spec.attempt.to_string();
    let label = format!("{}·{}", spec.stage, spec.attempt);
    let role = if spec.assignment.role.is_empty() {
        "factory-worker"
    } else {
        spec.assignment.role.as_str()
    };
    let _ = tmux.run(&["set-option", "-p", "-t", &pane_id, "remain-on-exit", "on"]);
    for (opt, val) in [
        ("@tandem_kind", "factory-worker"),
        ("@tandem_role", role),
        ("@tandem_index", attempt_str.as_str()),
        ("@tandem_label", label.as_str()),
        ("@tandem_run", spec.run_id.as_str()),
        ("@tandem_job", job_id.as_str()),
        ("@tandem_stage", spec.stage.as_str()),
        ("@tandem_cli", spec.assignment.cli.as_str()),
        ("@tandem_model", spec.assignment.model.as_str()),
    ] {
        if let Err(err) = tmux.run_checked(&["set-option", "-p", "-t", &pane_id, opt, val]) {
            let msg = err.to_string();
            if !msg.contains("no such pane") && !msg.contains("can't find pane") {
                return Err(err.into());
            }
            break;
        }
    }

    fs::create_dir_all(&spec.job_dir)?;
    let pane_file = spec.job_dir.join("pane_id");
    let _ = write_atomic(&pane_file, format!("{pane_id}\n").as_bytes());
    let _ = set_private_file_mode(&pane_file);

    Ok(LaunchedWorker {
        pane_id,
        run_id: spec.run_id.clone(),
        stage: spec.stage.clone(),
        attempt: spec.attempt,
        cli: spec.assignment.cli.clone(),
        model: spec.assignment.model.clone(),
        job_dir: spec.job_dir.clone(),
        result_path: spec.result_path.clone(),
        time_limit_secs: spec.time_limit_secs,
    })
}

pub fn watch(
    tmux: &TmuxClient,
    worker: &LaunchedWorker,
    stop_requested: &dyn Fn() -> bool,
) -> Result<WorkerWatchOutcome, WorkerError> {
    watch_with_interval(tmux, worker, stop_requested, DEFAULT_WATCH_INTERVAL)
}

pub fn finish(
    tmux: &TmuxClient,
    worker: &LaunchedWorker,
    outcome: WorkerWatchOutcome,
) -> Result<JobResult, WorkerError> {
    let _ = tmux.kill_pane(&worker.pane_id);
    match outcome {
        WorkerWatchOutcome::Completed(result) => Ok(result),
        WorkerWatchOutcome::ExitedWithoutResult { detail } => Err(WorkerError::Invalid(detail)),
        WorkerWatchOutcome::Interrupted => Err(WorkerError::Io(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "interrupted",
        ))),
        WorkerWatchOutcome::TimedOut { limit_secs } => Err(WorkerError::Invalid(format!(
            "worker timed out after {limit_secs}s"
        ))),
    }
}

pub fn find_orphaned_worker_panes(
    tmux: &TmuxClient,
    store: &FactoryStore,
    runner_live: bool,
) -> Vec<OrphanWorkerPane> {
    let Ok(res) = tmux.run(&["list-panes", "-a", "-F", crate::tmux::tabs::FORMAT]) else {
        return Vec::new();
    };
    if res.code != 0 {
        return Vec::new();
    }

    let mut seen_panes = HashSet::new();
    let mut orphans = Vec::new();
    for pane in parse_panes(&res.out) {
        if PaneKind::from_tag(&pane.kind) != PaneKind::FactoryWorker {
            continue;
        }
        if !seen_panes.insert(pane.id.clone()) {
            continue;
        }
        let is_live_job = if !runner_live || pane.run.is_empty() || pane.stage.is_empty() {
            false
        } else {
            let run_ok = matches!(
                get_run(store, &pane.run),
                Ok(Some(r)) if r.state == RunState::Running
            );
            if !run_ok {
                false
            } else if let Some((_, attempt_str)) = pane.job.rsplit_once('.')
                && let Ok(attempt) = attempt_str.parse::<u32>()
            {
                let jdir = job_dir(store, &pane.run, &pane.stage, attempt);
                matches!(read_job_state(&jdir), Ok(JobState::Running))
            } else {
                false
            }
        };
        if !is_live_job {
            orphans.push(OrphanWorkerPane {
                pane_id: pane.id,
                run_id: (!pane.run.is_empty()).then_some(pane.run),
                job_id: (!pane.job.is_empty()).then_some(pane.job),
                stage: (!pane.stage.is_empty()).then_some(pane.stage),
                cli: (!pane.cli.is_empty()).then_some(pane.cli),
            });
        }
    }
    orphans
}

pub fn sleep_poll_hint() -> Duration {
    DEFAULT_WATCH_INTERVAL
}
