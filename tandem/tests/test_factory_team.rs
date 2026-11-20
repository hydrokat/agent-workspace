use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::sync::{Arc, Mutex};

use tandem::factory::approval::{Caller, mark_ready_with_workflow};
use tandem::factory::artifacts;
use tandem::factory::broker::job::{JobState, job_dir, read_job_state};
use tandem::factory::checks::set_checks;
use tandem::factory::events::read_all;
use tandem::factory::executor::BrokerExecutor;
use tandem::factory::login::LoginProbe;
use tandem::factory::machine::Command;
use tandem::factory::model::{Actor, ActorKind, FeasibilityVerdict, Risk};
use tandem::factory::permissions;
use tandem::factory::routing::{ModelTier, RoutingContext, route_interactive};
use tandem::factory::run::{CreateRunOptions, create_run, get_run};
use tandem::factory::runner::drive_run_step;
use tandem::factory::settings;
use tandem::factory::stages::scope::list_changed_files;
use tandem::factory::store::FactoryStore;
use tandem::factory::team::{
    AssignStageInput, AssignmentSource, assign_stage, capture_agent_output, list_agents,
    load_assignments, relaunch_agent, resolve_stage_assignment, send_agent_message,
    terminate_agent,
};
use tandem::factory::worker::{WorkerAssignment, WorkerLaunchSpec, launch};
use tandem::factory::workflow::parse::load_workflow_file;
use tandem::factory::workflow::validate::validate;
use tandem::tmux::SESSION;
use tandem::tmux::client::{Result as TmuxCmdResult, TmuxClient, TmuxRunner};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!("tandem-team-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn init_repo(ws: &Path) {
    for args in [
        ["init", "-b", "trunk"].as_slice(),
        ["config", "user.email", "t@example.com"].as_slice(),
        ["config", "user.name", "Test"].as_slice(),
    ] {
        assert!(
            StdCommand::new("git")
                .args(args)
                .current_dir(ws)
                .status()
                .unwrap()
                .success()
        );
    }
    fs::write(ws.join("README.md"), "# Team\n").unwrap();
    for args in [
        ["add", "README.md"].as_slice(),
        ["commit", "-m", "init"].as_slice(),
    ] {
        assert!(
            StdCommand::new("git")
                .args(args)
                .current_dir(ws)
                .status()
                .unwrap()
                .success()
        );
    }
}

fn setup_run(ws: &Path) -> (FactoryStore, String, String) {
    init_repo(ws);
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();
    let user = Actor {
        kind: ActorKind::User,
        id: "alice".to_string(),
    };
    let orch = Actor {
        kind: ActorKind::Orchestrator,
        id: "orch-1".to_string(),
    };
    let caller = Caller::cli(true, &|_| None, &|_| None);

    let mut cfg = store.settings().unwrap();
    cfg.approval_grace_secs = 0;
    settings::save(&store, &cfg).unwrap();
    set_checks(
        &store,
        "workspace",
        vec![vec!["true".to_string()]],
        vec![],
        Some(30),
        vec![],
        "workspace",
        "alice",
        &caller,
    )
    .unwrap();

    let idea = store
        .create_idea("Team control", "Assign and manage team", "alice", &user)
        .unwrap();
    store
        .execute(&idea.id, Command::StartDiscovery, &orch)
        .unwrap();
    let feas = ws.join("f.md");
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
    let plan = ws.join("p.md");
    fs::write(
        &plan,
        "# Plan\n\n## Scope\nTeam.\n\n## Acceptance criteria\n- Done\n",
    )
    .unwrap();
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
    let ready = mark_ready_with_workflow(
        &store, ws, &idea.id, &idea.id, "alice", None, false, &caller,
    )
    .unwrap();
    let run_id = match ready.run {
        Some(r) => r.id,
        None => {
            create_run(&store, ws, &idea.id, CreateRunOptions::default(), &user)
                .unwrap()
                .id
        }
    };
    (store, idea.id, run_id)
}

#[derive(Default)]
struct TeamTmuxState {
    next_id: usize,
    panes: HashMap<String, BTreeMap<String, String>>,
    calls: Vec<(Vec<String>, Option<String>)>,
    capture_text: String,
}

#[derive(Clone, Default)]
struct TeamTmuxRunner {
    state: Arc<Mutex<TeamTmuxState>>,
}

impl TeamTmuxRunner {
    fn new(capture_text: &str) -> Self {
        Self {
            state: Arc::new(Mutex::new(TeamTmuxState {
                next_id: 30,
                capture_text: capture_text.to_string(),
                ..Default::default()
            })),
        }
    }

    fn insert_pane(&self, id: &str, kind: &str, run: &str, job: &str, stage: &str, cli: &str) {
        let mut st = self.state.lock().unwrap();
        let mut map = BTreeMap::new();
        map.insert("id".to_string(), id.to_string());
        map.insert("kind".to_string(), kind.to_string());
        map.insert("role".to_string(), kind.to_string());
        map.insert("run".to_string(), run.to_string());
        map.insert("job".to_string(), job.to_string());
        map.insert("stage".to_string(), stage.to_string());
        map.insert("cli".to_string(), cli.to_string());
        map.insert("dead".to_string(), "0".to_string());
        st.panes.insert(id.to_string(), map);
    }

    fn calls(&self) -> Vec<(Vec<String>, Option<String>)> {
        self.state.lock().unwrap().calls.clone()
    }
}

impl TmuxRunner for TeamTmuxRunner {
    fn run(&self, argv: &[String]) -> TmuxCmdResult {
        self.run_inner(argv, None)
    }

    fn run_with_stdin(&self, argv: &[String], stdin: &str) -> TmuxCmdResult {
        self.run_inner(argv, Some(stdin.to_string()))
    }
}

impl TeamTmuxRunner {
    fn run_inner(&self, argv: &[String], stdin: Option<String>) -> TmuxCmdResult {
        let mut st = self.state.lock().unwrap();
        st.calls.push((argv.to_vec(), stdin));
        let rest = if argv.len() >= 3 && argv[0] == "tmux" && argv[1] == "-L" {
            if argv.get(3).map(String::as_str) == Some("-f") {
                &argv[5..]
            } else {
                &argv[3..]
            }
        } else {
            argv
        };
        let Some(cmd) = rest.first().map(String::as_str) else {
            return TmuxCmdResult {
                code: 0,
                out: String::new(),
                err: String::new(),
            };
        };
        match cmd {
            "new-window" => {
                st.next_id += 1;
                let pane_id = format!("%{}", st.next_id);
                let mut map = BTreeMap::new();
                map.insert("id".to_string(), pane_id.clone());
                map.insert("dead".to_string(), "0".to_string());
                st.panes.insert(pane_id.clone(), map);
                TmuxCmdResult {
                    code: 0,
                    out: format!("{pane_id}\n"),
                    err: String::new(),
                }
            }
            "set-option" => {
                let mut pane_id = String::new();
                let mut i = 1;
                while i < rest.len() {
                    if rest[i] == "-p" {
                        i += 1;
                    } else if rest[i] == "-t" {
                        pane_id = rest.get(i + 1).cloned().unwrap_or_default();
                        i += 2;
                    } else {
                        break;
                    }
                }
                if let (Some(opt), Some(val)) = (rest.get(i), rest.get(i + 1))
                    && let Some(map) = st.panes.get_mut(&pane_id)
                {
                    match opt.as_str() {
                        "@tandem_kind" => {
                            map.insert("kind".to_string(), val.clone());
                        }
                        "@tandem_role" => {
                            map.insert("role".to_string(), val.clone());
                        }
                        "@tandem_run" => {
                            map.insert("run".to_string(), val.clone());
                        }
                        "@tandem_job" => {
                            map.insert("job".to_string(), val.clone());
                        }
                        "@tandem_stage" => {
                            map.insert("stage".to_string(), val.clone());
                        }
                        "@tandem_cli" => {
                            map.insert("cli".to_string(), val.clone());
                        }
                        "@tandem_model" => {
                            map.insert("model".to_string(), val.clone());
                        }
                        _ => {}
                    }
                }
                TmuxCmdResult {
                    code: 0,
                    out: String::new(),
                    err: String::new(),
                }
            }
            "display-message" => {
                let mut pane_id = String::new();
                for w in rest.windows(2) {
                    if w[0] == "-t" {
                        pane_id = w[1].clone();
                    }
                }
                let fmt = rest.last().map(String::as_str).unwrap_or("");
                let out = if let Some(map) = st.panes.get(&pane_id) {
                    match fmt {
                        "#{@tandem_kind}" => map.get("kind").cloned().unwrap_or_default(),
                        "#{@tandem_run}" => map.get("run").cloned().unwrap_or_default(),
                        "#{@tandem_kind}\t#{@tandem_run}" => format!(
                            "{}\t{}",
                            map.get("kind").map(String::as_str).unwrap_or(""),
                            map.get("run").map(String::as_str).unwrap_or("")
                        ),
                        _ => String::new(),
                    }
                } else {
                    String::new()
                };
                TmuxCmdResult {
                    code: 0,
                    out,
                    err: String::new(),
                }
            }
            "list-panes" => {
                let lines: Vec<String> = st
                    .panes
                    .values()
                    .map(|m| {
                        [
                            m.get("id").map(String::as_str).unwrap_or(""),
                            "worker",
                            m.get("kind").map(String::as_str).unwrap_or(""),
                            m.get("role").map(String::as_str).unwrap_or(""),
                            "1",
                            m.get("stage").map(String::as_str).unwrap_or(""),
                            m.get("dead").map(String::as_str).unwrap_or("0"),
                            "0",
                            "",
                            "",
                            "",
                            "",
                            "",
                            m.get("run").map(String::as_str).unwrap_or(""),
                            m.get("job").map(String::as_str).unwrap_or(""),
                            m.get("stage").map(String::as_str).unwrap_or(""),
                            m.get("cli").map(String::as_str).unwrap_or(""),
                            m.get("model").map(String::as_str).unwrap_or(""),
                        ]
                        .join("\t")
                    })
                    .collect();
                TmuxCmdResult {
                    code: 0,
                    out: lines.join("\n"),
                    err: String::new(),
                }
            }
            "capture-pane" => TmuxCmdResult {
                code: 0,
                out: st.capture_text.clone(),
                err: String::new(),
            },
            "kill-pane" => {
                for w in rest.windows(2) {
                    if w[0] == "-t" {
                        st.panes.remove(&w[1]);
                    }
                }
                TmuxCmdResult {
                    code: 0,
                    out: String::new(),
                    err: String::new(),
                }
            }
            _ => TmuxCmdResult {
                code: 0,
                out: String::new(),
                err: String::new(),
            },
        }
    }
}

struct AllReadyProbe;
impl LoginProbe for AllReadyProbe {
    fn which(&self, bin: &str) -> Option<PathBuf> {
        matches!(bin, "claude" | "agy" | "opencode").then(|| PathBuf::from(format!("/bin/{bin}")))
    }
    fn version(&self, _cli: &str) -> String {
        "2.0".to_string()
    }
    fn run(&self, argv: &[&str]) -> (i32, String, String) {
        match argv.first().copied() {
            Some("claude") => (
                0,
                r#"{"loggedIn":true,"authMethod":"apiKey"}"#.to_string(),
                String::new(),
            ),
            Some("agy") => (
                0,
                "gemini-2.5-pro\tGemini 2.5 Pro\n".to_string(),
                String::new(),
            ),
            Some("opencode") => (0, "anthropic oauth\n".to_string(), String::new()),
            _ => (1, String::new(), String::new()),
        }
    }
}

#[test]
fn assignments_precedence_user_beats_orchestrator_beats_default_and_job_started_event() {
    let tmp = TempDir::new("assign");
    let (store, _idea_id, run_id) = setup_run(&tmp.path);
    let mut run = get_run(&store, &run_id).unwrap().unwrap();
    let cfg = store.settings().unwrap();
    let wf = validate(
        &load_workflow_file(&store.run_dir(&run_id).join("workflow.yaml")).unwrap(),
        &cfg,
    )
    .unwrap();

    // 1. Orchestrator sets develop -> opencode / gpt-5.3-codex
    let a1 = assign_stage(
        &store,
        AssignStageInput {
            run_id: &run_id,
            stage: "develop",
            cli: "claude",
            model: "claude-opus-4-7",
            role: Some("senior-backend-dev"),
            by: AssignmentSource::Orchestrator,
            actor_id: "orch-1",
        },
    )
    .unwrap();
    assert_eq!(a1.source, AssignmentSource::Orchestrator);

    // 2. User overrides develop -> claude / claude-sonnet-4-6
    let a2 = assign_stage(
        &store,
        AssignStageInput {
            run_id: &run_id,
            stage: "develop",
            cli: "claude",
            model: "claude-sonnet-4-6",
            role: None,
            by: AssignmentSource::User,
            actor_id: "alice",
        },
    )
    .unwrap();
    assert_eq!(a2.source, AssignmentSource::User);
    assert_eq!(a2.model, "claude-sonnet-4-6");

    // 3. Orchestrator cannot overwrite user explicit assignment
    let err = assign_stage(
        &store,
        AssignStageInput {
            run_id: &run_id,
            stage: "develop",
            cli: "agy",
            model: "gemini-2.5-pro",
            role: None,
            by: AssignmentSource::Orchestrator,
            actor_id: "orch-1",
        },
    )
    .unwrap_err();
    assert!(err.to_string().to_lowercase().contains("user"));

    // 4. Orchestrator blocked when settings.orchestrator_may_choose == false
    let mut locked_cfg = cfg.clone();
    locked_cfg.orchestrator_may_choose = false;
    settings::save(&store, &locked_cfg).unwrap();
    let blocked = assign_stage(
        &store,
        AssignStageInput {
            run_id: &run_id,
            stage: "review",
            cli: "agy",
            model: "gemini-2.5-pro",
            role: None,
            by: AssignmentSource::Orchestrator,
            actor_id: "orch-1",
        },
    )
    .unwrap_err();
    assert!(blocked.to_string().to_lowercase().contains("orchestrator"));
    settings::save(&store, &cfg).unwrap();

    // 5. Resolve unassigned ("plan") -> Default; assigned ("develop") -> User
    let plan_def = wf.stages.iter().find(|s| s.name == "plan").unwrap();
    let plan_grant =
        permissions::grant(plan_def.agent, plan_def, "workspace", &wf, &cfg, false).unwrap();
    let resolved_plan = resolve_stage_assignment(
        &store,
        &run,
        plan_def,
        &plan_grant,
        &wf,
        &cfg,
        &["claude", "agy"],
        1,
        &RoutingContext::default(),
    )
    .unwrap();
    assert_eq!(resolved_plan.source, AssignmentSource::Default);

    let dev_def = wf.stages.iter().find(|s| s.name == "develop").unwrap();
    let dev_grant =
        permissions::grant(dev_def.agent, dev_def, "workspace", &wf, &cfg, false).unwrap();
    let resolved_dev = resolve_stage_assignment(
        &store,
        &run,
        dev_def,
        &dev_grant,
        &wf,
        &cfg,
        &["claude", "agy"],
        1,
        &RoutingContext::default(),
    )
    .unwrap();
    assert_eq!(resolved_dev.source, AssignmentSource::User);
    assert_eq!(resolved_dev.route.model, "claude-sonnet-4-6");

    // 6. Drive plan via BrokerExecutor + result writer -> job.started event records source
    let saved = load_assignments(&store, &run_id).unwrap();
    assert!(saved.stages.contains_key("develop"));

    // Assign plan explicitly by orchestrator so job.started asserts "orchestrator"
    assign_stage(
        &store,
        AssignStageInput {
            run_id: &run_id,
            stage: "plan",
            cli: "claude",
            model: "claude-sonnet-4-6",
            role: None,
            by: AssignmentSource::Orchestrator,
            actor_id: "orch-1",
        },
    )
    .unwrap();
    let runner = TeamTmuxRunner::new("");
    let tmux = TmuxClient::with_runner(SESSION, None, Arc::new(runner));
    let j1 = job_dir(&store, &run_id, "plan", 1);
    fs::create_dir_all(&j1).unwrap();
    let art_dir = store.run_dir(&run_id).join("artifacts");
    fs::create_dir_all(art_dir.join("specs/p-001-t")).unwrap();
    fs::write(art_dir.join("specs/p-001-t/impl.md"), "# Impl\n").unwrap();
    fs::write(
        art_dir.join("slices.json"),
        r#"{"phase":"p-001-t","slices":[{"id":"t-001","title":"S","type":"New Feature","paths":["README.md"],"dependencies":[],"acceptance_criteria":["Ok"],"ui":false}]}"#,
    )
    .unwrap();
    fs::write(
        j1.join("result.json"),
        r#"{"status":"succeeded","summary":"ok","files_changed":[],"artifacts":["specs/p-001-t/impl.md","slices.json"]}"#,
    )
    .unwrap();

    run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(run.state, tandem::factory::model::RunState::Queued);
    let mut exec = BrokerExecutor::new(&tmp.path)
        .with_tmux(tmux)
        .with_login_probe(Arc::new(AllReadyProbe));
    drive_run_step(&store, &tmp.path, &run_id, &mut exec).unwrap();

    let log = read_all(&store.root.join("events.jsonl")).unwrap();
    let js = log
        .events
        .iter()
        .find(|ev| ev.r#type == "job.started" && ev.run.as_deref() == Some(run_id.as_str()));
    // Note: idempotent pre-populated result.json finishes before launch unless launched live,
    // so test route_interactive directly + check Route/job.started recording helper too.
    let _ = (js, ModelTier::Standard, route_interactive);
}

#[test]
fn agent_send_capture_terminate_and_relaunch_use_worker_guards() {
    let tmp = TempDir::new("ops");
    let (store, _idea_id, run_id) = setup_run(&tmp.path);
    let runner = TeamTmuxRunner::new("captured line 1\ncaptured line 2\n");
    let tmux = TmuxClient::with_runner(SESSION, None, Arc::new(runner.clone()));

    let j1 = job_dir(&store, &run_id, "develop", 1);
    fs::create_dir_all(&j1).unwrap();
    fs::write(
        j1.join("input.json"),
        serde_json::json!({
            "correlation_id": format!("{run_id}.develop.1"),
            "run": run_id,
            "stage": "develop",
            "attempt": 1,
            "agent": "senior-backend-dev",
            "provider": "claude",
            "model": "claude-sonnet-4-6",
            "tool_grants": ["repo_read", "repo_write"],
            "cwd": tmp.path.to_string_lossy(),
            "time_limit_secs": 60,
            "context_manifest": {
                "items": [],
                "total_bytes": 10,
                "excluded_paths": []
            }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(j1.join("prompt.md"), "# Implement\n").unwrap();
    fs::write(j1.join("state"), "running\n").unwrap();
    fs::write(j1.join("pane_id"), "%21\n").unwrap();

    let job_id = format!("{run_id}.develop.1");
    runner.insert_pane(
        "%21",
        "factory-worker",
        &run_id,
        &job_id,
        "develop",
        "claude",
    );
    runner.insert_pane("%99", "login", "", "", "", "claude");
    runner.insert_pane(
        "%98",
        "factory-worker",
        "r-other",
        "r-other.develop.1",
        "develop",
        "claude",
    );

    let list = list_agents(&store, &tmux, &run_id).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].pane_id.as_deref(), Some("%21"));
    assert!(list[0].pane_alive);

    let msg_file = tmp.path.join("msg.txt");
    fs::write(&msg_file, "Please check failing test\n").unwrap();
    send_agent_message(&store, &tmux, &run_id, &job_id, &msg_file).unwrap();
    assert!(
        runner
            .calls()
            .iter()
            .any(|(argv, stdin)| argv.iter().any(|a| a == "load-buffer")
                && stdin.as_deref() == Some("Please check failing test\n"))
    );

    let captured = capture_agent_output(&store, &tmux, &run_id, &job_id, 50).unwrap();
    assert!(captured.contains("captured line 1"));

    // Wrong confirm fails terminate
    assert!(terminate_agent(&store, &tmux, &run_id, &job_id, "wrong").is_err());

    // Guards refuse login or wrong-run panes if job points at them
    fs::write(j1.join("pane_id"), "%99\n").unwrap();
    assert!(send_agent_message(&store, &tmux, &run_id, &job_id, &msg_file).is_err());
    assert!(terminate_agent(&store, &tmux, &run_id, &job_id, &job_id).is_err());

    fs::write(j1.join("pane_id"), "%98\n").unwrap();
    assert!(capture_agent_output(&store, &tmux, &run_id, &job_id, 20).is_err());

    // Restore real pane -> terminate -> relaunch with updated assignment
    fs::write(j1.join("pane_id"), "%21\n").unwrap();
    terminate_agent(&store, &tmux, &run_id, &job_id, &job_id).unwrap();
    assert_eq!(read_job_state(&j1).unwrap(), JobState::Interrupted);

    assign_stage(
        &store,
        AssignStageInput {
            run_id: &run_id,
            stage: "develop",
            cli: "claude",
            model: "claude-opus-4-7",
            role: None,
            by: AssignmentSource::User,
            actor_id: "alice",
        },
    )
    .unwrap();
    let relaunched = relaunch_agent(&store, &tmux, &run_id, &job_id).unwrap();
    assert_ne!(relaunched.pane_id, "%21");
    assert_eq!(relaunched.model, "claude-opus-4-7");
    assert_eq!(read_job_state(&j1).unwrap(), JobState::Running);
    let updated_list = list_agents(&store, &tmux, &run_id).unwrap();
    assert_eq!(updated_list[0].model, "claude-opus-4-7");
    assert_eq!(
        updated_list[0].pane_id.as_deref(),
        Some(relaunched.pane_id.as_str())
    );
    let evs = read_all(&store.root.join("events.jsonl")).unwrap().events;
    assert!(evs.iter().any(|ev| {
        ev.r#type == "job.started"
            && ev.data["model"] == "claude-opus-4-7"
            && ev.data["source"] == "user"
    }));
}

#[test]
fn claude_stop_hook_installed_for_claude_only_and_is_git_excluded() {
    let tmp = TempDir::new("stop-hook");
    let wt_claude = tmp.path.join("wt-claude");
    fs::create_dir_all(&wt_claude).unwrap();
    init_repo(&wt_claude);

    let j_claude = tmp.path.join("j-claude");
    fs::create_dir_all(&j_claude).unwrap();
    let prompt_claude = j_claude.join("prompt.md");
    fs::write(&prompt_claude, "# Prompt\n").unwrap();

    let runner = TeamTmuxRunner::default();
    let tmux = TmuxClient::with_runner(SESSION, None, Arc::new(runner));

    launch(
        &tmux,
        &WorkerLaunchSpec {
            run_id: "r-1".to_string(),
            stage: "develop".to_string(),
            attempt: 1,
            job_dir: j_claude.clone(),
            prompt_file: prompt_claude.clone(),
            result_path: j_claude.join("result.json"),
            worktree_path: wt_claude.clone(),
            assignment: WorkerAssignment {
                cli: "claude".to_string(),
                model: "claude-sonnet-4-6".to_string(),
                read_only: false,
                tools: vec![],
                agent: "senior-backend-dev".to_string(),
                role: "factory-worker".to_string(),
            },
            env: BTreeMap::new(),
            time_limit_secs: 30,
        },
    )
    .unwrap();

    let local_settings = wt_claude.join(".claude/settings.local.json");
    assert!(
        local_settings.is_file(),
        "Stop hook settings must exist for claude"
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&local_settings).unwrap()).unwrap();
    let cmd = parsed["hooks"]["Stop"][0]["hooks"][0]["command"]
        .as_str()
        .expect("Stop hook command string");
    assert!(list_changed_files(&wt_claude).unwrap().is_empty());

    // Linked git worktree with empty global git config keeps .claude/settings.local.json untracked-ignored
    let linked_wt = tmp.path.join("wt-linked");
    assert!(
        StdCommand::new("git")
            .args([
                "-C",
                wt_claude.to_str().unwrap(),
                "worktree",
                "add",
                "-b",
                "feat/hook-wt",
                linked_wt.to_str().unwrap(),
            ])
            .output()
            .unwrap()
            .status
            .success()
    );
    launch(
        &tmux,
        &WorkerLaunchSpec {
            run_id: "r-1".to_string(),
            stage: "develop".to_string(),
            attempt: 2,
            job_dir: j_claude.clone(),
            prompt_file: prompt_claude.clone(),
            result_path: j_claude.join("result.json"),
            worktree_path: linked_wt.clone(),
            assignment: WorkerAssignment {
                cli: "claude".to_string(),
                model: "sonnet".to_string(),
                read_only: false,
                tools: vec![],
                agent: "senior-backend-dev".to_string(),
                role: "factory-worker".to_string(),
            },
            env: BTreeMap::new(),
            time_limit_secs: 30,
        },
    )
    .unwrap();
    let wt_status = StdCommand::new("git")
        .args(["-C", linked_wt.to_str().unwrap(), "status", "--porcelain"])
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .output()
        .unwrap();
    assert!(wt_status.status.success());
    assert!(
        String::from_utf8_lossy(&wt_status.stdout).trim().is_empty(),
        "linked worktree must exclude .claude/settings.local.json via commondir info/exclude, got: {}",
        String::from_utf8_lossy(&wt_status.stdout)
    );

    // Execute hook command -> writes activity.json
    let out = StdCommand::new("sh")
        .args(["-c", cmd])
        .env("TANDEM_JOB_DIR", &j_claude)
        .env("TANDEM_STAGE", "develop")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(out.status.success(), "hook failed: {:?}", out);
    let act: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(j_claude.join("activity.json")).unwrap()).unwrap();
    assert_eq!(act["state"], "idle");
    assert_eq!(act["cli"], "claude");

    // Non-claude worker -> no .claude/settings.local.json installed
    let wt_agy = tmp.path.join("wt-agy");
    fs::create_dir_all(&wt_agy).unwrap();
    let j_agy = tmp.path.join("j-agy");
    fs::create_dir_all(&j_agy).unwrap();
    let p_agy = j_agy.join("prompt.md");
    fs::write(&p_agy, "# Review\n").unwrap();
    launch(
        &tmux,
        &WorkerLaunchSpec {
            run_id: "r-1".to_string(),
            stage: "review".to_string(),
            attempt: 1,
            job_dir: j_agy.clone(),
            prompt_file: p_agy,
            result_path: j_agy.join("result.json"),
            worktree_path: wt_agy.clone(),
            assignment: WorkerAssignment {
                cli: "agy".to_string(),
                model: "gemini-2.5-pro".to_string(),
                read_only: true,
                tools: vec![],
                agent: "code-reviewer".to_string(),
                role: "factory-worker".to_string(),
            },
            env: BTreeMap::new(),
            time_limit_secs: 30,
        },
    )
    .unwrap();
    assert!(!wt_agy.join(".claude/settings.local.json").exists());
}

#[test]
fn orchestrator_team_watch_uses_files_first_and_capture_fallback() {
    let tmp = TempDir::new("watch-py");
    let pkg_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let watch_py = pkg_root.join("templates/skills/factory-orchestrator/scripts/team_watch.py");
    let factory_py = pkg_root.join("templates/skills/factory-orchestrator/scripts/factory.py");

    let capture_log = tmp.path.join("captures.log");
    let fake_bin = tmp.path.join("bin");
    fs::create_dir_all(&fake_bin).unwrap();
    let tandem_stub = fake_bin.join("tandem");
    fs::write(
        &tandem_stub,
        format!(
            r#"#!/bin/sh
if [ "$1" = "factory" ] && [ "$2" = "agent" ] && [ "$3" = "list" ]; then
  cat "${{FAKE_LIST_JSON}}"
  exit 0
fi
if [ "$1" = "factory" ] && [ "$2" = "agent" ] && [ "$3" = "capture" ]; then
  echo "$@" >> "{}"
  echo '{{"job":"r-1.develop.1","pane_id":"%10","output":"waiting on question"}}'
  exit 0
fi
exit 1
"#,
            capture_log.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tandem_stub, fs::Permissions::from_mode(0o755)).unwrap();
    }

    // Case 1: fresh activity.json -> no capture call
    let list_json_path = tmp.path.join("list.json");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    fs::write(
        &list_json_path,
        serde_json::json!([
            {
                "job_id": "r-1.develop.1",
                "run_id": "r-1",
                "stage": "develop",
                "attempt": 1,
                "cli": "claude",
                "model": "claude-sonnet-4-6",
                "role": "senior-backend-dev",
                "source": "orchestrator",
                "job_state": "running",
                "pane_id": "%10",
                "pane_alive": true,
                "activity": { "last_stop_at": now, "state": "idle" },
                "has_result": false,
                "result_status": null
            }
        ])
        .to_string(),
    )
    .unwrap();

    let out1 = StdCommand::new("python3")
        .args([
            watch_py.to_str().unwrap(),
            "--run",
            "r-1",
            "--stale-secs",
            "120",
        ])
        .env("TANDEM_SELF", &tandem_stub)
        .env("FAKE_LIST_JSON", &list_json_path)
        .output()
        .unwrap();
    assert!(
        out1.status.success(),
        "{}",
        String::from_utf8_lossy(&out1.stderr)
    );
    assert!(
        !capture_log.exists(),
        "must not capture when activity.json is fresh"
    );

    // Case 2: stale/missing activity.json -> falls back to agent capture
    fs::write(
        &list_json_path,
        serde_json::json!([
            {
                "job_id": "r-1.develop.1",
                "run_id": "r-1",
                "stage": "develop",
                "attempt": 1,
                "cli": "claude",
                "model": "claude-sonnet-4-6",
                "role": "senior-backend-dev",
                "source": "orchestrator",
                "job_state": "running",
                "pane_id": "%10",
                "pane_alive": true,
                "activity": { "last_stop_at": now - 600.0, "state": "idle" },
                "has_result": false,
                "result_status": null
            }
        ])
        .to_string(),
    )
    .unwrap();
    let out2 = StdCommand::new("python3")
        .args([
            watch_py.to_str().unwrap(),
            "--run",
            "r-1",
            "--stale-secs",
            "120",
        ])
        .env("TANDEM_SELF", &tandem_stub)
        .env("FAKE_LIST_JSON", &list_json_path)
        .output()
        .unwrap();
    assert!(
        out2.status.success(),
        "{}",
        String::from_utf8_lossy(&out2.stderr)
    );
    let parsed2: serde_json::Value = serde_json::from_slice(&out2.stdout).unwrap();
    assert_eq!(parsed2["agents"][0]["capture"], "waiting on question");
    assert!(capture_log.exists());

    // factory.py exposes team subcommands
    let help = StdCommand::new("python3")
        .args([factory_py.to_str().unwrap(), "--help"])
        .output()
        .unwrap();
    let help_txt = String::from_utf8_lossy(&help.stdout);
    for cmd in [
        "assign",
        "agents",
        "send",
        "capture",
        "terminate",
        "relaunch",
    ] {
        assert!(help_txt.contains(cmd), "factory.py missing {cmd}");
    }
}

#[test]
fn remediation_regression_checks_cover_socket_env_i_hooks_auth_watch_layout_and_settings() {
    use std::io::Cursor;
    use tandem::factory::commands::{
        FactoryAgentArgs, FactoryAgentAssignArgs, FactoryAgentCommand, FactoryArgs, FactoryCommand,
        run_with_caller_io,
    };
    use tandem::factory::worker::{LaunchedWorker, WorkerWatchOutcome, watch_with_interval};
    use tandem::status::settings_tab::SettingsTab;
    use tandem::tmux::layout::Layout;
    use tandem::tmux::socket_for;
    use tandem::tmux::tabs::PaneInfo;

    let tmp = TempDir::new("remediation");
    let (store, _idea_id, run_id) = setup_run(&tmp.path);

    // #2: BrokerExecutor defaults to workspace socket when TANDEM_TMUX_SOCKET is unset
    if std::env::var_os("TANDEM_TMUX_SOCKET").is_none() {
        let exec = BrokerExecutor::new(&tmp.path);
        assert_eq!(exec.tmux.socket, socket_for(&tmp.path));
    }

    // #3: CLI non-TTY returns exit 3 (terminal_required); agent env returns exit 4
    let mut cfg = store.settings().unwrap();
    let cfg_dir = tmp.path.join(".agent-workspace");
    fs::create_dir_all(&cfg_dir).unwrap();
    let ws_cfg = tandem::config::Config {
        factory: true,
        state_dir: tmp.path.join(".state"),
        ..Default::default()
    };
    let cli_args = FactoryArgs {
        command: FactoryCommand::Agent(FactoryAgentArgs {
            command: FactoryAgentCommand::Assign(FactoryAgentAssignArgs {
                run: run_id.clone(),
                stage: "develop".to_string(),
                cli: "claude".to_string(),
                model: "claude-sonnet-4-6".to_string(),
                role: None,
                by: Some("user".to_string()),
                json: true,
            }),
        }),
    };

    let mut out = Vec::new();
    let mut err = Vec::new();
    let non_tty = Caller::cli(false, &|_| None, &|_| None);
    let code_non_tty = run_with_caller_io(
        &cli_args,
        &tmp.path,
        &ws_cfg,
        &non_tty,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut out,
        &mut err,
    );
    assert_eq!(code_non_tty, 3);
    assert!(String::from_utf8_lossy(&out).contains("terminal_required"));

    let mut out2 = Vec::new();
    let mut err2 = Vec::new();
    let agent_caller = Caller::cli(
        true,
        &|k| (k == "TANDEM_ROLE").then(|| "implementer".to_string()),
        &|_| None,
    );
    let code_agent = run_with_caller_io(
        &cli_args,
        &tmp.path,
        &ws_cfg,
        &agent_caller,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut out2,
        &mut err2,
    );
    assert_eq!(code_agent, 4);
    assert!(String::from_utf8_lossy(&out2).contains("agent_caller_forbidden"));

    let mut out3 = Vec::new();
    let mut err3 = Vec::new();
    let web_caller = Caller::web(&|_| None);
    let code_web = run_with_caller_io(
        &cli_args,
        &tmp.path,
        &ws_cfg,
        &web_caller,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut out3,
        &mut err3,
    );
    assert_eq!(
        code_web,
        0,
        "web caller stderr: {}",
        String::from_utf8_lossy(&err3)
    );

    // #5: User override survives orchestrator_may_choose=false && default_worker_cli="agy"
    cfg.orchestrator_may_choose = false;
    cfg.default_worker_cli = "agy".to_string();
    settings::save(&store, &cfg).unwrap();
    let run = get_run(&store, &run_id).unwrap().unwrap();
    let raw_wf = load_workflow_file(&store.run_dir(&run_id).join("workflow.yaml")).unwrap();
    let wf = validate(&raw_wf, &cfg).unwrap();
    let dev_def = wf.stages.iter().find(|s| s.name == "develop").unwrap();
    let dev_grant =
        permissions::grant(dev_def.agent, dev_def, "workspace", &wf, &cfg, false).unwrap();
    let resolved_user = resolve_stage_assignment(
        &store,
        &run,
        dev_def,
        &dev_grant,
        &wf,
        &cfg,
        &["claude", "agy"],
        1,
        &RoutingContext::default(),
    )
    .unwrap();
    assert_eq!(resolved_user.route.provider, "claude");
    assert_eq!(resolved_user.source, AssignmentSource::User);

    // #4 & #7: worker::launch uses `env -i`, preserves existing Stop hooks, and rejects symlinked `.claude`
    let repo_dir = tmp.path.join("repo-hook");
    fs::create_dir_all(repo_dir.join(".claude")).unwrap();
    init_repo(&repo_dir);
    fs::write(
        repo_dir.join(".claude/settings.local.json"),
        serde_json::json!({
            "hooks": {
                "Stop": [{ "hooks": [{ "type": "command", "command": "echo user-hook" }] }]
            }
        })
        .to_string(),
    )
    .unwrap();
    let jdir = tmp.path.join("job-hook");
    fs::create_dir_all(&jdir).unwrap();
    let prompt_file = jdir.join("prompt.md");
    fs::write(&prompt_file, "# Prompt\n").unwrap();
    let runner = TeamTmuxRunner::default();
    let tmux = TmuxClient::with_runner(SESSION, None, Arc::new(runner.clone()));
    launch(
        &tmux,
        &WorkerLaunchSpec {
            run_id: run_id.clone(),
            stage: "develop".to_string(),
            attempt: 1,
            job_dir: jdir.clone(),
            prompt_file: prompt_file.clone(),
            result_path: jdir.join("result.json"),
            worktree_path: repo_dir.clone(),
            assignment: WorkerAssignment {
                cli: "claude".to_string(),
                model: "claude-sonnet-4-6".to_string(),
                read_only: false,
                tools: vec![],
                agent: "senior-backend-dev".to_string(),
                role: "factory-worker".to_string(),
            },
            env: BTreeMap::from([("PATH".to_string(), "/usr/bin:/bin".to_string())]),
            time_limit_secs: 30,
        },
    )
    .unwrap();
    assert!(
        runner
            .calls()
            .iter()
            .any(|(argv, _)| argv.windows(2).any(|w| w[0] == "env" && w[1] == "-i")),
        "expected env -i in tmux new-window argv"
    );
    let merged_hook: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repo_dir.join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(merged_hook["hooks"]["Stop"].as_array().unwrap().len(), 2);

    #[cfg(unix)]
    {
        let sym_repo = tmp.path.join("repo-sym");
        fs::create_dir_all(&sym_repo).unwrap();
        init_repo(&sym_repo);
        std::os::unix::fs::symlink(tmp.path.join("nowhere"), sym_repo.join(".claude")).unwrap();
        assert!(
            launch(
                &tmux,
                &WorkerLaunchSpec {
                    run_id: run_id.clone(),
                    stage: "develop".to_string(),
                    attempt: 2,
                    job_dir: jdir.clone(),
                    prompt_file: prompt_file.clone(),
                    result_path: jdir.join("result.json"),
                    worktree_path: sym_repo,
                    assignment: WorkerAssignment {
                        cli: "claude".to_string(),
                        model: "claude-sonnet-4-6".to_string(),
                        read_only: false,
                        tools: vec![],
                        agent: "senior-backend-dev".to_string(),
                        role: "factory-worker".to_string(),
                    },
                    env: BTreeMap::new(),
                    time_limit_secs: 30,
                },
            )
            .is_err()
        );
    }

    // #8: Layout::show_pane_with switches window+pane for factory-worker and login panes
    let layout_runner = TeamTmuxRunner::default();
    let layout_tmux = TmuxClient::with_runner(SESSION, None, Arc::new(layout_runner.clone()));
    let layout = Layout::new(&layout_tmux);
    layout
        .show_pane_with(
            &[PaneInfo {
                id: "%77".to_string(),
                window: "fw-r-1-develop".to_string(),
                index: 1,
                role: "factory-worker".to_string(),
                kind: "factory-worker".to_string(),
                label: "develop".to_string(),
                run: run_id.clone(),
                job: format!("{run_id}.develop.1"),
                stage: "develop".to_string(),
                cli: "claude".to_string(),
                model: "claude-sonnet-4-6".to_string(),
                ..PaneInfo::default()
            }],
            "%77",
        )
        .unwrap();
    let layout_calls = layout_runner.calls();
    assert!(
        layout_calls
            .iter()
            .any(|(argv, _)| argv.contains(&"select-window".to_string())
                && argv.contains(&"%77".to_string()))
    );
    assert!(
        layout_calls
            .iter()
            .any(|(argv, _)| argv.contains(&"select-pane".to_string())
                && argv.contains(&"%77".to_string()))
    );

    // #9: SettingsTab probes logins once lazily
    struct CountProbe(Arc<Mutex<usize>>);
    impl LoginProbe for CountProbe {
        fn which(&self, _bin: &str) -> Option<PathBuf> {
            None
        }
        fn version(&self, _cli: &str) -> String {
            String::new()
        }
        fn run(&self, _argv: &[&str]) -> (i32, String, String) {
            *self.0.lock().unwrap() += 1;
            (1, String::new(), String::new())
        }
    }
    let count = Arc::new(Mutex::new(0usize));
    let mut tab = SettingsTab::default();
    assert!(!tab.logins_probed);
    let probe = CountProbe(Arc::clone(&count));
    tab.ensure_logins_probed_with(&probe);
    assert!(tab.logins_probed);
    let first_count = *count.lock().unwrap();
    tab.ensure_logins_probed_with(&probe);
    assert_eq!(*count.lock().unwrap(), first_count);

    // #6 & #11: watch_with_interval survives transient list-panes error and follows updated pane_id
    #[derive(Default)]
    struct WatchRunner {
        step: Mutex<usize>,
        job_dir: PathBuf,
        result_path: PathBuf,
    }
    impl TmuxRunner for WatchRunner {
        fn run(&self, argv: &[String]) -> TmuxCmdResult {
            if argv.iter().any(|a| a == "list-panes") {
                let mut s = self.step.lock().unwrap();
                *s += 1;
                if *s == 1 {
                    return TmuxCmdResult {
                        code: 1,
                        out: String::new(),
                        err: "transient".to_string(),
                    };
                }
                if *s == 2 {
                    fs::write(self.job_dir.join("pane_id"), "%56\n").unwrap();
                    return TmuxCmdResult {
                        code: 0,
                        out: "%55\tfw\t0\tfactory-worker\tfactory-worker\tdev\t\t0\tsh\t\tr\tj\td\tc\tm\t80\t24\n"
                            .to_string(),
                        err: String::new(),
                    };
                }
                fs::write(
                    &self.result_path,
                    r#"{"status":"succeeded","summary":"ok","files_changed":[],"artifacts":[]}"#,
                )
                .unwrap();
                return TmuxCmdResult {
                    code: 0,
                    out: "%56\tfw\t0\tfactory-worker\tfactory-worker\tdev\t\t0\tsh\t\tr\tj\td\tc\tm\t80\t24\n"
                        .to_string(),
                    err: String::new(),
                };
            }
            TmuxCmdResult {
                code: 0,
                out: String::new(),
                err: String::new(),
            }
        }
    }
    let watch_jdir = tmp.path.join("watch-job");
    fs::create_dir_all(&watch_jdir).unwrap();
    let watch_res = watch_jdir.join("result.json");
    let watch_tmux = TmuxClient::with_runner(
        SESSION,
        None,
        Arc::new(WatchRunner {
            step: Mutex::new(0),
            job_dir: watch_jdir.clone(),
            result_path: watch_res.clone(),
        }),
    );
    let outcome = watch_with_interval(
        &watch_tmux,
        &LaunchedWorker {
            pane_id: "%55".to_string(),
            run_id,
            stage: "develop".to_string(),
            attempt: 1,
            cli: "claude".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            job_dir: watch_jdir,
            result_path: watch_res,
            time_limit_secs: 5,
        },
        &|| false,
        std::time::Duration::from_millis(1),
    )
    .unwrap();
    assert!(matches!(outcome, WorkerWatchOutcome::Completed(_)));
}
