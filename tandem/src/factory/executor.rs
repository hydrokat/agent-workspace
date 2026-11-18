use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::broker::context::{
    ContextInputs, ReviewContextExtras, assemble_context, assemble_review_context,
};
use super::broker::gate::{check, today_iso_date};
use super::broker::git::{AllowedGitSubcommand, run_git};
use super::broker::job::{
    JobError, JobInput, JobSpec, JobState, job_dir, run_job_idempotent, set_job_state,
};
use super::broker::result::{JobResultStatus, parse_and_validate_result};
use super::broker::spawn::{DEFAULT_STOP_GRACE, ProviderCommand, WaitOutcome, spawn_provider};
use super::model::KNOWN_PROVIDERS;
use super::routing::{RoutingContext, TemperatureSupport, route};
use super::runner::{has_ui_slices, is_stop_requested};
use super::store::{FactoryStore, create_private_dir, write_atomic};
use super::workflow::parse::load_workflow_file;
use super::workflow::validate::validate;
use crate::adapters::agy::AgyAdapter;
use crate::adapters::base::{HeadlessAdapter, HeadlessRequest, OutputFormat};
use crate::adapters::claude::ClaudeAdapter;

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

pub trait Executor {
    fn run(&mut self, job: &StageJob) -> StageOutcome;
}

pub struct BrokerExecutor {
    pub workspace: PathBuf,
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
}

impl BrokerExecutor {
    pub fn run_with_env(
        &self,
        job: &StageJob,
        parent_env: BTreeMap<String, String>,
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

        let today = today_iso_date();
        let open_providers: Vec<&str> = KNOWN_PROVIDERS
            .iter()
            .copied()
            .filter(|p| check(p, &settings, &today).is_ok())
            .collect();
        if open_providers.is_empty() {
            return StageOutcome::Blocked {
                reason: "gate closed: no provider consented and allowed".to_string(),
                action_commands: vec![
                    "tandem factory provider consent claude --auth api-key --confirm claude"
                        .to_string(),
                ],
            };
        }

        let (avoid_provider, avoid_model) = if job.stage == "review" {
            last_stage_provider_model(&store, &run, "develop")
        } else {
            (None, None)
        };
        let r_ctx = RoutingContext {
            preferred_provider: idea
                .as_ref()
                .and_then(|i| i.orchestrator.as_ref())
                .map(|o| o.cli.clone()),
            is_ui_work: has_ui_slices(&store, &job.run),
            avoid_provider,
            avoid_model,
        };
        let routed = match route(
            stage_def,
            &wf,
            &settings,
            &open_providers,
            job.attempt,
            &r_ctx,
        ) {
            Ok(r) => r,
            Err(e) => {
                return StageOutcome::Blocked {
                    reason: e.to_string(),
                    action_commands: vec![],
                };
            }
        };

        let pass = match check(&routed.provider, &settings, &today) {
            Ok(p) => p,
            Err(e) => {
                return StageOutcome::Blocked {
                    reason: e.to_string(),
                    action_commands: vec![format!(
                        "tandem factory provider consent {} --auth api-key --confirm {}",
                        routed.provider, routed.provider
                    )],
                };
            }
        };

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
        };
        let assembled_res = if job.stage == "review" {
            let extras = collect_review_extras(&store, &run, &worktree_path, &artifact_dir);
            assemble_review_context(&context_inputs, &extras)
        } else {
            assemble_context(&context_inputs)
        };
        let assembled = match assembled_res {
            Ok(c) => c,
            Err(e) => {
                return StageOutcome::Failed {
                    reason: e.to_string(),
                };
            }
        };

        let headless_req = HeadlessRequest {
            model: routed.model.clone(),
            tools: routed.tool_flags.clone(),
            temperature: match routed.temperature {
                TemperatureSupport::Supported(t) => Some(t),
                TemperatureSupport::Unsupported => None,
            },
            output: OutputFormat::Json,
        };
        let argv = match routed.provider.as_str() {
            "claude" => ClaudeAdapter::default().build_headless_argv(&headless_req),
            "agy" => AgyAdapter::default().build_headless_argv(&headless_req),
            other => {
                return StageOutcome::Failed {
                    reason: format!("unsupported provider: {other}"),
                };
            }
        };
        let argv = match argv {
            Ok(a) => a,
            Err(e) => {
                return StageOutcome::Failed {
                    reason: e.to_string(),
                };
            }
        };
        let cli_args: Vec<String> = argv.into_iter().skip(1).collect();

        let spec = JobSpec {
            run: job.run.clone(),
            stage: job.stage.clone(),
            attempt: job.attempt,
            agent: routed.agent_definition.to_string(),
            provider: routed.provider.clone(),
            model: routed.model.clone(),
            tool_grants: stage_def.tools.iter().map(|t| format!("{t:?}")).collect(),
            cwd: worktree_path.to_string_lossy().to_string(),
            time_limit_secs: stage_def.budget.max_duration_secs.unwrap_or(1800),
        };

        let mut was_interrupted = false;
        let mut pass_opt = Some(pass);

        let job_outcome = run_job_idempotent(&store, &spec, &assembled, |prepared| {
            let pass = pass_opt
                .take()
                .ok_or_else(|| JobError::Io(std::io::Error::other("missing gate pass")))?;
            let mut factory_env = BTreeMap::new();
            factory_env.insert("TANDEM_RUN_ID".to_string(), job.run.clone());
            factory_env.insert("TANDEM_STAGE".to_string(), job.stage.clone());
            factory_env.insert("TANDEM_ATTEMPT".to_string(), job.attempt.to_string());
            factory_env.insert(
                "TANDEM_JOB_DIR".to_string(),
                prepared.dir.to_string_lossy().to_string(),
            );
            let cmd = ProviderCommand {
                program: routed.provider.clone(),
                args: cli_args.clone(),
                workspace_root: self.workspace.clone(),
                cwd: worktree_path.clone(),
                log_path: prepared.log_path.clone(),
                stdin_prompt: Some(assembled.prompt.clone()),
                factory_env,
                parent_env: parent_env.clone(),
            };
            let mut running = spawn_provider(pass, cmd)
                .map_err(|e| JobError::Io(std::io::Error::other(e.to_string())))?;
            let timeout = Duration::from_secs(spec.time_limit_secs);
            match running.wait_with_stop(
                timeout,
                &|| is_stop_requested(&store, &job.run),
                DEFAULT_STOP_GRACE,
            ) {
                Ok(WaitOutcome::Stopped(_)) => {
                    was_interrupted = true;
                    Err(JobError::Io(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "interrupted",
                    )))
                }
                Ok(WaitOutcome::Exited(_)) => Ok(parse_and_validate_result(
                    &prepared.result_path,
                    job.attempt > 1,
                )),
                Err(e) => Err(JobError::Io(std::io::Error::other(e.to_string()))),
            }
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

        let has_repo_write = stage_def
            .tools
            .iter()
            .any(|t| matches!(t, super::workflow::schema::WorkflowTool::RepoWrite));

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
                    && let Err(failure) =
                        super::stages::scope::verify_scope(&super::stages::scope::ScopeCheckInput {
                            worktree_path: &worktree_path,
                            artifact_dir: &artifact_dir,
                            has_repo_write: false,
                        })
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
