use std::collections::HashMap;
use std::path::PathBuf;

use clap::Parser;
use tandem::adapters::base::{AgentSpec, session_env};
use tandem::cli::Cli;
use tandem::factory::commands::{FactoryCommand, FactoryIdeaCommand};
use tandem::factory::machine::Command as FactoryMachineCmd;
use tandem::factory::store::FactoryStore;
use tandem::sidebar::model::{SidebarRow, rows_from};
use tandem::state;
use tandem::tmux::layout;
use tandem::tmux::tabs::PaneInfo;
use tandem::wizard::flow::{self, Step, WizardError, WizardFlow};

#[test]
fn test_roles_factory_isolation() {
    assert_eq!(state::ROLES, ["planner", "implementer"]);
    assert_eq!(state::roles(false), &["planner", "implementer"]);
    assert_eq!(state::roles(true), &["orchestrator"]);

    assert_eq!(flow::ROLES, ["planner", "implementer"]);
    assert_eq!(flow::roles(false), &["planner", "implementer"]);
    assert_eq!(flow::roles(true), &["orchestrator"]);

    assert_eq!(layout::ROLES, ["planner", "implementer"]);
    assert_eq!(layout::roles(false), &["planner", "implementer"]);
    assert_eq!(layout::roles(true), &["orchestrator"]);

    let mut installed = HashMap::new();
    installed.insert("claude".to_string(), true);
    let mut normal = WizardFlow::new(installed);
    normal.choose_cli("claude").unwrap();
    assert_eq!(normal.step(), Step::Role);
    assert_eq!(
        normal.choose_role("orchestrator"),
        Err(WizardError::InvalidRole)
    );
}

#[test]
fn test_factory_wizard_skips_role_perms_cwd() {
    let mut installed = HashMap::new();
    installed.insert("claude".to_string(), true);
    installed.insert("agy".to_string(), true);
    let mut wf = WizardFlow::new_for_session(installed, true);
    assert_eq!(wf.step(), Step::Cli);
    wf.choose_cli("agy").unwrap();
    assert_eq!(wf.step(), Step::Model);
    assert_eq!(wf.draft_role(), "orchestrator");

    wf.choose_model("opus").unwrap();
    assert_eq!(wf.step(), Step::Summary);
    assert_eq!(wf.agents.len(), 1);
    assert!(!wf.agents[0].skip_permissions);
    assert_eq!(wf.agents[0].role, "orchestrator");
    assert_eq!(wf.agents[0].cli, "agy");
    assert_eq!(wf.agents[0].model, "opus");
    assert_eq!(wf.agents[0].cwd, None);
}

#[test]
fn test_agent_spec_idea_and_session_env() {
    let normal = AgentSpec::new("claude", "planner", "opus");
    let normal_json = serde_json::to_string(&normal).unwrap();
    assert!(!normal_json.contains("idea"));
    let normal_env = session_env(&normal, "/tmp/coord");
    assert!(!normal_env.contains_key("TANDEM_FACTORY"));
    assert!(!normal_env.contains_key("TANDEM_FACTORY_IDEA"));

    let mut orch = AgentSpec::new("claude", "orchestrator", "opus");
    orch.idea = Some("i-7k2m9q4d".to_string());
    let orch_json = serde_json::to_string(&orch).unwrap();
    assert!(orch_json.contains("i-7k2m9q4d"));
    let orch_env = session_env(&orch, "/tmp/coord");
    assert_eq!(
        orch_env.get("TANDEM_FACTORY").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        orch_env.get("TANDEM_FACTORY_IDEA").map(String::as_str),
        Some("i-7k2m9q4d")
    );
}

#[test]
fn test_sidebar_orchestrators_group() {
    let orch_pane = PaneInfo {
        id: "%5".to_string(),
        window: "main".to_string(),
        kind: "agent".to_string(),
        role: "orchestrator".to_string(),
        index: 1,
        label: "i-7k2m9q4d Export PDF".to_string(),
        dead: false,
        dead_status: None,
        session: "orchestrator-1".to_string(),
        spec: String::new(),
        cli_version: String::new(),
        name: String::new(),
        idea: "i-7k2m9q4d".to_string(),
    };
    let rows = rows_from(&[orch_pane], &[], &[], Some("%5"), &[]);
    assert!(matches!(
        &rows[0],
        SidebarRow::Header { title, count: 1 } if title == "ORCHESTRATORS"
    ));
    assert!(matches!(
        &rows[1],
        SidebarRow::Agent { role, label, .. } if role == "orchestrator" && label.contains("i-7k2m9q4d")
    ));
}

#[test]
fn test_factory_idea_start_and_resume_cli_parse() {
    let start_cli = Cli::try_parse_from([
        "tandem",
        "factory",
        "idea",
        "start",
        "--cli",
        "claude",
        "--model",
        "opus",
        "--title",
        "Export reports",
    ])
    .unwrap();
    match start_cli.command {
        Some(tandem::cli::Command::Factory(args)) => match args.command {
            FactoryCommand::Idea(idea_args) => match idea_args.command {
                FactoryIdeaCommand::Start(s) => {
                    assert_eq!(s.cli, "claude");
                    assert_eq!(s.model, "opus");
                    assert_eq!(s.title.as_deref(), Some("Export reports"));
                }
                other => panic!("expected Start, got {other:?}"),
            },
            other => panic!("expected Idea, got {other:?}"),
        },
        other => panic!("expected Factory, got {other:?}"),
    }

    let resume_cli =
        Cli::try_parse_from(["tandem", "factory", "idea", "resume", "i-7k2m9q4d"]).unwrap();
    match resume_cli.command {
        Some(tandem::cli::Command::Factory(args)) => match args.command {
            FactoryCommand::Idea(idea_args) => match idea_args.command {
                FactoryIdeaCommand::Resume(r) => {
                    assert_eq!(r.id, "i-7k2m9q4d");
                }
                other => panic!("expected Resume, got {other:?}"),
            },
            other => panic!("expected Idea, got {other:?}"),
        },
        other => panic!("expected Factory, got {other:?}"),
    }
}

#[test]
fn test_factory_store_cancel_on_launch_failure() {
    use tandem::factory::model::{Actor, ActorKind, IdeaState, KNOWN_PROVIDERS};
    let dir = PathBuf::from(format!(
        "/tmp/tandem-test-factory-orch-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let store = FactoryStore::new(&dir);
    assert!(KNOWN_PROVIDERS.contains(&"claude"));
    let actor = Actor {
        kind: ActorKind::User,
        id: "tester".to_string(),
    };
    let idea = store
        .create_idea("Test Idea", "desc", "tester", &actor)
        .expect("create_idea");
    let cancelled = store
        .execute(
            &idea.id,
            FactoryMachineCmd::Cancel {
                reason: "orchestrator failed to start".to_string(),
            },
            &actor,
        )
        .expect("cancel_idea");
    assert_eq!(cancelled.state, IdeaState::Cancelled);
    let _ = std::fs::remove_dir_all(&dir);
}
