use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::context::{AssembledContext, ContextManifest};
use super::result::{JobResult, parse_and_validate_result_strict};
use crate::factory::FactoryError;
use crate::factory::events::{reject_if_symlink, set_private_file_mode};
use crate::factory::ids::is_valid_run_id;
use crate::factory::redact::RedactContext;
use crate::factory::store::{FactoryStore, create_private_dir, write_atomic};

pub const MAX_INTERRUPTIONS_PER_STAGE: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Prepared,
    Running,
    Finished,
    Interrupted,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Running => "running",
            Self::Finished => "finished",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "prepared" => Some(Self::Prepared),
            "running" => Some(Self::Running),
            "finished" => Some(Self::Finished),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSpec {
    pub run: String,
    pub stage: String,
    pub attempt: u32,
    pub agent: String,
    pub provider: String,
    pub model: String,
    pub tool_grants: Vec<String>,
    pub cwd: String,
    pub time_limit_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobInput {
    pub correlation_id: String,
    pub run: String,
    pub stage: String,
    pub attempt: u32,
    pub agent: String,
    pub provider: String,
    pub model: String,
    pub tool_grants: Vec<String>,
    pub cwd: String,
    pub time_limit_secs: u64,
    pub context_manifest: ContextManifest,
}

#[derive(Debug, Clone)]
pub struct PreparedJob {
    pub correlation_id: String,
    pub dir: PathBuf,
    pub input: JobInput,
    pub prompt_path: PathBuf,
    pub result_path: PathBuf,
    pub log_path: PathBuf,
    pub redact_context: RedactContext,
}

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error("stage {stage} exceeded {max} interrupted attempts")]
    TooManyInterruptions { stage: String, max: usize },
    #[error("invalid job state in {path}")]
    InvalidState { path: String },
    #[error(transparent)]
    Factory(#[from] FactoryError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub fn correlation_id(run_id: &str, stage: &str, attempt: u32) -> String {
    format!("{run_id}.{stage}.{attempt}")
}

pub fn jobs_dir(store: &FactoryStore, run_id: &str) -> PathBuf {
    store.root.join("runs").join(run_id).join("jobs")
}

pub fn job_dir(store: &FactoryStore, run_id: &str, stage: &str, attempt: u32) -> PathBuf {
    jobs_dir(store, run_id).join(correlation_id(run_id, stage, attempt))
}

pub fn read_job_state(dir: &Path) -> Result<JobState, JobError> {
    let state_path = dir.join("state");
    reject_if_symlink(&state_path)?;
    let raw = fs::read_to_string(&state_path)?;
    JobState::parse(&raw).ok_or_else(|| JobError::InvalidState {
        path: state_path.display().to_string(),
    })
}

pub fn set_job_state(dir: &Path, state: JobState) -> Result<(), JobError> {
    let state_path = dir.join("state");
    write_atomic(&state_path, format!("{}\n", state.as_str()).as_bytes())?;
    set_private_file_mode(&state_path)?;
    Ok(())
}

pub fn prepare_job(
    store: &FactoryStore,
    spec: &JobSpec,
    context: &AssembledContext,
) -> Result<PreparedJob, JobError> {
    if !is_valid_run_id(&spec.run) {
        return Err(FactoryError::InvalidInput(format!("invalid run id: {}", spec.run)).into());
    }
    store.ensure()?;
    let corr_id = correlation_id(&spec.run, &spec.stage, spec.attempt);
    let dir = job_dir(store, &spec.run, &spec.stage, spec.attempt);
    create_private_dir(&dir)?;

    let input_path = dir.join("input.json");
    let prompt_path = dir.join("prompt.md");
    let result_path = dir.join("result.json");
    let log_path = dir.join("log.txt");

    let input = JobInput {
        correlation_id: corr_id.clone(),
        run: spec.run.clone(),
        stage: spec.stage.clone(),
        attempt: spec.attempt,
        agent: spec.agent.clone(),
        provider: spec.provider.clone(),
        model: spec.model.clone(),
        tool_grants: spec.tool_grants.clone(),
        cwd: spec.cwd.clone(),
        time_limit_secs: spec.time_limit_secs,
        context_manifest: context.manifest.clone(),
    };

    if !input_path.exists() {
        let bytes = serde_json::to_vec_pretty(&input)?;
        write_atomic(&input_path, &bytes)?;
        set_private_file_mode(&input_path)?;
        write_atomic(&prompt_path, context.prompt.as_bytes())?;
        set_private_file_mode(&prompt_path)?;
        set_job_state(&dir, JobState::Prepared)?;
    }

    Ok(PreparedJob {
        correlation_id: corr_id,
        dir,
        input,
        prompt_path,
        result_path,
        log_path,
        redact_context: RedactContext::from_env(),
    })
}

pub fn count_interrupted_attempts(
    store: &FactoryStore,
    run_id: &str,
    stage: &str,
) -> Result<usize, JobError> {
    let base = jobs_dir(store, run_id);
    if !base.exists() {
        return Ok(0);
    }
    reject_if_symlink(&base)?;
    let prefix = format!("{run_id}.{stage}.");
    let mut count = 0usize;
    for entry in fs::read_dir(&base)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if name_str.starts_with(&prefix)
            && entry.path().is_dir()
            && matches!(read_job_state(&entry.path()), Ok(JobState::Interrupted))
        {
            count += 1;
        }
    }
    Ok(count)
}

pub fn recover_interrupted_job(
    store: &FactoryStore,
    run_id: &str,
    stage: &str,
    attempt: u32,
) -> Result<(), JobError> {
    let dir = job_dir(store, run_id, stage, attempt);
    if read_job_state(&dir)? == JobState::Running {
        let existing = count_interrupted_attempts(store, run_id, stage)?;
        if existing >= MAX_INTERRUPTIONS_PER_STAGE {
            return Err(JobError::TooManyInterruptions {
                stage: stage.to_string(),
                max: MAX_INTERRUPTIONS_PER_STAGE,
            });
        }
        set_job_state(&dir, JobState::Interrupted)?;
    }
    Ok(())
}

pub fn run_job_idempotent<F>(
    store: &FactoryStore,
    spec: &JobSpec,
    context: &AssembledContext,
    runner: F,
) -> Result<JobResult, JobError>
where
    F: FnOnce(&PreparedJob) -> Result<JobResult, JobError>,
{
    let prepared = prepare_job(store, spec, context)?;
    let is_retry = spec.attempt > 1;
    if prepared.result_path.exists()
        && let Ok(existing) = parse_and_validate_result_strict(&prepared.result_path, is_retry)
    {
        return Ok(existing);
    }

    set_job_state(&prepared.dir, JobState::Running)?;
    let result = runner(&prepared)?;
    let bytes = serde_json::to_vec_pretty(&result)?;
    write_atomic(&prepared.result_path, &bytes)?;
    set_private_file_mode(&prepared.result_path)?;
    set_job_state(&prepared.dir, JobState::Finished)?;
    Ok(result)
}
