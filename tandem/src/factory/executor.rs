use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::auth_risk::{
    AuthRisk, acknowledge_command, assess, method_from_login, method_name, read_acks,
};
use super::broker::context::{
    ContextError, ContextInputs, ReviewContextExtras, assemble_context_with_policy,
    assemble_review_context_with_policy,
};
use super::broker::git::{AllowedGitSubcommand, run_git};
use super::broker::job::{
    JobError, JobInput, JobSpec, JobState, correlation_id, job_dir, run_job_idempotent,
    set_job_state,
};
use super::broker::result::{JobResultStatus, parse_and_validate_result};
use super::broker::terms::{AuthType, PROVIDER_TERMS, ProviderTermsRow};
use super::data_policy::{format_readable_scope, load_exclusions};
use super::dates::today_iso_date;
use super::login::interactive_ready;
use super::login::{HostLoginProbe, LOGIN_SPECS, LoginProbe};
use super::model::{Actor, ActorKind};
use super::routing::RoutingContext;
use super::runner::{has_ui_slices, is_stop_requested};
use super::store::{FactoryStore, create_private_dir, write_atomic};
use super::team::{
    adapter_model_runner, discover_catalogs, record_default_selection,
    record_job_started_with_selection, resolve_stage_assignment, slice_hints,
};
use super::worker::{
    WorkerAssignment, WorkerLaunchSpec, WorkerWatchOutcome, finish, launch, watch,
};
use super::workflow::parse::load_workflow_file;
use super::workflow::validate::validate;
use crate::tmux::client::TmuxClient;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageJob {
    pub run: String,
    pub stage: String,
    pub attempt: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum StageOutcome {
    Succeeded,
    Failed {
        reason: String,
    },
    Interrupted,
    Blocked {
        reason: String,
        action_commands: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadyCli {
    pub cli: &'static str,
    pub method: Option<AuthType>,
}

impl ReadyCli {
    pub fn new(cli: &'static str, method: Option<AuthType>) -> Self {
        Self { cli, method }
    }
}

pub trait Executor {
    fn run(&mut self, job: &StageJob) -> StageOutcome;

    fn ready_clis(&self) -> Vec<ReadyCli>;
}

pub type SharedModelRunner = Arc<
    dyn Fn(&str) -> Result<Vec<crate::adapters::base::Model>, crate::adapters::base::AdapterError>
        + Send
        + Sync,
>;

pub struct BrokerExecutor {
    pub workspace: PathBuf,
    pub tmux: TmuxClient,
    pub login_probe: Arc<dyn LoginProbe + Send + Sync>,
    model_runner: SharedModelRunner,
    readiness_cache: Mutex<Option<(Instant, Vec<ReadyCli>)>>,
}

impl BrokerExecutor {
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        let workspace = workspace.into();
        let socket = std::env::var("TANDEM_TMUX_SOCKET")
            .ok()
            .filter(|s| {
                !s.is_empty()
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            })
            .unwrap_or_else(|| crate::tmux::socket_for(&workspace));
        Self {
            workspace,
            tmux: TmuxClient::new(socket, None),
            login_probe: Arc::new(HostLoginProbe),
            model_runner: Arc::new(adapter_model_runner),
            readiness_cache: Mutex::new(None),
        }
    }

    pub fn with_model_runner(mut self, runner: SharedModelRunner) -> Self {
        self.model_runner = runner;
        self
    }

    pub fn with_tmux(mut self, tmux: TmuxClient) -> Self {
        self.tmux = tmux;
        self
    }

    pub fn with_login_probe(mut self, login_probe: Arc<dyn LoginProbe + Send + Sync>) -> Self {
        self.login_probe = login_probe;
        self.readiness_cache = Mutex::new(None);
        self
    }

    fn probe_ready(&self) -> Vec<ReadyCli> {
        const TTL: Duration = Duration::from_secs(10);
        if let Ok(guard) = self.readiness_cache.lock()
            && let Some((ts, ref cached)) = *guard
            && ts.elapsed() < TTL
        {
            return cached.clone();
        }
        let ready: Vec<ReadyCli> = LOGIN_SPECS
            .iter()
            .filter_map(|spec| {
                let status = interactive_ready(spec.cli, self.login_probe.as_ref());
                status.ready.then(|| {
                    ReadyCli::new(
                        spec.cli,
                        method_from_login(spec.cli, status.method.as_deref()),
                    )
                })
            })
            .collect();
        if let Ok(mut guard) = self.readiness_cache.lock() {
            *guard = Some((Instant::now(), ready.clone()));
        }
        ready
    }
}

fn ensure_context_file(
    preferred: PathBuf,
    fallback_dir: &Path,
    fallback_name: &str,
    default_body: &str,
) -> Result<PathBuf, std::io::Error> {
    if preferred.exists() {
        return Ok(preferred);
    }
    create_private_dir(fallback_dir).map_err(|e| std::io::Error::other(e.to_string()))?;
    let fallback = fallback_dir.join(fallback_name);
    if !fallback.exists() {
        write_atomic(&fallback, default_body.as_bytes())
            .map_err(|e| std::io::Error::other(e.to_string()))?;
    }
    Ok(fallback)
}

pub(crate) fn last_stage_provider_model(
    store: &FactoryStore,
    run: &super::model::Run,
    stage: &str,
) -> (Option<String>, Option<String>) {
    let Some(st) = run.stages.iter().find(|s| s.name == stage) else {
        return (None, None);
    };
    if st.attempts == 0 {
        return (None, None);
    }
    let input_path = job_dir(store, &run.id, stage, st.attempts).join("input.json");
    let Ok(raw) = fs::read_to_string(input_path) else {
        return (None, None);
    };
    let Ok(input) = serde_json::from_str::<JobInput>(&raw) else {
        return (None, None);
    };
    (Some(input.provider), Some(input.model))
}

fn collect_review_extras(
    store: &FactoryStore,
    run: &super::model::Run,
    worktree_path: &Path,
    artifact_dir: &Path,
) -> ReviewContextExtras {
    let mut changed_files = Vec::new();
    if let Some(dev_stage) = run.stages.iter().find(|s| s.name == "develop")
        && dev_stage.attempts > 0
    {
        let dev_result = job_dir(store, &run.id, "develop", dev_stage.attempts).join("result.json");
        if dev_result.exists() {
            let parsed = parse_and_validate_result(&dev_result, dev_stage.attempts > 1);
            changed_files = parsed.files_changed;
        }
    }

    let range = run
        .base_commit
        .as_deref()
        .filter(|b| !b.trim().is_empty())
        .map(|b| format!("{b}..HEAD"));
    if changed_files.is_empty() {
        let args: Vec<&str> = match range.as_deref() {
            Some(r) => vec!["--name-only", r],
            None => vec!["--name-only", "HEAD"],
        };
        if let Ok(out) = run_git(
            AllowedGitSubcommand::Diff,
            &args,
            worktree_path,
            Duration::from_secs(15),
        ) {
            changed_files = out
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(ToString::to_string)
                .collect();
        }
    }

    let diff_args: Vec<&str> = match range.as_deref() {
        Some(r) => vec![r],
        None => vec!["HEAD"],
    };
    let diff = run_git(
        AllowedGitSubcommand::Diff,
        &diff_args,
        worktree_path,
        Duration::from_secs(15),
    )
    .unwrap_or_default();

    let mut evidence_parts = Vec::new();
    let checks_dir = artifact_dir.join("checks");
    if checks_dir.is_dir()
        && let Ok(entries) = fs::read_dir(&checks_dir)
    {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            evidence_parts.push(format!("check artifact: {name}"));
        }
    }
    if let Some(test_stage) = run.stages.iter().find(|s| s.name == "test")
        && test_stage.attempts > 0
    {
        evidence_parts.push(format!(
            "test stage state: {:?} (attempts: {})",
            test_stage.state, test_stage.attempts
        ));
    }

    ReviewContextExtras {
        changed_files,
        diff,
        evidence: evidence_parts.join("\n"),
    }
}

impl Executor for BrokerExecutor {
    fn run(&mut self, job: &StageJob) -> StageOutcome {
        self.run_with_env(job, std::env::vars().collect())
    }

    fn ready_clis(&self) -> Vec<ReadyCli> {
        self.probe_ready()
    }
}

impl BrokerExecutor {
    pub fn run_with_env(
        &self,
        job: &StageJob,
        parent_env: BTreeMap<String, String>,
    ) -> StageOutcome {
        self.run_with_env_and_gate(job, parent_env, PROVIDER_TERMS, &today_iso_date())
    }

    pub fn run_with_env_and_gate(
        &self,
        job: &StageJob,
        parent_env: BTreeMap<String, String>,
        terms: &[ProviderTermsRow],
        today: &str,
    ) -> StageOutcome {
        let store = FactoryStore::new(&self.workspace);
        let run = match super::run::get_run(&store, &job.run) {
            Ok(Some(r)) => r,
            Ok(None) => {
                return StageOutcome::Failed {
                    reason: format!("run {} not found", job.run),
                };
            }
            Err(e) => {
                return StageOutcome::Failed {
                    reason: e.to_string(),
                };
            }
        };
        let settings = match store.settings() {
            Ok(s) => s,
            Err(e) => {
                return StageOutcome::Failed {
                    reason: e.to_string(),
                };
            }
        };
        let idea = store.get_idea(&run.idea).ok().flatten();
        let wf_path = store.run_dir(&job.run).join("workflow.yaml");
        let raw = match load_workflow_file(&wf_path) {
            Ok(r) => r,
            Err(e) => {
                return StageOutcome::Failed {
                    reason: e.to_string(),
                };
            }
        };
        let wf = match validate(&raw, &settings) {
            Ok(w) => w,
            Err(diags) => {
                return StageOutcome::Failed {
                    reason: diags
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; "),
                };
            }
        };
        let Some(stage_def) = wf.stages.iter().find(|s| s.name == job.stage) else {
            return StageOutcome::Failed {
                reason: format!("stage {} not found in workflow", job.stage),
            };
        };

        let worktree_path = run
            .worktree_path
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.workspace.clone());
        let artifact_dir = store.run_dir(&job.run).join("artifacts");
        if !artifact_dir.exists() {
            let _ = fs::create_dir_all(&artifact_dir);
        }

        let mut confirmed_for_stage: Option<super::checks::ConfirmedChecks> = None;
        if job.stage == "create-tests" {
            let target_name = run.target.as_deref().unwrap_or("workspace");
            match super::checks::require_confirmed_checks(&settings, target_name) {
                Ok(c) => confirmed_for_stage = Some(c),
                Err(e) => {
                    return StageOutcome::Blocked {
                        reason: e.to_string(),
                        action_commands: vec![format!(
                            "tandem factory checks discover --target {target_name}"
                        )],
                    };
                }
            }
        }

        let mut check_failures_summary: Option<String> = None;
        if job.stage == "test" {
            let target_name = run.target.as_deref().unwrap_or("workspace");
            let confirmed = match super::checks::require_confirmed_checks(&settings, target_name) {
                Ok(c) => c,
                Err(e) => {
                    return StageOutcome::Blocked {
                        reason: e.to_string(),
                        action_commands: vec![format!(
                            "tandem factory checks discover --target {target_name}"
                        )],
                    };
                }
            };
            let report = match super::stages::test::run_test_stage_checks(
                &confirmed,
                &worktree_path,
                &artifact_dir,
                job.attempt,
            ) {
                Ok(r) => r,
                Err(e) => {
                    return StageOutcome::Failed {
                        reason: e.to_string(),
                    };
                }
            };
            let sys_actor = super::model::Actor {
                kind: super::model::ActorKind::System,
                id: "executor".to_string(),
            };
            let _ = super::evidence::record_test_stage_evidence(
                &store,
                &run.idea,
                &job.run,
                job.attempt,
                &report,
                &sys_actor,
            );
            if report.all_passed {
                return StageOutcome::Succeeded;
            }
            let summary_path = artifact_dir
                .join("checks")
                .join(format!("failures-attempt{}.md", job.attempt));
            let _ = write_atomic(&summary_path, report.failures_summary.as_bytes());
            check_failures_summary = Some(report.failures_summary);
        }

        if job.stage == "deploy" {
            if !run.deploy_requested {
                return StageOutcome::Blocked {
                    reason: "deploy was not requested at mark-ready".to_string(),
                    action_commands: vec![],
                };
            }
            if run.prs.is_empty() {
                return StageOutcome::Blocked {
                    reason: "a pull request is required before deploy".to_string(),
                    action_commands: vec![format!("tandem factory run prs {} --refresh", job.run)],
                };
            }
            let target_name = run.target.as_deref().unwrap_or("workspace");
            let confirmed = match super::checks::require_confirmed_deploy(&settings, target_name) {
                Ok(c) => c,
                Err(e) => {
                    return StageOutcome::Blocked {
                        reason: e.to_string(),
                        action_commands: vec![format!(
                            "tandem factory checks set --target {target_name} --deploy '[\"<cmd>\"]' --confirm {target_name}"
                        )],
                    };
                }
            };
            let report = match super::stages::deploy::run_deploy_stage(
                &confirmed,
                &worktree_path,
                &artifact_dir,
                job.attempt,
            ) {
                Ok(r) => r,
                Err(e) => {
                    return StageOutcome::Failed {
                        reason: e.to_string(),
                    };
                }
            };
            let sys_actor = super::model::Actor {
                kind: super::model::ActorKind::System,
                id: "executor".to_string(),
            };
            let _ = super::evidence::record_deploy_stage_evidence(
                &store,
                &run.idea,
                &job.run,
                job.attempt,
                &report,
                &sys_actor,
            );
            if report.passed {
                return StageOutcome::Succeeded;
            }
            return StageOutcome::Failed {
                reason: format!("deploy command failed: {}", report.summary),
            };
        }

        let ready = self.probe_ready();
        let ready_clis: Vec<&'static str> = ready.iter().map(|r| r.cli).collect();
        if ready_clis.is_empty() {
            return StageOutcome::Blocked {
                reason: "no CLI is installed and signed in".to_string(),
                action_commands: vec!["tandem factory login open claude".to_string()],
            };
        }

        let (avoid_provider, avoid_model) = if job.stage == "review" {
            last_stage_provider_model(&store, &run, "develop")
        } else {
            (None, None)
        };
        let preferred_provider =
            (settings.default_worker_cli != "auto").then(|| settings.default_worker_cli.clone());
        let (complexity, required_tier) = slice_hints(&store, &job.run);
        let r_ctx = RoutingContext {
            preferred_provider,
            is_ui_work: has_ui_slices(&store, &job.run),
            avoid_provider,
            avoid_model,
            complexity,
            required_tier,
            catalog: discover_catalogs(&store, &settings, &ready_clis, &*self.model_runner),
        };
        let target_name = run.target.as_deref().unwrap_or("workspace");
        let tool_grant = match super::permissions::grant(
            stage_def.agent,
            stage_def,
            target_name,
            &wf,
            &settings,
            false,
        ) {
            Ok(g) => g,
            Err(denied) => {
                return StageOutcome::Blocked {
                    reason: denied.to_string(),
                    action_commands: denied.action_commands(&run.id),
                };
            }
        };

        let resolved = match resolve_stage_assignment(
            &store,
            &run,
            stage_def,
            &tool_grant,
            &wf,
            &settings,
            &ready_clis,
            job.attempt,
            &r_ctx,
        ) {
            Ok(r) => r,
            Err(e) => {
                let mut action_commands = Vec::new();
                if !settings.orchestrator_may_choose
                    && settings.default_worker_cli != "auto"
                    && e.to_string().contains("configured default_worker_cli")
                {
                    action_commands.push(format!(
                        "tandem factory login open {}",
                        settings.default_worker_cli
                    ));
                }
                return StageOutcome::Blocked {
                    reason: e.to_string(),
                    action_commands,
                };
            }
        };
        let routed = resolved.route;
        let assigned_role = resolved.role;
        let assignment_source = resolved.source;
        let selection = resolved.selection;
        if assignment_source == super::team::AssignmentSource::Default
            && let Some(sel) = &selection
        {
            let _ = record_default_selection(
                &store,
                &job.run,
                &job.stage,
                &routed.provider,
                &routed.model,
                &assigned_role,
                sel,
            );
        }
        let detected = ready
            .iter()
            .find(|r| r.cli == routed.provider)
            .and_then(|r| r.method);
        if let AuthRisk::Risky { reason, .. } =
            assess(&routed.provider, detected, today, terms, &read_acks(&store))
        {
            let method = method_name(detected);
            return StageOutcome::Blocked {
                reason: format!(
                    "auth risk not acknowledged for {} ({method}): {reason}",
                    routed.provider
                ),
                action_commands: vec![acknowledge_command(&routed.provider, method)],
            };
        }

        let plan_snapshot_path = store
            .root
            .join("ideas")
            .join(&run.idea)
            .join("artifacts")
            .join("plan-snapshot.md");

        let context_cache_dir = store.run_dir(&job.run).join("context");
        let playbook_path = match ensure_context_file(
            self.workspace
                .join(".agent-workspace/factory/playbooks")
                .join(format!("{}.md", job.stage)),
            &context_cache_dir,
            &format!("playbook-{}.md", job.stage),
            &format!(
                "# Stage: {}\nExecute stage {} cleanly.\n",
                job.stage, job.stage
            ),
        ) {
            Ok(p) => p,
            Err(e) => {
                return StageOutcome::Failed {
                    reason: e.to_string(),
                };
            }
        };
        let agent_def_path = match ensure_context_file(
            self.workspace
                .join(".agents/agents")
                .join(format!("{}.md", routed.agent_definition)),
            &context_cache_dir,
            &format!("agent-{}.md", routed.agent_definition),
            &format!("# Agent: {}\n", routed.agent_definition),
        ) {
            Ok(p) => p,
            Err(e) => {
                return StageOutcome::Failed {
                    reason: e.to_string(),
                };
            }
        };

        let mut dependency_summaries = Vec::new();
        if let Some(ref failures) = check_failures_summary {
            dependency_summaries.push(("checks".to_string(), failures.clone()));
        }
        for dep in &stage_def.depends_on {
            if let Some(dep_stage) = run.stages.iter().find(|s| s.name == *dep)
                && dep_stage.attempts > 0
            {
                let dep_result =
                    job_dir(&store, &job.run, dep, dep_stage.attempts).join("result.json");
                if dep_result.exists() {
                    let parsed = parse_and_validate_result(&dep_result, dep_stage.attempts > 1);
                    if !parsed.summary.is_empty() {
                        dependency_summaries.push((dep.clone(), parsed.summary));
                    }
                }
            }
        }

        let retry_rationale = run
            .stages
            .iter()
            .find(|s| s.name == job.stage)
            .and_then(|s| s.retry_hypothesis.clone().or_else(|| s.rationale.clone()));

        let context_inputs = ContextInputs {
            plan_snapshot_path: plan_snapshot_path.clone(),
            acceptance_criteria: idea
                .as_ref()
                .and_then(|i| i.plan.as_ref())
                .map(|p| p.acceptance_criteria.clone())
                .unwrap_or_default(),
            playbook_path,
            agent_def_path,
            dependency_summaries: if job.stage == "review" {
                Vec::new()
            } else {
                dependency_summaries
            },
            retry_rationale,
            tool_grant: Some(tool_grant.clone()),
        };
        let assembled_res = if job.stage == "review" {
            let extras = collect_review_extras(&store, &run, &worktree_path, &artifact_dir);
            assemble_review_context_with_policy(
                &context_inputs,
                &extras,
                &routed.provider,
                &settings,
            )
        } else {
            assemble_context_with_policy(&context_inputs, &routed.provider, &settings)
        };
        let mut assembled = match assembled_res {
            Ok(c) => c,
            Err(ContextError::PolicyDenied(denial)) => {
                return StageOutcome::Blocked {
                    reason: denial.to_string(),
                    action_commands: vec![format!("tandem factory run resume {}", job.run)],
                };
            }
            Err(e) => {
                return StageOutcome::Failed {
                    reason: e.to_string(),
                };
            }
        };
        assembled.manifest.excluded_paths = load_exclusions(&store, &job.run)
            .into_iter()
            .map(|e| e.path)
            .collect();
        assembled.manifest.readable_scope = Some(format_readable_scope(
            run.worktree_path.as_deref(),
            &worktree_path.to_string_lossy(),
        ));

        let spec = JobSpec {
            run: job.run.clone(),
            stage: job.stage.clone(),
            attempt: job.attempt,
            agent: routed.agent_definition.to_string(),
            provider: routed.provider.clone(),
            model: routed.model.clone(),
            tool_grants: tool_grant
                .tools
                .iter()
                .map(|t| t.as_str().to_string())
                .collect(),
            cwd: worktree_path.to_string_lossy().to_string(),
            time_limit_secs: stage_def.budget.max_duration_secs.unwrap_or(1800),
        };

        let mut was_interrupted = false;

        let job_outcome = run_job_idempotent(&store, &spec, &assembled, |prepared| {
            let worker_spec = WorkerLaunchSpec {
                run_id: job.run.clone(),
                stage: job.stage.clone(),
                attempt: job.attempt,
                job_dir: prepared.dir.clone(),
                prompt_file: prepared.prompt_path.clone(),
                result_path: prepared.result_path.clone(),
                worktree_path: worktree_path.clone(),
                assignment: WorkerAssignment {
                    cli: routed.provider.clone(),
                    model: routed.model.clone(),
                    read_only: !tool_grant.has_repo_write,
                    tools: routed.tool_flags.clone(),
                    agent: routed.agent_definition.to_string(),
                    role: assigned_role.clone(),
                },
                env: parent_env.clone(),
                time_limit_secs: spec.time_limit_secs,
            };
            let launched = launch(&self.tmux, &worker_spec)
                .map_err(|e| JobError::Io(std::io::Error::other(e.to_string())))?;
            let _ = record_job_started_with_selection(
                &store,
                &run.idea,
                &job.run,
                &correlation_id(&job.run, &job.stage, job.attempt),
                &job.stage,
                job.attempt,
                &routed.provider,
                &routed.model,
                &assigned_role,
                assignment_source,
                &launched.pane_id,
                &Actor {
                    kind: ActorKind::System,
                    id: "executor".to_string(),
                },
                selection.clone(),
            );
            let watch_outcome = watch(&self.tmux, &launched, &|| {
                is_stop_requested(&store, &job.run)
            })
            .map_err(|e| JobError::Io(std::io::Error::other(e.to_string())))?;
            if matches!(watch_outcome, WorkerWatchOutcome::Interrupted) {
                was_interrupted = true;
                let _ = finish(&self.tmux, &launched, watch_outcome);
                return Err(JobError::Io(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "interrupted",
                )));
            }
            finish(&self.tmux, &launched, watch_outcome)
                .map_err(|e| JobError::Io(std::io::Error::other(e.to_string())))
        });

        if was_interrupted {
            let dir = job_dir(&store, &job.run, &job.stage, job.attempt);
            let _ = set_job_state(&dir, JobState::Interrupted);
            return StageOutcome::Interrupted;
        }

        let parsed = match job_outcome {
            Ok(r) => r,
            Err(e) => {
                return StageOutcome::Failed {
                    reason: check_failures_summary.unwrap_or_else(|| e.to_string()),
                };
            }
        };

        if let Some(failures) = check_failures_summary {
            let diagnosis = parsed
                .rationale
                .filter(|s| !s.trim().is_empty())
                .or_else(|| (!parsed.summary.trim().is_empty()).then_some(parsed.summary))
                .unwrap_or(failures);
            return StageOutcome::Failed { reason: diagnosis };
        }

        let has_repo_write = tool_grant.has_repo_write;

        match parsed.status {
            JobResultStatus::Succeeded => {
                if let Some(contract) = super::stages::contract_for_stage(&job.stage) {
                    let ctx = super::stages::StageContext {
                        run_id: &job.run,
                        idea_id: &run.idea,
                        stage: &job.stage,
                        attempt: job.attempt,
                        has_repo_write,
                        artifact_dir: &artifact_dir,
                        target_repo: &worktree_path,
                        worktree_path: &worktree_path,
                        plan_snapshot_path: &plan_snapshot_path,
                        confirmed_checks: confirmed_for_stage.as_ref(),
                        result: &parsed,
                    };
                    if let Err(failure) = contract.verify(&ctx) {
                        return StageOutcome::Failed {
                            reason: failure.to_string(),
                        };
                    }
                    let sys_actor = super::model::Actor {
                        kind: super::model::ActorKind::System,
                        id: "executor".to_string(),
                    };
                    if job.stage == "create-tests" && parsed.no_behaviour_change {
                        let _ = super::evidence::record_create_tests_exception(
                            &store,
                            &run.idea,
                            &job.run,
                            job.attempt,
                            parsed.rationale.as_deref().unwrap_or("no_behaviour_change"),
                            &sys_actor,
                        );
                    }
                    if job.stage == "demo"
                        && let Err(e) = super::evidence::record_demo_screenshots(
                            &store,
                            &run.idea,
                            &job.run,
                            job.attempt,
                            &artifact_dir,
                            &sys_actor,
                        )
                    {
                        return StageOutcome::Failed {
                            reason: e.to_string(),
                        };
                    }
                    if matches!(job.stage.as_str(), "create-tests" | "develop") {
                        match super::stages::scope::list_changed_files(&worktree_path).and_then(
                            |files| {
                                let committed = !files.is_empty();
                                super::stages::commit_stage_changes(
                                    &worktree_path,
                                    &run.idea,
                                    &job.stage,
                                    &files,
                                )?;
                                Ok(committed)
                            },
                        ) {
                            Ok(committed) => {
                                if committed {
                                    let _ = super::evidence::record_committed_diff_stat(
                                        &store,
                                        &run.idea,
                                        &job.run,
                                        &job.stage,
                                        job.attempt,
                                        &worktree_path,
                                        &sys_actor,
                                    );
                                }
                            }
                            Err(failure) => {
                                return StageOutcome::Failed {
                                    reason: failure.to_string(),
                                };
                            }
                        }
                    }
                } else if !has_repo_write
                    && let Err(failure) = super::stages::scope::verify_scope_with_grant(
                        &worktree_path,
                        &artifact_dir,
                        &tool_grant,
                    )
                {
                    return StageOutcome::Failed {
                        reason: failure.to_string(),
                    };
                }
                StageOutcome::Succeeded
            }
            JobResultStatus::Failed | JobResultStatus::NeedsInput => StageOutcome::Failed {
                reason: parsed
                    .rationale
                    .unwrap_or_else(|| "no structured result".to_string()),
            },
        }
    }
}
