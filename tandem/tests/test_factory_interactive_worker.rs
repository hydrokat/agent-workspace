use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tandem::adapters::agy::AgyAdapter;
use tandem::adapters::base::{InteractiveWorkerAdapter, ToolFlag, WorkerLaunchRequest};
use tandem::adapters::claude::ClaudeAdapter;
use tandem::adapters::opencode::OpencodeAdapter;
use tandem::factory::approval::{Caller, MarkReadyOptions, mark_ready_with_options};
use tandem::factory::artifacts;
use tandem::factory::broker::result::{JobResultStatus, JobUsage};
use tandem::factory::budget::{BudgetCapKind, BudgetCheck, RunBudget, check, spent};
use tandem::factory::events::{append, read_all, redact_event};
use tandem::factory::machine::{Command, EventData};
use tandem::factory::model::{Actor, ActorKind, FeasibilityVerdict, ModelPrice, Risk};
use tandem::factory::recovery::{
    DoctorFinding, ProblemKind, diagnose_with_tmux, doctor_fix_with_tmux,
};
use tandem::factory::redact::RedactContext;
use tandem::factory::routing::ModelTier;
use tandem::factory::run::{CreateRunOptions, create_run};
use tandem::factory::store::FactoryStore;
use tandem::factory::telemetry::{RecordJobUsageInput, record_interactive_job_usage};
use tandem::factory::worker::{
    WorkerAssignment, WorkerLaunchSpec, WorkerWatchOutcome, finish, launch, watch_with_interval,
};
use tandem::tmux::client::{Result as TmuxCmdResult, TmuxClient, TmuxRunner};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("tandem-iw-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Default)]
struct RecordingTmuxState {
    calls: Vec<Vec<String>>,
    panes_responses: VecDeque<String>,
    default_panes: String,
    next_pane_id: String,
}

#[derive(Clone)]
struct RecordingTmuxRunner {
    state: Arc<Mutex<RecordingTmuxState>>,
}

impl RecordingTmuxRunner {
    fn with_pane(pane_id: &str) -> Self {
        Self {
            state: Arc::new(Mutex::new(RecordingTmuxState {
                next_pane_id: pane_id.to_string(),
                ..Default::default()
            })),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.state.lock().unwrap().calls.clone()
    }

    fn set_default_panes(&self, raw: String) {
        self.state.lock().unwrap().default_panes = raw;
    }

    fn push_panes_response(&self, raw: String) {
        self.state.lock().unwrap().panes_responses.push_back(raw);
    }
}

impl TmuxRunner for RecordingTmuxRunner {
    fn run(&self, args: &[String]) -> TmuxCmdResult {
        self.state.lock().unwrap().calls.push(args.to_vec());
        // argv layout from TmuxClient: ["tmux", "-L", <socket>, ...args]
        let rest = if args.len() >= 3 && args[0] == "tmux" && args[1] == "-L" {
            if args.get(3).map(String::as_str) == Some("-f") {
                &args[5..]
            } else {
                &args[3..]
            }
        } else {
            args
        };
        let mut st = self.state.lock().unwrap();
        match rest.first().map(String::as_str) {
            Some("new-window") => TmuxCmdResult {
                code: 0,
                out: st.next_pane_id.clone(),
                err: String::new(),
            },
            Some("list-panes") => {
                let out = if let Some(next) = st.panes_responses.pop_front() {
                    next
                } else {
                    st.default_panes.clone()
                };
                TmuxCmdResult {
                    code: 0,
                    out,
                    err: String::new(),
                }
            }
            Some("set-option" | "kill-pane") => TmuxCmdResult {
                code: 0,
                out: String::new(),
                err: String::new(),
            },
            Some(other) => TmuxCmdResult {
                code: 1,
                out: String::new(),
                err: format!("unexpected tmux cmd: {other}"),
            },
            None => TmuxCmdResult {
                code: 0,
                out: String::new(),
                err: String::new(),
            },
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn pane_line(
    pane_id: &str,
    window: &str,
    kind: &str,
    dead: bool,
    run: &str,
    job: &str,
    stage: &str,
    cli: &str,
    model: &str,
) -> String {
    let dead_flag = if dead { "1" } else { "0" };
    [
        pane_id,
        window,
        kind,
        "factory-worker",
        "1",
        stage,
        dead_flag,
        "0",
        "",
        "",
        "",
        "",
        "",
        run,
        job,
        stage,
        cli,
        model,
    ]
    .join("\t")
}

#[test]
fn claude_worker_argv_sets_permission_mode_model_tools_and_prompt_file() {
    let adapter = ClaudeAdapter::new();
    let req = WorkerLaunchRequest {
        model: "claude-sonnet-4-6".to_string(),
        tools: vec![
            ToolFlag::Allow("Read".to_string()),
            ToolFlag::Allow("Edit".to_string()),
            ToolFlag::Disallow("WebFetch".to_string()),
        ],
        read_only: false,
        prompt_file: PathBuf::from("/tmp/run/jobs/j1/prompt.md"),
    };
    let spec = adapter.build_worker_argv(&req).unwrap();
    assert_eq!(spec.argv[0], "claude");
    assert!(
        spec.argv
            .windows(2)
            .any(|w| w == ["--permission-mode", "acceptEdits"])
    );
    assert!(
        spec.argv
            .windows(2)
            .any(|w| w == ["--model", "claude-sonnet-4-6"])
    );
    assert!(
        spec.argv
            .windows(2)
            .any(|w| w == ["--allowedTools", "Read,Edit"])
    );
    assert!(
        spec.argv
            .windows(2)
            .any(|w| w == ["--disallowedTools", "WebFetch"])
    );
    assert!(
        spec.argv
            .last()
            .unwrap()
            .contains("/tmp/run/jobs/j1/prompt.md")
    );
    assert!(spec.env.is_empty());

    let ro_req = WorkerLaunchRequest {
        read_only: true,
        ..req
    };
    let ro_spec = adapter.build_worker_argv(&ro_req).unwrap();
    assert!(
        ro_spec
            .argv
            .windows(2)
            .any(|w| w == ["--permission-mode", "plan"])
    );
}

#[test]
fn agy_worker_argv_uses_verified_flags_and_refuses_unsupported_grants() {
    let adapter = AgyAdapter::new();
    let req = WorkerLaunchRequest {
        model: "gemini-2.5-pro".to_string(),
        tools: vec![
            ToolFlag::Allow("Read".to_string()),
            ToolFlag::Allow("Edit".to_string()),
        ],
        read_only: false,
        prompt_file: PathBuf::from("/tmp/job/prompt.md"),
    };
    let spec = adapter.build_worker_argv(&req).unwrap();
    assert_eq!(spec.argv[0], "agy");
    assert!(
        spec.argv
            .windows(2)
            .any(|w| w == ["--model", "gemini-2.5-pro"])
    );
    assert!(
        spec.argv
            .windows(2)
            .any(|w| w == ["--mode", "accept-edits"])
    );
    assert!(
        spec.argv
            .windows(2)
            .any(|w| w[0] == "--prompt-interactive" && w[1].contains("/tmp/job/prompt.md"))
    );

    let ro = adapter
        .build_worker_argv(&WorkerLaunchRequest {
            model: "gemini-2.5-pro".to_string(),
            tools: vec![ToolFlag::Allow("Read".to_string())],
            read_only: true,
            prompt_file: PathBuf::from("/tmp/job/prompt.md"),
        })
        .unwrap();
    assert!(ro.argv.windows(2).any(|w| w == ["--mode", "plan"]));

    let disallowed = adapter.build_worker_argv(&WorkerLaunchRequest {
        model: "gemini-2.5-pro".to_string(),
        tools: vec![ToolFlag::Disallow("Bash".to_string())],
        read_only: false,
        prompt_file: PathBuf::from("/tmp/job/prompt.md"),
    });
    assert!(disallowed.is_err());

    let scoped_bash = adapter.build_worker_argv(&WorkerLaunchRequest {
        model: "gemini-2.5-pro".to_string(),
        tools: vec![ToolFlag::Allow("Bash(gh:*)".to_string())],
        read_only: false,
        prompt_file: PathBuf::from("/tmp/job/prompt.md"),
    });
    assert!(scoped_bash.is_err());
}

#[test]
fn opencode_worker_argv_sets_auto_prompt_and_env_and_refuses_read_only() {
    let adapter = OpencodeAdapter::new();
    let req = WorkerLaunchRequest {
        model: "anthropic/claude-sonnet-4-6".to_string(),
        tools: vec![
            ToolFlag::Allow("Read".to_string()),
            ToolFlag::Allow("Edit".to_string()),
        ],
        read_only: false,
        prompt_file: PathBuf::from("/tmp/job/prompt.md"),
    };
    let spec = adapter.build_worker_argv(&req).unwrap();
    assert_eq!(spec.argv[0], "opencode");
    assert!(spec.argv.contains(&"--auto".to_string()));
    assert!(
        spec.argv
            .windows(2)
            .any(|w| w[0] == "--prompt" && w[1].contains("/tmp/job/prompt.md"))
    );
    assert_eq!(
        spec.env.get("OPENCODE_CONFIG_CONTENT").map(String::as_str),
        Some(r#"{"model":"anthropic/claude-sonnet-4-6"}"#)
    );

    let ro = adapter.build_worker_argv(&WorkerLaunchRequest {
        read_only: true,
        ..req.clone()
    });
    assert!(ro.is_err());

    let scoped = adapter.build_worker_argv(&WorkerLaunchRequest {
        tools: vec![ToolFlag::Allow("Bash(gh:*)".to_string())],
        ..req
    });
    assert!(scoped.is_err());
}

#[test]
fn worker_launch_watch_and_finish_tag_pane_detect_result_and_kill_pane() {
    let tmp = TempDir::new("happy");
    let job_dir = tmp.path.join("run-1.plan.1");
    let worktree = tmp.path.join("wt");
    fs::create_dir_all(&job_dir).unwrap();
    fs::create_dir_all(&worktree).unwrap();
    let prompt_file = job_dir.join("prompt.md");
    let result_path = job_dir.join("result.json");
    fs::write(&prompt_file, "# Plan\n").unwrap();

    let runner = RecordingTmuxRunner::with_pane("%42");
    runner.set_default_panes(pane_line(
        "%42",
        "fw-run-1-plan",
        "factory-worker",
        false,
        "run-1",
        "run-1.plan.1",
        "plan",
        "claude",
        "claude-sonnet-4-6",
    ));
    let tmux = TmuxClient::with_runner("tandem", None, Arc::new(runner.clone()));

    let spec = WorkerLaunchSpec {
        run_id: "run-1".to_string(),
        stage: "plan".to_string(),
        attempt: 1,
        job_dir: job_dir.clone(),
        prompt_file,
        result_path: result_path.clone(),
        worktree_path: worktree,
        assignment: WorkerAssignment {
            cli: "claude".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            read_only: true,
            tools: vec![ToolFlag::Allow("Read".to_string())],
            agent: "business-analyst".to_string(),
            role: "factory-worker".to_string(),
        },
        env: BTreeMap::new(),
        time_limit_secs: 10,
    };

    let launched = launch(&tmux, &spec).unwrap();
    assert_eq!(launched.pane_id, "%42");

    let calls = runner.calls();
    assert!(
        calls
            .iter()
            .any(|c| { c.windows(2).any(|w| w == ["new-window", "-d"]) }),
        "worker launch must pass -d to new-window so it stays detached: {calls:?}"
    );
    for (opt, val) in [
        ("@tandem_kind", "factory-worker"),
        ("@tandem_run", "run-1"),
        ("@tandem_job", "run-1.plan.1"),
        ("@tandem_stage", "plan"),
        ("@tandem_cli", "claude"),
        ("@tandem_model", "claude-sonnet-4-6"),
    ] {
        assert!(
            calls.iter().any(|c| {
                c.iter().any(|a| a == "set-option") && c.windows(2).any(|w| w == [opt, val])
            }),
            "missing tmux option {opt}={val} in {calls:?}"
        );
    }

    fs::write(
        &result_path,
        r#"{
  "status": "succeeded",
  "summary": "Planned specs",
  "files_changed": [],
  "artifacts": [],
  "no_behaviour_change": false
}"#,
    )
    .unwrap();

    let watched =
        watch_with_interval(&tmux, &launched, &|| false, Duration::from_millis(5)).unwrap();
    assert!(matches!(watched, WorkerWatchOutcome::Completed(_)));
    let job_result = finish(&tmux, &launched, watched).unwrap();
    assert_eq!(job_result.status, JobResultStatus::Succeeded);
    assert!(
        runner.calls().iter().any(|c| c.ends_with(&[
            "kill-pane".to_string(),
            "-t".to_string(),
            "%42".to_string()
        ])),
        "finish must kill worker pane"
    );
}

#[test]
fn worker_finish_prepares_main_slot_when_visible_worker_closes() {
    #[derive(Default)]
    struct FinishSwapState {
        calls: Vec<Vec<String>>,
    }
    #[derive(Clone, Default)]
    struct FinishSwapRunner {
        state: Arc<Mutex<FinishSwapState>>,
    }
    impl TmuxRunner for FinishSwapRunner {
        fn run(&self, args: &[String]) -> TmuxCmdResult {
            self.state.lock().unwrap().calls.push(args.to_vec());
            let rest = if args.len() >= 3 && args[0] == "tmux" && args[1] == "-L" {
                if args.get(3).map(String::as_str) == Some("-f") {
                    &args[5..]
                } else {
                    &args[3..]
                }
            } else {
                args
            };
            match rest.first().map(String::as_str) {
                Some("list-panes") => TmuxCmdResult {
                    code: 0,
                    out: [
                        "%1\tmain\town\tsidebar\t0\t\t0\t\t",
                        "%42\tmain\tfactory-worker\tbackend\t1\tdevelop·1\t0\t\t\t\t\t\t\trun-1\trun-1.develop.1\tdevelop\tclaude\tclaude-sonnet-4-6",
                        "%3\tmain\town\tstatus\t0\t\t0\t\t",
                        "%2\tstash-orchestrator-1\tagent\torchestrator\t1\tclaude\t0\t\tsess-orch",
                    ]
                    .join("\n"),
                    err: String::new(),
                },
                Some("display-message") => TmuxCmdResult {
                    code: 0,
                    out: "%42\t160\t40\n".to_string(),
                    err: String::new(),
                },
                _ => TmuxCmdResult {
                    code: 0,
                    out: String::new(),
                    err: String::new(),
                },
            }
        }
    }

    let tmp = TempDir::new("finish-swap");
    let job_dir = tmp.path.join("run-1.develop.1");
    fs::create_dir_all(&job_dir).unwrap();
    let result_path = job_dir.join("result.json");
    let worker = tandem::factory::worker::LaunchedWorker {
        pane_id: "%42".to_string(),
        run_id: "run-1".to_string(),
        stage: "develop".to_string(),
        attempt: 1,
        cli: "claude".to_string(),
        model: "claude-sonnet-4-6".to_string(),
        job_dir,
        result_path,
        time_limit_secs: 10,
    };
    let runner = FinishSwapRunner::default();
    let tmux = TmuxClient::with_runner("tandem", None, Arc::new(runner.clone()));
    let result = tandem::factory::broker::result::JobResult {
        status: JobResultStatus::Succeeded,
        summary: "done".to_string(),
        files_changed: Vec::new(),
        artifacts: Vec::new(),
        rationale: None,
        hypothesis: None,
        usage: None,
        no_behaviour_change: false,
    };

    let _ = finish(&tmux, &worker, WorkerWatchOutcome::Completed(result)).unwrap();
    let calls = runner.state.lock().unwrap().calls.clone();
    let swap_idx = calls
        .iter()
        .position(|c| {
            c.windows(5)
                .any(|w| w == ["swap-pane", "-d", "-s", "%2", "-t"])
        })
        .expect("finish must swap fallback %2 into main before killing visible %42");
    let kill_idx = calls
        .iter()
        .position(|c| c.ends_with(&["kill-pane".to_string(), "-t".to_string(), "%42".to_string()]))
        .expect("finish must still kill %42");
    assert!(
        swap_idx < kill_idx,
        "swap must precede kill-pane: {calls:?}"
    );
}

#[test]
fn worker_watch_handles_exit_without_result_timeout_and_stop() {
    let tmp = TempDir::new("failures");
    let job_dir = tmp.path.join("job");
    let worktree = tmp.path.join("wt");
    fs::create_dir_all(&job_dir).unwrap();
    fs::create_dir_all(&worktree).unwrap();
    let prompt_file = job_dir.join("prompt.md");
    fs::write(&prompt_file, "# Dev\n").unwrap();

    let base_spec = |suffix: &str, limit: u64| -> WorkerLaunchSpec {
        let d = job_dir.join(suffix);
        fs::create_dir_all(&d).unwrap();
        WorkerLaunchSpec {
            run_id: "run-1".to_string(),
            stage: "develop".to_string(),
            attempt: 1,
            job_dir: d.clone(),
            prompt_file: prompt_file.clone(),
            result_path: d.join("result.json"),
            worktree_path: worktree.clone(),
            assignment: WorkerAssignment {
                cli: "claude".to_string(),
                model: "claude-sonnet-4-6".to_string(),
                read_only: false,
                tools: vec![
                    ToolFlag::Allow("Read".to_string()),
                    ToolFlag::Allow("Edit".to_string()),
                ],
                agent: "senior-backend-dev".to_string(),
                role: "factory-worker".to_string(),
            },
            env: BTreeMap::new(),
            time_limit_secs: limit,
        }
    };

    // 1. Pane exits cleanly without result.json
    let r1 = RecordingTmuxRunner::with_pane("%51");
    r1.set_default_panes(pane_line(
        "%51",
        "fw",
        "factory-worker",
        true,
        "run-1",
        "run-1.develop.1",
        "develop",
        "claude",
        "claude-sonnet-4-6",
    ));
    let t1 = TmuxClient::with_runner("tandem", None, Arc::new(r1));
    let l1 = launch(&t1, &base_spec("dead", 5)).unwrap();
    let o1 = watch_with_interval(&t1, &l1, &|| false, Duration::from_millis(5)).unwrap();
    assert!(matches!(o1, WorkerWatchOutcome::ExitedWithoutResult { .. }));

    // 2. Stop requested interrupts and kills pane
    let r2 = RecordingTmuxRunner::with_pane("%52");
    r2.set_default_panes(pane_line(
        "%52",
        "fw",
        "factory-worker",
        false,
        "run-1",
        "run-1.develop.1",
        "develop",
        "claude",
        "claude-sonnet-4-6",
    ));
    let t2 = TmuxClient::with_runner("tandem", None, Arc::new(r2.clone()));
    let l2 = launch(&t2, &base_spec("stop", 5)).unwrap();
    let o2 = watch_with_interval(&t2, &l2, &|| true, Duration::from_millis(5)).unwrap();
    assert!(matches!(o2, WorkerWatchOutcome::Interrupted));
    assert!(r2.calls().iter().any(|c| c.ends_with(&[
        "kill-pane".to_string(),
        "-t".to_string(),
        "%52".to_string()
    ])));

    // 3. Timeout kills pane and reports TimedOut
    let r3 = RecordingTmuxRunner::with_pane("%53");
    r3.push_panes_response(pane_line(
        "%53",
        "fw",
        "factory-worker",
        false,
        "run-1",
        "run-1.develop.1",
        "develop",
        "claude",
        "claude-sonnet-4-6",
    ));
    let t3 = TmuxClient::with_runner("tandem", None, Arc::new(r3.clone()));
    let l3 = launch(&t3, &base_spec("timeout", 0)).unwrap();
    let o3 = watch_with_interval(&t3, &l3, &|| false, Duration::from_millis(5)).unwrap();
    assert!(matches!(o3, WorkerWatchOutcome::TimedOut { .. }));
    assert!(r3.calls().iter().any(|c| c.ends_with(&[
        "kill-pane".to_string(),
        "-t".to_string(),
        "%53".to_string()
    ])));
}

#[test]
fn interactive_job_usage_marks_cost_unknown_and_skips_cost_cap_but_keeps_duration() {
    let tmp = TempDir::new("budget");
    for args in [
        ["init", "-b", "trunk"].as_slice(),
        ["config", "user.email", "t@example.com"].as_slice(),
        ["config", "user.name", "Test"].as_slice(),
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&tmp.path)
                .status()
                .unwrap()
                .success()
        );
    }
    fs::write(tmp.path.join("README.md"), "# Test\n").unwrap();
    for args in [
        ["add", "README.md"].as_slice(),
        ["commit", "-m", "init"].as_slice(),
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&tmp.path)
                .status()
                .unwrap()
                .success()
        );
    }

    let store = FactoryStore::new(&tmp.path);
    store.ensure().unwrap();
    let user = Actor {
        kind: ActorKind::User,
        id: "alice".to_string(),
    };
    let orch = Actor {
        kind: ActorKind::Orchestrator,
        id: "orch".to_string(),
    };
    let sys = Actor {
        kind: ActorKind::System,
        id: "runner".to_string(),
    };
    let idea = store
        .create_idea(
            "Interactive cost test",
            "Verify cost unknown",
            "alice",
            &user,
        )
        .unwrap();
    store
        .execute(&idea.id, Command::StartDiscovery, &orch)
        .unwrap();
    let feas = tmp.path.join("feas.md");
    fs::write(&feas, "# Feasible\n").unwrap();
    let feas_ref = artifacts::put(&store, &idea.id, "feasibility", &feas).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: feas_ref,
                verdict: Some(FeasibilityVerdict::Feasible),
                risk: Some(Risk::Low),
                workflow: None,
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();
    store
        .execute(&idea.id, Command::SelectDirection, &user)
        .unwrap();
    let plan = tmp.path.join("plan.md");
    fs::write(&plan, "# Plan\n## Scope\n## Tasks\n").unwrap();
    let plan_ref = artifacts::put(&store, &idea.id, "plan", &plan).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".to_string(),
                artifact: plan_ref,
                verdict: None,
                risk: None,
                workflow: Some("default-delivery".to_string()),
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();
    let caller = Caller::cli(true, &|_| None, &|_| None);
    mark_ready_with_options(
        &store,
        &tmp.path,
        &idea.id,
        &idea.id,
        "alice",
        MarkReadyOptions::default(),
        &caller,
    )
    .unwrap();
    let run = create_run(
        &store,
        &tmp.path,
        &idea.id,
        CreateRunOptions::default(),
        &sys,
    )
    .unwrap();

    let mut prices = BTreeMap::new();
    prices.insert(
        "claude-sonnet-4-6".to_string(),
        ModelPrice {
            input: 3.0,
            output: 15.0,
            as_of: "2026-11-20".to_string(),
        },
    );
    let usage = JobUsage {
        input_tokens: Some(100_000),
        output_tokens: Some(50_000),
        cost_usd: Some(999.0),
    };
    let ev = record_interactive_job_usage(RecordJobUsageInput {
        store: &store,
        idea_id: &idea.id,
        run_id: &run.id,
        stage: "plan",
        attempt: 1,
        provider: "claude",
        model: "claude-sonnet-4-6",
        tier: ModelTier::Standard,
        duration_secs: 45.0,
        usage: Some(&usage),
        price_table: &prices,
        actor: &sys,
    })
    .unwrap();
    assert_eq!(ev.data["cost"]["basis"], "unknown");
    assert!(ev.data["cost"].get("value").is_none());
    assert!(ev.data.get("cost_usd").is_none());

    let started = tandem::factory::events::Event::from_data(
        run.created + 1.0,
        sys.clone(),
        idea.id.clone(),
        Some(run.id.clone()),
        &EventData::StageStarted {
            run: run.id.clone(),
            stage: "plan".to_string(),
            attempt: 1,
            grant: None,
        },
    )
    .unwrap();
    let finished = tandem::factory::events::Event::from_data(
        run.created + 46.0,
        sys,
        idea.id.clone(),
        Some(run.id.clone()),
        &EventData::StageSucceeded {
            run: run.id.clone(),
            stage: "plan".to_string(),
        },
    )
    .unwrap();
    append(
        &store.root.join("events.jsonl"),
        &store.root.join(".lock"),
        &[
            redact_event(started, &RedactContext::new()),
            redact_event(finished, &RedactContext::new()),
        ],
    )
    .unwrap();

    let log = read_all(&store.root.join("events.jsonl")).unwrap();
    let s = spent(&run, &log.events, run.created + 50.0);
    assert_eq!(s.cost_usd, None);
    assert_eq!(s.duration_secs, 45);

    let cost_cap_only = RunBudget {
        max_cost_usd: Some(0.01),
        max_duration_secs: None,
        max_retries: None,
    };
    assert_eq!(check(&run.id, &cost_cap_only, &s, false), BudgetCheck::Ok);

    let dur_cap = RunBudget {
        max_cost_usd: Some(0.01),
        max_duration_secs: Some(30),
        max_retries: None,
    };
    assert!(matches!(
        check(&run.id, &dur_cap, &s, false),
        BudgetCheck::Breach(b) if b.cap == BudgetCapKind::Duration
    ));
}

#[test]
fn recovery_diagnose_and_fix_detect_and_kill_orphaned_worker_panes() {
    let tmp = TempDir::new("orphan-pane");
    let store = FactoryStore::new(&tmp.path);
    store.ensure().unwrap();

    let runner = RecordingTmuxRunner::with_pane("%77");
    runner.set_default_panes(pane_line(
        "%77",
        "fw-stale",
        "factory-worker",
        false,
        "run-missing",
        "run-missing.develop.1",
        "develop",
        "claude",
        "claude-sonnet-4-6",
    ));
    let tmux = TmuxClient::with_runner("tandem", None, Arc::new(runner.clone()));

    let diag = diagnose_with_tmux(&store, &tmp.path, &tmux).unwrap();
    assert!(
        diag.findings
            .iter()
            .any(|f: &DoctorFinding| f.kind == ProblemKind::OrphanWorkerPane && !f.repaired)
    );
    assert_eq!(diag.orphan_worker_panes.len(), 1);
    assert_eq!(diag.orphan_worker_panes[0].pane_id, "%77");

    let caller = Caller::cli(true, &|_| None, &|_| None);
    let mut input = std::io::Cursor::new(b"" as &[u8]);
    let mut out = Vec::new();
    let fixed =
        doctor_fix_with_tmux(&store, &tmp.path, &caller, &mut input, &mut out, &tmux).unwrap();
    assert!(fixed.orphan_worker_panes.iter().all(|p| p.killed));
    assert!(
        runner.calls().iter().any(|c| c.ends_with(&[
            "kill-pane".to_string(),
            "-t".to_string(),
            "%77".to_string()
        ])),
        "doctor --fix must kill orphaned factory-worker panes"
    );
}
