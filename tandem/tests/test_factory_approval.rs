use std::fs;
use std::path::PathBuf;

use clap::Parser;
use tandem::cli::Cli;
use tandem::factory::approval::{
    Caller, MarkReadyOptions, Surface, mark_ready, mark_ready_with_options, require_user,
    revoke_ready, verify_snapshot,
};
use tandem::factory::artifacts::put as put_artifact;
use tandem::factory::commands::{FactoryCommand, FactoryIdeaCommand};
use tandem::factory::machine::Command;
use tandem::factory::model::{Actor, ActorKind, FeasibilityVerdict, IdeaState, Risk};
use tandem::factory::store::FactoryStore;

fn temp_ws(name: &str) -> PathBuf {
    let dir = PathBuf::from(format!(
        "/tmp/tandem-test-appr-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn user_actor() -> Actor {
    Actor {
        kind: ActorKind::User,
        id: "alice".to_string(),
    }
}

fn orch_actor() -> Actor {
    Actor {
        kind: ActorKind::Orchestrator,
        id: "orchestrator-1".to_string(),
    }
}

fn no_env(_k: &str) -> Option<String> {
    None
}

fn no_pane(_pane: &str) -> Option<String> {
    None
}

fn cli_tty_caller() -> Caller<'static> {
    Caller::cli(true, &no_env, &no_pane)
}

#[test]
fn test_caller_check_rejects_each_agent_env_var() {
    assert_eq!(require_user(&cli_tty_caller()).unwrap(), Surface::Cli);

    for var in ["TANDEM_ROLE", "TANDEM_SESSION_ID", "TANDEM_FACTORY_IDEA"] {
        let env = |k: &str| {
            if k == var {
                Some("val".to_string())
            } else {
                None
            }
        };
        let caller = Caller::cli(true, &env, &no_pane);
        let err = require_user(&caller).unwrap_err();
        assert_eq!(err.exit_code(), 3, "expected exit 3 for {var}");
        assert!(
            err.to_string()
                .contains("only the user can mark an idea ready for development")
        );
    }
}

#[test]
fn test_require_user_cli_refuses_non_terminal() {
    let caller = Caller::cli(false, &no_env, &no_pane);
    let err = require_user(&caller).unwrap_err();
    assert_eq!(err.exit_code(), 3);
    assert!(
        err.to_string().contains("this action needs a terminal"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_require_user_refuses_agent_tmux_pane_even_with_clean_env() {
    let env = |k: &str| {
        if k == "TMUX_PANE" {
            Some("%42".to_string())
        } else {
            None
        }
    };
    for role in ["planner", "implementer", "orchestrator", "reviewer"] {
        let pane_lookup = |pane: &str| {
            assert_eq!(pane, "%42");
            Some(role.to_string())
        };
        let caller = Caller::cli(true, &env, &pane_lookup);
        let err = require_user(&caller).unwrap_err();
        assert_eq!(err.exit_code(), 3, "expected exit 3 for role {role}");
    }

    let empty_pane = |_: &str| None;
    assert!(require_user(&Caller::cli(true, &env, &empty_pane)).is_ok());
}

#[test]
fn test_mark_ready_gate_freeze_verify_and_revoke() {
    let ws = temp_ws("gate2");
    let store = FactoryStore::new(&ws);
    let user = user_actor();
    let orch = orch_actor();

    let idea = store
        .create_idea("Export PDF", "desc", "alice", &user)
        .unwrap();

    // Wrong state (Inbox) -> refused
    assert!(mark_ready(&store, &idea.id, &idea.id, "alice", &cli_tty_caller()).is_err());

    // Move to Ideation -> Planning with feasibility
    let f = ws.join("f.md");
    fs::write(&f, "# Feasible\n").unwrap();
    let fart = put_artifact(&store, &idea.id, "feasibility", &f).unwrap();
    store
        .execute(
            &idea.id,
            Command::Describe {
                title: "Export PDF".to_string(),
                text: "desc".to_string(),
            },
            &orch,
        )
        .unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: fart,
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

    // Missing plan -> refused
    assert!(mark_ready(&store, &idea.id, &idea.id, "alice", &cli_tty_caller()).is_err());

    // Plan with missing sections -> refused
    let p_draft = ws.join("p_draft.md");
    fs::write(&p_draft, "## Scope\nOnly scope.\n").unwrap();
    let part_draft = put_artifact(&store, &idea.id, "plan", &p_draft).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".to_string(),
                artifact: part_draft,
                verdict: None,
                risk: None,
                workflow: None,
                missing_sections: vec!["Estimates".to_string()],
            },
            &orch,
        )
        .unwrap();
    assert!(mark_ready(&store, &idea.id, &idea.id, "alice", &cli_tty_caller()).is_err());

    // Complete plan, but with open blocking question -> refused
    let p_full = ws.join("p_full.md");
    fs::write(
        &p_full,
        "## Scope\n## Acceptance criteria\n## Risks\n## Estimates\n## Workflow recommendation\n",
    )
    .unwrap();
    let part_full = put_artifact(&store, &idea.id, "plan", &p_full).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".to_string(),
                artifact: part_full,
                verdict: None,
                risk: None,
                workflow: Some("feature-dev".to_string()),
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();

    let qdir = ws.join(".agent-workspace/coordination/queries");
    fs::create_dir_all(&qdir).unwrap();
    let qpath = qdir.join("q-1111aaaa.md");
    fs::write(
        &qpath,
        format!(
            "---\nid: \"q-1111aaaa\"\nstatus: \"open\"\nasked_by: \"orchestrator-1\"\ntask: \"idea:{}\"\nblocking_scope: \"gate\"\ncreated: 1.0\nquestion: \"Which format?\"\n---\n",
            idea.id
        ),
    )
    .unwrap();
    assert!(mark_ready(&store, &idea.id, &idea.id, "alice", &cli_tty_caller()).is_err());

    // Answer question -> confirm mismatch refused, matching confirm succeeds
    fs::write(
        &qpath,
        format!(
            "---\nid: \"q-1111aaaa\"\nstatus: \"answered\"\nasked_by: \"orchestrator-1\"\ntask: \"idea:{}\"\nblocking_scope: \"gate\"\ncreated: 1.0\nquestion: \"Which format?\"\nanswer: \"PDF\"\n---\n",
            idea.id
        ),
    )
    .unwrap();
    assert!(mark_ready(&store, &idea.id, "i-wrong000", "alice", &cli_tty_caller()).is_err());

    let tui_caller = Caller::tui(&no_env, &no_pane);
    let ready = mark_ready(&store, &idea.id, &idea.id, "alice", &tui_caller).unwrap();
    assert_eq!(ready.idea.state, IdeaState::ReadyForDevelopment);
    assert_eq!(ready.idea.ready_via.as_deref(), Some("tui"));
    assert!(ready.snapshot.verified);
    let snap_path = store
        .root
        .join("ideas")
        .join(&idea.id)
        .join("artifacts")
        .join("plan-snapshot.md");
    assert!(snap_path.exists());

    // Verify snapshot intact
    let v_ok = verify_snapshot(&store, &ready.idea).unwrap();
    assert!(v_ok.verified);

    // Tamper with snapshot -> verification detects it
    fs::write(&snap_path, "# tampered\n").unwrap();
    let v_bad = verify_snapshot(&store, &ready.idea).unwrap();
    assert!(!v_bad.verified);
    assert_eq!(
        v_bad.error.as_deref(),
        Some("plan snapshot was modified after approval")
    );

    // Revoke returns to Planning
    let revoked = revoke_ready(&store, &idea.id, "alice", &cli_tty_caller()).unwrap();
    assert_eq!(revoked.state, IdeaState::Planning);
    assert_eq!(revoked.ready_via, None);
    assert!(snap_path.exists());

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_approval_cli_parse() {
    assert!(
        Cli::try_parse_from([
            "tandem",
            "factory",
            "idea",
            "ready",
            "i-7k2m9q4d",
            "--confirm",
            "i-7k2m9q4d",
            "--via",
            "tui",
        ])
        .is_err(),
        "--via must be removed from `factory idea ready`"
    );

    assert!(
        Cli::try_parse_from([
            "tandem",
            "factory",
            "run",
            "approve",
            "r-7k2m9q4d",
            "--action",
            "production_deploy",
            "--confirm",
            "r-7k2m9q4d",
            "--via",
            "web",
        ])
        .is_err(),
        "--via must be removed from `factory run approve`"
    );

    let ready_cli = Cli::try_parse_from([
        "tandem",
        "factory",
        "idea",
        "ready",
        "i-7k2m9q4d",
        "--confirm",
        "i-7k2m9q4d",
        "--json",
    ])
    .unwrap();
    match ready_cli.command {
        Some(tandem::cli::Command::Factory(args)) => match args.command {
            FactoryCommand::Idea(i) => match i.command {
                FactoryIdeaCommand::Ready(r) => {
                    assert_eq!(r.id, "i-7k2m9q4d");
                    assert_eq!(r.confirm, "i-7k2m9q4d");
                    assert!(r.json);
                }
                other => panic!("expected Ready, got {other:?}"),
            },
            other => panic!("expected Idea, got {other:?}"),
        },
        other => panic!("expected Factory, got {other:?}"),
    }

    let rev_cli = Cli::try_parse_from([
        "tandem",
        "factory",
        "idea",
        "revoke-ready",
        "i-7k2m9q4d",
        "--json",
    ])
    .unwrap();
    assert!(matches!(
        rev_cli.command,
        Some(tandem::cli::Command::Factory(args))
            if matches!(args.command, FactoryCommand::Idea(ref i) if matches!(i.command, FactoryIdeaCommand::RevokeReady(_)))
    ));
}

#[test]
fn test_mark_ready_with_workflow_and_deploy_creates_run_and_refuses_invalid() {
    use tandem::config::Config;
    use tandem::factory::commands::run_with_caller;
    use tandem::factory::model::FactorySettings;
    use tandem::factory::run::list_runs;
    use tandem::factory::settings::save as save_settings;

    let tmp = std::env::temp_dir().join(format!(
        "tandem-test-approval-wf-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    let store = FactoryStore::new(&tmp);
    store.ensure().unwrap();
    let settings = FactorySettings {
        approval_grace_secs: 0,
        ..Default::default()
    };
    save_settings(&store, &settings).unwrap();
    let user = Actor {
        kind: ActorKind::User,
        id: "alice".to_string(),
    };
    let orch = Actor {
        kind: ActorKind::Orchestrator,
        id: "orch-1".to_string(),
    };
    let idea = store
        .create_idea("Export", "Export CSV", "alice", &user)
        .unwrap();
    store
        .execute(&idea.id, Command::StartDiscovery, &orch)
        .unwrap();
    let feas_src = tmp.join("feas.md");
    fs::write(&feas_src, "# Feasible\n").unwrap();
    let feas_ref =
        tandem::factory::artifacts::put(&store, &idea.id, "feasibility", &feas_src).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: feas_ref,
                verdict: None,
                risk: Some(Risk::Low),
                workflow: None,
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();
    store
        .execute(&idea.id, Command::SelectDirection, &orch)
        .unwrap();
    let plan_src = tmp.join("plan.md");
    fs::write(
        &plan_src,
        "# Plan\n\n## Goal\nExport.\n\n## Acceptance Criteria\n- Works\n\n## Files\n- a.rs\n\n## Tests\n- cargo test\n",
    )
    .unwrap();
    let plan_ref = tandem::factory::artifacts::put(&store, &idea.id, "plan", &plan_src).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".to_string(),
                artifact: plan_ref,
                verdict: None,
                risk: None,
                workflow: Some("missing-recommended".to_string()),
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();

    let cfg = Config {
        factory: true,
        state_dir: tmp.join("state"),
        ..Default::default()
    };

    // 1. Explicit invalid workflow is refused without changing idea state
    let cli_bad = Cli::try_parse_from([
        "tandem",
        "factory",
        "idea",
        "ready",
        &idea.id,
        "--confirm",
        &idea.id,
        "--workflow",
        "nonexistent-wf",
        "--json",
    ])
    .unwrap();
    let Some(tandem::cli::Command::Factory(f_bad)) = cli_bad.command else {
        panic!("expected factory");
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let caller = cli_tty_caller();
    let code = run_with_caller(&f_bad, &tmp, &cfg, &caller, &mut stdout, &mut stderr);
    assert_ne!(code, 0);
    assert_eq!(
        store.get_idea(&idea.id).unwrap().unwrap().state,
        IdeaState::Planning
    );

    // 2. Omitted --workflow falls back from missing recommended workflow to default-delivery and creates run with --deploy
    let cli_ok = Cli::try_parse_from([
        "tandem",
        "factory",
        "idea",
        "ready",
        &idea.id,
        "--confirm",
        &idea.id,
        "--deploy",
        "--json",
    ])
    .unwrap();
    let Some(tandem::cli::Command::Factory(f_ok)) = cli_ok.command else {
        panic!("expected factory");
    };
    stdout.clear();
    stderr.clear();
    let code = run_with_caller(&f_ok, &tmp, &cfg, &caller, &mut stdout, &mut stderr);
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));
    let v: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(v["idea"]["state"], "queued");
    let runs = list_runs(&store, Some(&idea.id)).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].workflow, "default-delivery");
    assert!(runs[0].deploy_requested);

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn test_cli_binary_refuses_non_tty_across_user_only_actions() {
    use std::process::{Command as StdCommand, Stdio};

    let tmp = temp_ws("non-tty");
    let store = FactoryStore::new(&tmp);
    store.ensure().unwrap();
    let bin = env!("CARGO_BIN_EXE_tandem");

    let cases: Vec<Vec<&str>> = vec![
        vec![
            "factory",
            "idea",
            "ready",
            "i-1111aaaa",
            "--confirm",
            "i-1111aaaa",
        ],
        vec!["factory", "idea", "revoke-ready", "i-1111aaaa"],
        vec![
            "factory",
            "login",
            "acknowledge-risk",
            "claude",
            "--method",
            "subscription",
            "--confirm",
            "claude",
        ],
        vec![
            "factory",
            "checks",
            "set",
            "--target",
            "workspace",
            "--test",
            "[[\"cargo\",\"test\"]]",
            "--confirm",
            "workspace",
        ],
        vec![
            "factory",
            "run",
            "budget",
            "r-1111aaaa",
            "--max-cost",
            "5.00",
        ],
        vec![
            "factory",
            "run",
            "approve",
            "r-1111aaaa",
            "--action",
            "production_deploy",
            "--confirm",
            "r-1111aaaa",
        ],
        vec!["factory", "settings", "set", "approval_grace_secs", "10"],
        vec![
            "factory",
            "permissions",
            "allow",
            "implementer",
            "repo_read",
        ],
    ];

    for args in cases {
        let out = StdCommand::new(bin)
            .args(&args)
            .current_dir(&tmp)
            .stdin(Stdio::null())
            .env("TANDEM_WORKSPACE", &tmp)
            .env("TANDEM_FACTORY", "1")
            .env_remove("TANDEM_ROLE")
            .env_remove("TANDEM_SESSION_ID")
            .env_remove("TANDEM_FACTORY_IDEA")
            .env_remove("TMUX_PANE")
            .output()
            .unwrap();
        assert_ne!(out.status.code(), Some(0), "expected refusal for {args:?}");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            combined.contains("this action needs a terminal"),
            "expected terminal refusal for {args:?}, got: {combined}"
        );
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn test_grace_period_holds_runner_and_undo_leaves_no_run() {
    use std::collections::HashMap;
    use tandem::factory::executor::{Executor, StageJob, StageOutcome};
    use tandem::factory::model::FactorySettings;
    use tandem::factory::run::list_runs;
    use tandem::factory::runner::drive_workspace_step;
    use tandem::factory::settings::save as save_settings;

    struct NoopExec;
    impl Executor for NoopExec {
        fn ready_clis(&self) -> Vec<tandem::factory::executor::ReadyCli> {
            vec![tandem::factory::executor::ReadyCli::new(
                "claude",
                Some(tandem::factory::broker::terms::AuthType::ApiKey),
            )]
        }

        fn run(&mut self, _job: &StageJob) -> StageOutcome {
            StageOutcome::Succeeded
        }
    }

    let ws = temp_ws("grace");
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();
    let settings = FactorySettings {
        approval_grace_secs: 30,
        ..Default::default()
    };
    save_settings(&store, &settings).unwrap();

    let user = user_actor();
    let orch = orch_actor();
    let idea = store
        .create_idea("Grace Test", "desc", "alice", &user)
        .unwrap();
    let f = ws.join("f.md");
    fs::write(&f, "# Feasible\n").unwrap();
    let fart = put_artifact(&store, &idea.id, "feasibility", &f).unwrap();
    store
        .execute(
            &idea.id,
            Command::Describe {
                title: "Grace Test".to_string(),
                text: "desc".to_string(),
            },
            &orch,
        )
        .unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: fart,
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
    let p = ws.join("p.md");
    fs::write(
        &p,
        "## Scope\n## Acceptance criteria\n## Risks\n## Estimates\n## Workflow recommendation\n",
    )
    .unwrap();
    let part = put_artifact(&store, &idea.id, "plan", &p).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".to_string(),
                artifact: part,
                verdict: None,
                risk: None,
                workflow: Some("default-delivery".to_string()),
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();

    let web_caller = Caller::web(&no_env);
    let ready = mark_ready_with_options(
        &store,
        &ws,
        &idea.id,
        &idea.id,
        "alice",
        MarkReadyOptions::default(),
        &web_caller,
    )
    .unwrap();
    assert_eq!(ready.idea.state, IdeaState::ReadyForDevelopment);
    assert_eq!(ready.idea.ready_via.as_deref(), Some("web"));
    assert!(ready.run.is_none(), "grace period must defer run creation");
    assert!(list_runs(&store, Some(&idea.id)).unwrap().is_empty());

    let mut exec = NoopExec;
    drive_workspace_step(&store, &ws, &mut exec).unwrap();
    assert!(
        list_runs(&store, Some(&idea.id)).unwrap().is_empty(),
        "runner must wait during grace period"
    );

    // Undo inside grace period leaves no run and returns to Planning
    let revoked = revoke_ready(&store, &idea.id, "alice", &web_caller).unwrap();
    assert_eq!(revoked.state, IdeaState::Planning);
    assert!(list_runs(&store, Some(&idea.id)).unwrap().is_empty());

    // Now mark ready with grace=0 -> runner starts immediately
    let zero_settings = FactorySettings {
        approval_grace_secs: 0,
        ..Default::default()
    };
    save_settings(&store, &zero_settings).unwrap();
    let ready_now = mark_ready_with_options(
        &store,
        &ws,
        &idea.id,
        &idea.id,
        "alice",
        MarkReadyOptions::default(),
        &cli_tty_caller(),
    )
    .unwrap();
    assert_eq!(ready_now.idea.ready_via.as_deref(), Some("cli"));
    assert!(ready_now.run.is_some());
    let _ = HashMap::<String, String>::new();
    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_ready_grace_banner_format_and_undo() {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use tandem::status::banners::{BannerAction, ReadyGraceBanner};

    let banner = ReadyGraceBanner {
        idea_id: "i-7k2m9q4d".to_string(),
        via: "cli".to_string(),
        remaining_secs: 24,
        undo_shortcut: "Ctrl+Opt+u".to_string(),
    };
    let area = Rect::new(0, 0, 100, 1);
    let mut buf = Buffer::empty(area);
    banner.render(area, &mut buf);
    let rendered: String = (0..100).map(|x| buf[(x, 0)].symbol().to_string()).collect();
    assert!(
        rendered.contains(
            "Idea i-7k2m9q4d was marked ready via cli · run starts in 24 s · Ctrl+Opt+u undo"
        ),
        "rendered: {rendered}"
    );
    assert_eq!(
        banner.click_at(10, 0, area),
        BannerAction::UndoReady("i-7k2m9q4d".to_string())
    );

    let narrow = Rect::new(0, 0, 50, 1);
    let mut narrow_buf = Buffer::empty(narrow);
    banner.render(narrow, &mut narrow_buf);
    let narrow_rendered: String = (0..50)
        .map(|x| narrow_buf[(x, 0)].symbol().to_string())
        .collect();
    assert!(narrow_rendered.contains("24 s"));
    assert!(narrow_rendered.contains("undo"));
}
