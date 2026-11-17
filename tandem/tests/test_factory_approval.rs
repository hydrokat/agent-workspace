use std::fs;
use std::path::PathBuf;

use clap::Parser;
use tandem::cli::Cli;
use tandem::factory::approval::{ensure_user_caller, mark_ready, revoke_ready, verify_snapshot};
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

#[test]
fn test_caller_check_rejects_each_agent_env_var() {
    assert!(ensure_user_caller(&no_env).is_ok());

    for var in ["TANDEM_ROLE", "TANDEM_SESSION_ID", "TANDEM_FACTORY_IDEA"] {
        let err = ensure_user_caller(&|k| {
            if k == var {
                Some("val".to_string())
            } else {
                None
            }
        })
        .unwrap_err();
        assert_eq!(err.exit_code(), 3, "expected exit 3 for {var}");
        assert!(
            err.to_string()
                .contains("only the user can mark an idea ready for development")
        );
    }
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
    assert!(mark_ready(&store, &idea.id, &idea.id, "cli", "alice", &no_env).is_err());

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
    assert!(mark_ready(&store, &idea.id, &idea.id, "cli", "alice", &no_env).is_err());

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
    assert!(mark_ready(&store, &idea.id, &idea.id, "cli", "alice", &no_env).is_err());

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
    assert!(mark_ready(&store, &idea.id, &idea.id, "cli", "alice", &no_env).is_err());

    // Answer question -> confirm mismatch refused, matching confirm succeeds
    fs::write(
        &qpath,
        format!(
            "---\nid: \"q-1111aaaa\"\nstatus: \"answered\"\nasked_by: \"orchestrator-1\"\ntask: \"idea:{}\"\nblocking_scope: \"gate\"\ncreated: 1.0\nquestion: \"Which format?\"\nanswer: \"PDF\"\n---\n",
            idea.id
        ),
    )
    .unwrap();
    assert!(mark_ready(&store, &idea.id, "i-wrong000", "cli", "alice", &no_env).is_err());

    let ready = mark_ready(&store, &idea.id, &idea.id, "tui", "alice", &no_env).unwrap();
    assert_eq!(ready.idea.state, IdeaState::ReadyForDevelopment);
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
    let revoked = revoke_ready(&store, &idea.id, "alice", &no_env).unwrap();
    assert_eq!(revoked.state, IdeaState::Planning);
    assert!(snap_path.exists());

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_approval_cli_parse() {
    let ready_cli = Cli::try_parse_from([
        "tandem",
        "factory",
        "idea",
        "ready",
        "i-7k2m9q4d",
        "--confirm",
        "i-7k2m9q4d",
        "--via",
        "tui",
        "--json",
    ])
    .unwrap();
    match ready_cli.command {
        Some(tandem::cli::Command::Factory(args)) => match args.command {
            FactoryCommand::Idea(i) => match i.command {
                FactoryIdeaCommand::Ready(r) => {
                    assert_eq!(r.id, "i-7k2m9q4d");
                    assert_eq!(r.confirm, "i-7k2m9q4d");
                    assert_eq!(r.via, "tui");
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
    use tandem::factory::commands::run_with_io;
    use tandem::factory::run::list_runs;

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
    let mut stdin = std::io::empty();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = run_with_io(&f_bad, &tmp, &cfg, &mut stdin, &mut stdout, &mut stderr);
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
    let code = run_with_io(&f_ok, &tmp, &cfg, &mut stdin, &mut stdout, &mut stderr);
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));
    let v: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(v["idea"]["state"], "queued");
    let runs = list_runs(&store, Some(&idea.id)).unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].workflow, "default-delivery");
    assert!(runs[0].deploy_requested);

    let _ = fs::remove_dir_all(&tmp);
}
