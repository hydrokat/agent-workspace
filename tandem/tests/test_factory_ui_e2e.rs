mod common;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use common::TempDir;
use common::run_fixture::{full_plan_md, orch_actor, setup_run, user_actor};
use serde_json::Value;
use tandem::factory::approval::{Caller, mark_ready_with_workflow, revoke_ready};
use tandem::factory::artifacts;
use tandem::factory::broker::job::job_dir;
use tandem::factory::machine::Command as FactoryCommand;
use tandem::factory::model::{FeasibilityVerdict, IdeaState, Risk};
use tandem::factory::runner::cancel_idea;
use tandem::factory::store::FactoryStore;
use tandem::factory::worker::{
    WorkerAssignment, WorkerLaunchSpec, WorkerWatchOutcome, finish, launch,
};
use tandem::sidebar::model::{SidebarRow, rows_from_for};
use tandem::snapshot::{IdeaCard, collect_factory_summary};
use tandem::tmux::{Launch, Layout, SESSION, TmuxClient, socket_for};

struct TmuxSession {
    socket: String,
}

impl TmuxSession {
    fn start(workspace: &Path) -> Self {
        let socket = socket_for(workspace);
        let status = Command::new("tmux")
            .args([
                "-L",
                &socket,
                "new-session",
                "-d",
                "-s",
                SESSION,
                "-n",
                "status",
                "sleep",
                "120",
            ])
            .status()
            .expect("spawn real tmux session");
        assert!(status.success(), "tmux new-session failed: {status}");
        Self { socket }
    }

    fn tmux(&self, args: &[&str]) -> String {
        let out = Command::new("tmux")
            .arg("-L")
            .arg(&self.socket)
            .args(args)
            .output()
            .expect("run tmux");
        assert!(
            out.status.success(),
            "tmux {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn start_worker_pane(&self, run_id: &str, job_id: &str, stage: &str) -> String {
        let pane = self.tmux(&[
            "new-window",
            "-d",
            "-P",
            "-F",
            "#{pane_id}",
            "-n",
            "worker",
            "sleep",
            "120",
        ]);
        for (option, value) in [
            ("@tandem_kind", "factory-worker"),
            ("@tandem_role", "factory-worker"),
            ("@tandem_run", run_id),
            ("@tandem_job", job_id),
            ("@tandem_stage", stage),
            ("@tandem_cli", "claude"),
        ] {
            self.tmux(&["set-option", "-p", "-t", &pane, option, value]);
        }
        pane
    }
}

impl Drop for TmuxSession {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-L", &self.socket, "kill-server"])
            .status();
    }
}

fn run_cli_as(ws: &Path, args: &[&str], surface: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_tandem"));
    if let Some(surface) = surface {
        cmd.env("TANDEM_INTERNAL_SURFACE", surface);
    }
    cmd.args(args)
        .env("TANDEM_WORKSPACE", ws)
        .env("XDG_STATE_HOME", ws.join(".state"))
        .env("TANDEM_FACTORY", "1")
        .env_remove("TANDEM_ROLE")
        .env_remove("TANDEM_SESSION_ID")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run tandem")
}

fn run_cli(ws: &Path, args: &[&str]) -> Output {
    run_cli_as(ws, args, Some("web"))
}

fn cli_json(ws: &Path, args: &[&str]) -> Value {
    let out = run_cli(ws, args);
    assert_eq!(
        out.status.code(),
        Some(0),
        "tandem {args:?} failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("json output")
}

fn planning_idea(store: &FactoryStore, ws: &Path, title: &str, text: &str) -> String {
    let user = user_actor();
    let orch = orch_actor();
    let idea = store.create_idea(title, text, &user.id, &user).unwrap();
    store
        .execute(
            &idea.id,
            FactoryCommand::BindOrchestrator {
                cli: "claude".to_string(),
                model: "opus".to_string(),
                conversation: Some("conv-2".to_string()),
            },
            &user,
        )
        .unwrap();
    let feasibility = ws.join(format!("{}-feas.md", idea.id));
    fs::write(&feasibility, "# Feasibility\n\nFeasible.\n").unwrap();
    let feasibility_ref = artifacts::put(store, &idea.id, "feasibility", &feasibility).unwrap();
    store
        .execute(
            &idea.id,
            FactoryCommand::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: feasibility_ref,
                verdict: Some(FeasibilityVerdict::Feasible),
                risk: Some(Risk::Low),
                workflow: None,
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();
    store
        .execute(&idea.id, FactoryCommand::SelectDirection, &user)
        .unwrap();
    let plan = ws.join(format!("{}-plan.md", idea.id));
    fs::write(&plan, full_plan_md()).unwrap();
    let plan_ref = artifacts::put(store, &idea.id, "plan", &plan).unwrap();
    store
        .execute(
            &idea.id,
            FactoryCommand::StoreArtifact {
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
    idea.id
}

fn write_running_job(store: &FactoryStore, run_id: &str, pane: &str) -> String {
    let job_id = format!("{run_id}.develop.1");
    let dir = job_dir(store, run_id, "develop", 1);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("input.json"),
        serde_json::json!({
            "correlation_id": job_id,
            "run": run_id,
            "stage": "develop",
            "attempt": 1,
            "agent": "senior-backend-dev",
            "provider": "claude",
            "model": "claude-sonnet-4-6",
            "tool_grants": ["repo_read"],
            "cwd": "/tmp",
            "time_limit_secs": 60,
            "context_manifest": { "items": [], "total_bytes": 0, "excluded_paths": [] }
        })
        .to_string(),
    )
    .unwrap();
    fs::write(dir.join("state"), "running\n").unwrap();
    fs::write(dir.join("pane_id"), format!("{pane}\n")).unwrap();
    job_id
}

fn card<'a>(cards: &'a [IdeaCard], id: &str) -> &'a IdeaCard {
    cards
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("card {id}"))
}

fn vias(card: &IdeaCard) -> Vec<String> {
    card.moves
        .iter()
        .map(|m| {
            serde_json::to_value(m.via)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

#[test]
fn board_moves_agents_and_guarded_cli_actions_agree_end_to_end() {
    if !common::require_tool("tmux") {
        return;
    }
    let tmp = TempDir::new();
    let ws = tmp.path();
    let (store, running_idea, run_id) = setup_run(ws);
    let planning = planning_idea(&store, ws, "Dark mode for reports", "Add a dark theme");
    let session = TmuxSession::start(ws);

    let pane = session.start_worker_pane(&run_id, &format!("{run_id}.develop.1"), "develop");
    let job_id = write_running_job(&store, &run_id, &pane);

    let summary = collect_factory_summary(ws, &[], 1_800_000_000.0);
    assert_eq!(summary.agents_active, 1, "one running worker");
    assert_eq!(summary.columns.first().map(String::as_str), Some("inbox"));
    assert_eq!(
        vias(card(&summary.ideas, &planning)),
        vec!["ready", "cancel"]
    );
    assert_eq!(card(&summary.ideas, &planning).state, IdeaState::Planning);
    let running_moves = vias(card(&summary.ideas, &running_idea));
    assert!(
        !running_moves.contains(&"ready".to_string()),
        "an idea with a run is not offered Gate 2: {running_moves:?}"
    );

    let agents = cli_json(ws, &["factory", "agent", "list", "--all", "--json"]);
    let agents = agents.as_array().expect("array");
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["idea_id"], running_idea.as_str());
    assert_eq!(agents[0]["idea_title"], "Export team activity as CSV");
    assert_eq!(agents[0]["job_id"], job_id.as_str());
    assert_eq!(agents[0]["pane_alive"], true, "{agents:?}");
    let active = cli_json(
        ws,
        &["factory", "agent", "list", "--all", "--active", "--json"],
    );
    assert_eq!(active.as_array().unwrap().len(), 1);

    let no_terminal = run_cli(
        ws,
        &[
            "factory",
            "idea",
            "ready",
            &planning,
            "--confirm",
            &planning,
            "--json",
        ],
    );
    assert_eq!(
        no_terminal.status.code(),
        Some(3),
        "Gate 2 refuses a caller without a user surface"
    );
    let untouched = collect_factory_summary(ws, &[], 1_800_000_000.0);
    assert_eq!(card(&untouched.ideas, &planning).state, IdeaState::Planning);

    let env_fn = |k: &str| {
        if k == "TANDEM_ROLE" {
            None
        } else {
            std::env::var(k).ok()
        }
    };
    let caller = Caller::tui(&env_fn, &|_| None);
    mark_ready_with_workflow(
        &store,
        ws,
        &planning,
        &planning,
        &user_actor().id,
        Some("default-delivery"),
        false,
        &caller,
    )
    .expect("the user confirms Gate 2 from the TUI surface");
    let after = collect_factory_summary(ws, &[], 1_800_000_000.0);
    assert_ne!(card(&after.ideas, &planning).state, IdeaState::Planning);
    assert!(
        !vias(card(&after.ideas, &planning)).contains(&"ready".to_string()),
        "the board stops offering Gate 2 once the idea left Planning"
    );
    assert!(
        revoke_ready(&store, &planning, &user_actor().id, &caller).is_err(),
        "revoke is refused while a run is active"
    );
    assert!(
        !vias(card(&after.ideas, &planning)).contains(&"revoke_ready".to_string()),
        "and the board does not offer it"
    );

    let cancelled = cancel_idea(&store, &running_idea, &running_idea, &user_actor(), &caller)
        .expect("the user cancels the idea from the TUI surface");
    assert_eq!(cancelled.idea.state, IdeaState::Cancelled);
    let remaining = cli_json(
        ws,
        &["factory", "agent", "list", "--all", "--active", "--json"],
    );
    assert!(
        remaining.as_array().unwrap().is_empty(),
        "cancelling the idea stops its worker: {remaining}"
    );
    let all = cli_json(ws, &["factory", "agent", "list", "--all", "--json"]);
    assert_eq!(
        all.as_array().unwrap().len(),
        1,
        "the finished job stays in All"
    );
}

#[test]
fn an_idea_can_carry_a_description_from_creation_to_the_snapshot() {
    let tmp = TempDir::new();
    let ws = tmp.path();
    let body = ws.join("body.md");
    fs::write(&body, "Weekly CSV export.\nInclude totals.").unwrap();

    let created = cli_json(
        ws,
        &[
            "factory",
            "idea",
            "new",
            "--title",
            "Export",
            "--text-file",
            body.to_str().unwrap(),
            "--json",
        ],
    );
    let id = created["id"].as_str().unwrap().to_string();
    let shown = cli_json(ws, &["factory", "idea", "show", &id, "--json"]);
    assert_eq!(shown["idea"]["text"], "Weekly CSV export.\nInclude totals.");

    let summary = collect_factory_summary(ws, &[], 1_800_000_000.0);
    assert_eq!(card(&summary.ideas, &id).state, IdeaState::Inbox);
    assert_eq!(vias(card(&summary.ideas, &id)), vec!["cancel"]);
    assert_eq!(summary.agents_active, 0);
}

#[test]
fn factory_worker_launches_detached_swaps_and_grids_in_main_with_safe_teardown() {
    if !common::require_tool("tmux") {
        return;
    }

    struct TmuxServerGuard(String);
    impl Drop for TmuxServerGuard {
        fn drop(&mut self) {
            let _ = Command::new("tmux")
                .args(["-L", &self.0, "kill-server"])
                .status();
        }
    }

    let tmp = TempDir::new();
    let ws = tmp.path();
    let (_store, _idea_id, run_id) = setup_run(ws);

    let bin_dir = ws.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let stub_claude = bin_dir.join("claude");
    fs::write(&stub_claude, "#!/bin/sh\nexec sleep 120\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&stub_claude).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&stub_claude, perms).unwrap();
    }

    let socket = socket_for(ws);
    let _guard = TmuxServerGuard(socket.clone());
    let base_client = TmuxClient::new(&socket, None);
    let mut base_layout = Layout::new(&base_client);
    base_layout.factory = true;
    let conf = base_layout.write_conf(&ws.join("tandem.conf")).unwrap();
    let client = TmuxClient::new(&socket, Some(conf));
    let mut layout = Layout::new(&client);
    layout.factory = true;

    let sleep_cmd = || vec!["sleep".to_string(), "120".to_string()];
    let mut sb = Launch::new(sleep_cmd(), "sidebar", 0, "sidebar");
    sb.kind = "own".to_string();
    let mut orch = Launch::new(sleep_cmd(), "orchestrator", 1, "claude·opus");
    orch.session_id = "orch-1".to_string();
    let mut st = Launch::new(sleep_cmd(), "status", 0, "status");
    st.kind = "own".to_string();

    let built = layout.build(160, 45, &sb, &orch, &st).unwrap();
    let sb_pane = built["sidebar"].clone();
    let orch_pane = built["main"].clone();
    let st_pane = built["status"].clone();

    let worktree = ws.join("wt");
    let jdir = ws.join("job");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&jdir).unwrap();
    let prompt_file = jdir.join("prompt.md");
    fs::write(&prompt_file, "Implement change cleanly.\n").unwrap();

    let mut env = BTreeMap::new();
    let path_env = match std::env::var("PATH") {
        Ok(p) if !p.is_empty() => format!("{}:{p}", bin_dir.display()),
        _ => bin_dir.display().to_string(),
    };
    env.insert("PATH".to_string(), path_env);

    let launched = launch(
        &client,
        &WorkerLaunchSpec {
            run_id: run_id.clone(),
            stage: "develop".to_string(),
            attempt: 1,
            job_dir: jdir.clone(),
            prompt_file,
            result_path: jdir.join("result.json"),
            worktree_path: worktree,
            assignment: WorkerAssignment {
                cli: "claude".to_string(),
                model: "sonnet".to_string(),
                read_only: false,
                tools: Vec::new(),
                agent: "senior-backend-dev".to_string(),
                role: "senior-backend-dev".to_string(),
            },
            env,
            time_limit_secs: 60,
        },
    )
    .unwrap();

    // Active window stays `main` immediately after launch; chrome panes remain intact.
    let active_win = client
        .run_checked(&["display-message", "-p", "-t", SESSION, "#{window_name}"])
        .unwrap();
    assert_eq!(active_win.trim(), "main");

    let panes = layout.panes().unwrap();
    let main_panes: Vec<&str> = panes
        .iter()
        .filter(|p| p.window == "main")
        .map(|p| p.id.as_str())
        .collect();
    assert!(main_panes.contains(&sb_pane.as_str()));
    assert!(main_panes.contains(&orch_pane.as_str()));
    assert!(main_panes.contains(&st_pane.as_str()));
    assert!(!main_panes.contains(&launched.pane_id.as_str()));

    let summary = collect_factory_summary(ws, &[], 1_800_000_000.0);
    let rows = rows_from_for(&panes, &[], &[], Some(&orch_pane), &[], Some(&summary));
    let orch_hdr = rows
        .iter()
        .position(|r| matches!(r, SidebarRow::Header { title, .. } if title == "ORCHESTRATORS"));
    let impl_hdr = rows
        .iter()
        .position(|r| matches!(r, SidebarRow::Header { title, .. } if title == "IMPLEMENTERS"));
    let worker_pos = rows.iter().position(|r| {
        matches!(
            r,
            SidebarRow::Agent {
                pane_id,
                is_worker: true,
                ..
            } if pane_id == &launched.pane_id
        )
    });
    assert!(orch_hdr.is_some(), "missing ORCHESTRATORS header: {rows:?}");
    assert!(impl_hdr.is_some(), "missing IMPLEMENTERS header: {rows:?}");
    assert!(
        worker_pos > impl_hdr,
        "worker must appear under IMPLEMENTERS: {rows:?}"
    );

    // Clicking/selecting worker swaps it into `main` without leaving `main` full-screen.
    layout.select_agent(&launched.pane_id, false).unwrap();
    let active_win = client
        .run_checked(&["display-message", "-p", "-t", SESSION, "#{window_name}"])
        .unwrap();
    assert_eq!(active_win.trim(), "main");
    let panes = layout.panes().unwrap();
    let main_panes: Vec<&str> = panes
        .iter()
        .filter(|p| p.window == "main")
        .map(|p| p.id.as_str())
        .collect();
    assert!(main_panes.contains(&sb_pane.as_str()));
    assert!(main_panes.contains(&launched.pane_id.as_str()));
    assert!(main_panes.contains(&st_pane.as_str()));
    assert!(!main_panes.contains(&orch_pane.as_str()));

    // Multi-pane grid toggle places both orchestrator and worker inside `main`.
    layout.select_agent(&orch_pane, true).unwrap();
    let active_win = client
        .run_checked(&["display-message", "-p", "-t", SESSION, "#{window_name}"])
        .unwrap();
    assert_eq!(active_win.trim(), "main");
    let panes = layout.panes().unwrap();
    let main_panes: Vec<&str> = panes
        .iter()
        .filter(|p| p.window == "main")
        .map(|p| p.id.as_str())
        .collect();
    assert!(main_panes.contains(&sb_pane.as_str()));
    assert!(main_panes.contains(&st_pane.as_str()));
    assert!(main_panes.contains(&orch_pane.as_str()));
    assert!(main_panes.contains(&launched.pane_id.as_str()));

    // Swap back to worker-only in `main`, then finish -> `main` swaps orchestrator back cleanly.
    layout.select_agent(&launched.pane_id, false).unwrap();
    let _ = finish(&client, &launched, WorkerWatchOutcome::Interrupted);

    let active_win = client
        .run_checked(&["display-message", "-p", "-t", SESSION, "#{window_name}"])
        .unwrap();
    assert_eq!(active_win.trim(), "main");
    let panes = layout.panes().unwrap();
    assert!(!panes.iter().any(|p| p.id == launched.pane_id));
    let main_panes: Vec<&str> = panes
        .iter()
        .filter(|p| p.window == "main")
        .map(|p| p.id.as_str())
        .collect();
    assert!(main_panes.contains(&sb_pane.as_str()));
    assert!(main_panes.contains(&st_pane.as_str()));
    assert!(
        main_panes.len() >= 3,
        "workspace pane in `main` must be preserved after finish: {main_panes:?}"
    );
    let _: HashMap<String, String> = built;
}
