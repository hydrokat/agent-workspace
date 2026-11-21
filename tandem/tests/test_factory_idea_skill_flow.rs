use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;
use tandem::cli::{Cli, Command as RootCommand};
use tandem::config::Config;
use tandem::factory::approval::Caller;
use tandem::factory::artifacts::put as put_artifact;
use tandem::factory::commands::run_with_caller_io;
use tandem::factory::machine::Command;
use tandem::factory::model::{
    Actor, ActorKind, FeasibilityVerdict, Idea, IdeaState, OrchestratorRef, Risk,
};
use tandem::factory::store::FactoryStore;

fn temp_ws(name: &str) -> PathBuf {
    let dir = PathBuf::from(format!(
        "/tmp/tandem-test-idea-skill-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn orch_actor() -> Actor {
    Actor {
        kind: ActorKind::Orchestrator,
        id: "orchestrator-1".to_string(),
    }
}

fn test_config(ws: &Path) -> Config {
    let state_dir = ws.join(".state");
    fs::create_dir_all(&state_dir).unwrap();
    Config {
        workspace: Some(ws.to_path_buf()),
        state_dir,
        coord_dir: Some(ws.join(".agent-workspace").join("coordination")),
        claude_dir: ws.join(".claude"),
        session_id: "orchestrator-1".to_string(),
        tmux_pane: String::new(),
        color: false,
        update_spec: "hydrokat/agent-workspace".to_string(),
        update_interval: 14400,
        update_check: false,
        factory: true,
    }
}

#[test]
fn test_orchestrator_can_bind_and_inbox_feasibility_starts_discovery() {
    let ws = temp_ws("orch-bind-and-inbox-feas");
    let store = FactoryStore::new(&ws);
    let orch = orch_actor();

    let idea = store
        .create_idea("CSV Export", "Export the audit log as CSV", "tester", &orch)
        .unwrap();
    assert_eq!(idea.state, IdeaState::Inbox);

    let bound = store
        .execute(
            &idea.id,
            Command::BindOrchestrator {
                cli: "claude".to_string(),
                model: "claude-sonnet-4-6".to_string(),
                conversation: Some("conv-123".to_string()),
            },
            &orch,
        )
        .expect("Orchestrator actor must be allowed to execute BindOrchestrator");
    assert_eq!(
        bound.orchestrator,
        Some(OrchestratorRef {
            cli: "claude".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            conversation: Some("conv-123".to_string()),
        })
    );
    assert_eq!(bound.state, IdeaState::Inbox);

    let feas_path = ws.join("feasibility.md");
    fs::write(&feas_path, "# Feasibility\nFeasible.\n").unwrap();
    let art = put_artifact(&store, &idea.id, "feasibility", &feas_path).unwrap();

    let after_feas = store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: art,
                verdict: Some(FeasibilityVerdict::Feasible),
                risk: Some(Risk::Low),
                workflow: None,
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();
    assert_eq!(
        after_feas.state,
        IdeaState::Ideation,
        "storing a feasibility artifact on an Inbox idea must transition it to Ideation"
    );

    let after_gate1 = store
        .execute(&idea.id, Command::SelectDirection, &orch)
        .expect("SelectDirection must succeed after feasibility study on Inbox idea");
    assert_eq!(after_gate1.state, IdeaState::Planning);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_cli_idea_new_with_start_discovery_and_bind_current() {
    let ws = temp_ws("cli-new-bind");
    let cfg = test_config(&ws);

    let cli = Cli::try_parse_from([
        "tandem",
        "factory",
        "idea",
        "new",
        "--title",
        "Add dark mode",
        "--text",
        "Support dark theme toggle in settings",
        "--start-discovery",
        "--bind-current",
        "--cli",
        "agy",
        "--model",
        "gemini-2.5-pro",
        "--conversation",
        "conv-42",
        "--json",
    ])
    .expect("tandem factory idea new must accept --start-discovery, --bind-current, --cli, --model, --conversation");

    let RootCommand::Factory(f_args) = cli.command.unwrap() else {
        panic!("expected Factory command");
    };

    let env_fn = |_: &str| None;
    let pane_fn = |_: &str| None;
    let caller = Caller::cli(false, &env_fn, &pane_fn);
    let mut stdin = std::io::empty();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let code = run_with_caller_io(
        &f_args,
        &ws,
        &cfg,
        &caller,
        &mut stdin,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));

    let created: Idea = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(created.title, "Add dark mode");
    assert_eq!(created.text, "Support dark theme toggle in settings");
    assert_eq!(created.state, IdeaState::Ideation);
    assert_eq!(
        created.orchestrator,
        Some(OrchestratorRef {
            cli: "agy".to_string(),
            model: "gemini-2.5-pro".to_string(),
            conversation: Some("conv-42".to_string()),
        })
    );

    for bad_args in [
        vec![
            "tandem",
            "factory",
            "idea",
            "new",
            "--title",
            "Missing model",
            "--text",
            "Body",
            "--cli",
            "claude",
            "--json",
        ],
        vec![
            "tandem",
            "factory",
            "idea",
            "new",
            "--title",
            "Injected model",
            "--text",
            "Body",
            "--cli",
            "claude",
            "--model",
            "sonnet;rm -rf /",
            "--json",
        ],
        vec![
            "tandem",
            "factory",
            "idea",
            "new",
            "--title",
            "Unknown CLI",
            "--text",
            "Body",
            "--cli",
            "evilcli",
            "--model",
            "m1",
            "--json",
        ],
    ] {
        let bad_cli = Cli::try_parse_from(bad_args).unwrap();
        let RootCommand::Factory(bad_f_args) = bad_cli.command.unwrap() else {
            panic!("expected Factory command");
        };
        let mut bad_stdout = Vec::new();
        let mut bad_stderr = Vec::new();
        let bad_code = run_with_caller_io(
            &bad_f_args,
            &ws,
            &cfg,
            &caller,
            &mut std::io::empty(),
            &mut bad_stdout,
            &mut bad_stderr,
        );
        assert_eq!(bad_code, 1, "invalid binding arguments must fail closed");
    }

    let store = FactoryStore::new(&ws);
    assert_eq!(
        store.list_ideas().unwrap().len(),
        1,
        "rejected idea new calls must not persist partial ideas"
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_cli_idea_current_resolves_env_and_none() {
    let ws = temp_ws("cli-idea-current");
    let cfg = test_config(&ws);

    let cli = Cli::try_parse_from(["tandem", "factory", "idea", "current", "--json"])
        .expect("tandem factory idea current --json must parse");
    let RootCommand::Factory(f_args) = cli.command.unwrap() else {
        panic!("expected Factory command");
    };

    let env_with_idea = |k: &str| match k {
        "TANDEM_FACTORY_IDEA" => Some("i-7k2m9q4d".to_string()),
        _ => None,
    };
    let pane_fn = |_: &str| None;
    let caller = Caller::cli(false, &env_with_idea, &pane_fn);
    let mut stdin = std::io::empty();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let code = run_with_caller_io(
        &f_args,
        &ws,
        &cfg,
        &caller,
        &mut stdin,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));
    let val: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(val["idea"], "i-7k2m9q4d");
    assert_eq!(val["source"], "env");

    let env_empty = |_: &str| None;
    let caller_none = Caller::cli(false, &env_empty, &pane_fn);
    let mut stdout_none = Vec::new();
    let mut stderr_none = Vec::new();
    let code_none = run_with_caller_io(
        &f_args,
        &ws,
        &cfg,
        &caller_none,
        &mut std::io::empty(),
        &mut stdout_none,
        &mut stderr_none,
    );
    assert_eq!(code_none, 0);
    let val_none: serde_json::Value = serde_json::from_slice(&stdout_none).unwrap();
    assert!(val_none["idea"].is_null());
    assert_eq!(val_none["source"], "none");

    let _ = fs::remove_dir_all(&ws);
}

fn package_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn run_py_script(
    script: &Path,
    cwd: &Path,
    args: &[&str],
    env: &[(&str, &str)],
) -> serde_json::Value {
    let mut cmd = std::process::Command::new("python3");
    cmd.arg(script).args(args).current_dir(cwd);
    for k in [
        "TANDEM_FACTORY",
        "TANDEM_ROLE",
        "TANDEM_FACTORY_IDEA",
        "TANDEM_CLI",
        "TANDEM_MODEL",
        "TANDEM_SELF",
        "TANDEM_BIN",
        "TMUX_PANE",
    ] {
        cmd.env_remove(k);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("python3 must run skill script");
    assert!(
        out.status.success(),
        "script {} {:?} failed:\nstdout: {}\nstderr: {}",
        script.display(),
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("script output must be valid JSON")
}

#[test]
fn test_e2e_idea_skill_reuses_blank_inbox_and_creates_second_idea_to_gate2() {
    let ws = temp_ws("e2e-idea-skill-flow");
    let store = FactoryStore::new(&ws);
    let user_env = |_: &str| None;
    let user_pane = |_: &str| None;
    let user_caller = Caller::cli(true, &user_env, &user_pane);
    tandem::factory::settings::set(&store, "approval_grace_secs", "0", "alice", &user_caller)
        .unwrap();

    let state_dir = ws.join(".state");
    fs::create_dir_all(&state_dir).unwrap();
    tandem::registry::register_or_heartbeat_with_mode(
        &state_dir,
        &ws,
        Some(std::process::id()),
        true,
    )
    .unwrap();

    let user = Actor {
        kind: ActorKind::User,
        id: "alice".to_string(),
    };
    let blank = store.create_idea("New idea", "", "alice", &user).unwrap();
    let blank = store
        .execute(
            &blank.id,
            Command::BindOrchestrator {
                cli: "claude".to_string(),
                model: "claude-sonnet-4-6".to_string(),
                conversation: None,
            },
            &user,
        )
        .unwrap();
    assert_eq!(blank.state, IdeaState::Inbox);

    let pkg_root = package_root();
    let idea_py = pkg_root.join("templates/skills/idea/scripts/idea.py");
    let factory_py = pkg_root.join("templates/skills/factory-orchestrator/scripts/factory.py");
    let tandem_bin = env!("CARGO_BIN_EXE_tandem");

    let base_env = [
        ("TANDEM_SELF", tandem_bin),
        ("TANDEM_WORKSPACE", ws.to_str().unwrap()),
        ("TANDEM_STATE_DIR", state_dir.to_str().unwrap()),
        ("TANDEM_FACTORY", "1"),
        ("TANDEM_ROLE", "orchestrator"),
        ("TANDEM_CLI", "claude"),
        ("TANDEM_MODEL", "claude-sonnet-4-6"),
        ("TANDEM_FACTORY_IDEA", blank.id.as_str()),
    ];

    let mode_val = run_py_script(&idea_py, &ws, &["mode"], &base_env);
    assert_eq!(mode_val["mode"], "factory");

    let desc1_path = ws.join("desc1.md");
    fs::write(
        &desc1_path,
        "Export the Factory audit timeline as CSV for the last 90 days.",
    )
    .unwrap();
    let ensured1 = run_py_script(
        &factory_py,
        &ws,
        &[
            "ensure-idea",
            "--title",
            "Export audit timeline as CSV",
            "--text-file",
            desc1_path.to_str().unwrap(),
        ],
        &base_env,
    );
    assert_eq!(ensured1["action"], "reused");
    assert_eq!(ensured1["idea_id"], blank.id);

    let feas_dir = ws.join("specs/feasibility");
    fs::create_dir_all(&feas_dir).unwrap();
    let report1_rel = "specs/feasibility/2026-11-21-export-audit-timeline-as-csv.md";
    fs::write(
        ws.join(report1_rel),
        "# Feasibility: Export audit timeline as CSV\n\
         <!-- feasibility: {\"verdict\": \"feasible-with-caveats\", \"effort\": \"M\", \"recommended\": \"Streamed CSV export\", \"unknowns\": 1} -->\n\n\
         **Verdict**: Feasible with caveats\n",
    )
    .unwrap();

    let verdict1 = run_py_script(&idea_py, &ws, &["verdict", report1_rel], &base_env);
    assert_eq!(verdict1["verdict"], "feasible-with-caveats");
    let fmap1 = run_py_script(
        &idea_py,
        &ws,
        &[
            "factory-map",
            "--verdict",
            "feasible-with-caveats",
            "--effort",
            "M",
            "--unknowns",
            "1",
        ],
        &base_env,
    );
    assert_eq!(fmap1["factory_verdict"], "feasible-with-risks");
    assert_eq!(fmap1["factory_risk"], "medium");

    run_py_script(
        &factory_py,
        &ws,
        &[
            "--idea",
            &blank.id,
            "feasibility",
            "--file",
            report1_rel,
            "--verdict",
            "feasible-with-risks",
            "--risk",
            "medium",
        ],
        &base_env,
    );

    let gate1 = run_py_script(
        &idea_py,
        &ws,
        &[
            "gate",
            "--verdict",
            "feasible-with-caveats",
            "--effort",
            "M",
            "--auto",
        ],
        &base_env,
    );
    assert_eq!(gate1["decision"], "proceed");
    run_py_script(
        &factory_py,
        &ws,
        &[
            "--idea",
            &blank.id,
            "assume",
            "--text",
            "CSV export is bounded to 90 days of audit entries.",
        ],
        &base_env,
    );

    run_py_script(
        &factory_py,
        &ws,
        &["--idea", &blank.id, "select-direction"],
        &base_env,
    );
    let plan1_path = ws.join("plan1.md");
    fs::write(
        &plan1_path,
        "## Scope\nAdd CSV export for audit timeline.\n\n\
         ## Acceptance criteria\n- CLI exports valid RFC 4180 CSV.\n\n\
         ## Risks\nLarge logs bounded to 90 days.\n\n\
         ## Estimates\nEffort M (2 days).\n\n\
         ## Workflow recommendation\ndefault-delivery\n",
    )
    .unwrap();
    run_py_script(
        &factory_py,
        &ws,
        &[
            "--idea",
            &blank.id,
            "plan",
            "--file",
            plan1_path.to_str().unwrap(),
            "--workflow",
            "default-delivery",
        ],
        &base_env,
    );

    let res1 = run_py_script(
        &idea_py,
        &ws,
        &[
            "result",
            "--status",
            "planned",
            "--idea",
            &blank.id,
            "--report",
            report1_rel,
            "--verdict",
            "feasible-with-caveats",
            "--effort",
            "M",
        ],
        &base_env,
    );
    assert_eq!(res1["idea_result"]["status"], "planned");
    assert_eq!(res1["idea_result"]["mode"], "factory");
    assert_eq!(res1["idea_result"]["idea"], blank.id);

    let stored1 = store.get_idea(&blank.id).unwrap().unwrap();
    assert_eq!(stored1.state, IdeaState::Planning);
    assert_eq!(
        stored1.feasibility_verdict,
        Some(FeasibilityVerdict::FeasibleWithRisks)
    );
    assert_eq!(stored1.risk, Some(Risk::Medium));
    assert_eq!(
        stored1.assumptions,
        vec!["CSV export is bounded to 90 days of audit entries.".to_string()]
    );
    assert!(stored1.plan.as_ref().unwrap().missing_sections.is_empty());

    let desc2_path = ws.join("desc2.md");
    fs::write(&desc2_path, "Add webhook retry backoff configuration.").unwrap();
    let ensured2 = run_py_script(
        &factory_py,
        &ws,
        &[
            "ensure-idea",
            "--title",
            "Webhook retry backoff",
            "--text-file",
            desc2_path.to_str().unwrap(),
        ],
        &base_env,
    );
    assert_eq!(ensured2["action"], "created");
    let second_id = ensured2["idea_id"].as_str().unwrap().to_string();
    assert_ne!(second_id, blank.id);

    let report2_rel = "specs/feasibility/2026-11-21-webhook-retry-backoff.md";
    fs::write(
        ws.join(report2_rel),
        "# Feasibility: Webhook retry backoff\n\
         <!-- feasibility: {\"verdict\": \"feasible\", \"effort\": \"S\", \"recommended\": \"Exponential backoff\", \"unknowns\": 0} -->\n",
    )
    .unwrap();
    run_py_script(
        &factory_py,
        &ws,
        &[
            "--idea",
            &second_id,
            "feasibility",
            "--file",
            report2_rel,
            "--verdict",
            "feasible",
            "--risk",
            "low",
        ],
        &base_env,
    );
    run_py_script(
        &factory_py,
        &ws,
        &["--idea", &second_id, "select-direction"],
        &base_env,
    );
    run_py_script(
        &factory_py,
        &ws,
        &[
            "--idea",
            &second_id,
            "plan",
            "--file",
            plan1_path.to_str().unwrap(),
            "--workflow",
            "default-delivery",
        ],
        &base_env,
    );

    let stored2 = store.get_idea(&second_id).unwrap().unwrap();
    assert_eq!(stored2.state, IdeaState::Planning);
    assert_eq!(
        stored2.orchestrator,
        Some(OrchestratorRef {
            cli: "claude".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            conversation: None,
        })
    );

    let orch_env = |k: &str| match k {
        "TANDEM_ROLE" => Some("orchestrator".to_string()),
        _ => None,
    };
    let orch_caller = Caller::cli(true, &orch_env, &user_pane);
    let denied = tandem::factory::approval::mark_ready_with_options(
        &store,
        &ws,
        &blank.id,
        &blank.id,
        "orchestrator",
        tandem::factory::approval::MarkReadyOptions::default(),
        &orch_caller,
    );
    assert!(
        matches!(
            denied,
            Err(tandem::factory::approval::ApprovalError::AgentCaller)
        ),
        "Orchestrator caller must be blocked from Gate 2 approval"
    );

    for id in [&blank.id, &second_id] {
        let ready = tandem::factory::approval::mark_ready_with_options(
            &store,
            &ws,
            id,
            id,
            "alice",
            tandem::factory::approval::MarkReadyOptions {
                accept_estimate: true,
                ..Default::default()
            },
            &user_caller,
        )
        .expect("User Gate 2 approval must succeed for /idea-planned Factory idea");
        assert!(ready.snapshot.verified);
        assert!(ready.run.is_some(), "Gate 2 must queue a Factory run");
        assert_eq!(ready.idea.state, IdeaState::Queued);
    }

    let _ = fs::remove_dir_all(&ws);
}
