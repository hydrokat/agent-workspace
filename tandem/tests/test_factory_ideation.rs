use std::fs;
use std::path::PathBuf;

use clap::Parser;
use tandem::cli::Cli;
use tandem::factory::artifacts::{
    missing_plan_sections, put as put_artifact, read as read_artifact,
};
use tandem::factory::commands::{FactoryArtifactCommand, FactoryCommand, FactoryIdeaCommand};
use tandem::factory::machine::{Command, TransitionError};
use tandem::factory::model::{Actor, ActorKind, FeasibilityVerdict, IdeaState, Risk};
use tandem::factory::store::FactoryStore;

fn temp_ws(name: &str) -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/tandem-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn user_actor() -> Actor {
    Actor {
        kind: ActorKind::User,
        id: "tester".to_string(),
    }
}

fn orch_actor() -> Actor {
    Actor {
        kind: ActorKind::Orchestrator,
        id: "orchestrator-1".to_string(),
    }
}

#[test]
fn test_artifacts_put_read_and_validation() {
    let ws = temp_ws("artifacts");
    let store = FactoryStore::new(&ws);
    let idea = store
        .create_idea("Idea 1", "desc", "tester", &user_actor())
        .unwrap();

    let src_dir = ws.join("inputs");
    fs::create_dir_all(&src_dir).unwrap();

    let f1 = src_dir.join("f1.md");
    fs::write(&f1, "# Feasibility\nAll good.\n").unwrap();
    let r1 = put_artifact(&store, &idea.id, "feasibility", &f1).unwrap();
    assert_eq!(r1.version, 1);
    assert_eq!(r1.kind, "feasibility");
    assert_eq!(r1.bytes, 24);
    assert_eq!(r1.sha256.len(), 64);

    let p1 = src_dir.join("p1.md");
    fs::write(&p1, "## Scope\nA\n").unwrap();
    let rp1 = put_artifact(&store, &idea.id, "plan", &p1).unwrap();
    assert_eq!(rp1.version, 1);

    let p2 = src_dir.join("p2.md");
    fs::write(&p2, "## Scope\nB\n").unwrap();
    let rp2 = put_artifact(&store, &idea.id, "plan", &p2).unwrap();
    assert_eq!(rp2.version, 2);

    // Earlier version is preserved and readable
    assert_eq!(
        read_artifact(&store, &idea.id, "plan", Some(1)).unwrap(),
        "## Scope\nA\n"
    );
    assert_eq!(
        read_artifact(&store, &idea.id, "plan", None).unwrap(),
        "## Scope\nB\n"
    );

    // Reject directory
    assert!(put_artifact(&store, &idea.id, "plan", &src_dir).is_err());

    // Reject oversized (> 256 KiB)
    let big = src_dir.join("big.md");
    fs::write(&big, vec![b'a'; 256 * 1024 + 1]).unwrap();
    assert!(put_artifact(&store, &idea.id, "plan", &big).is_err());

    // Reject invalid UTF-8
    let bad_utf8 = src_dir.join("bad.md");
    fs::write(&bad_utf8, [0xff, 0xfe, 0xfd]).unwrap();
    assert!(put_artifact(&store, &idea.id, "plan", &bad_utf8).is_err());

    // Reject control chars (NUL)
    let ctrl = src_dir.join("ctrl.md");
    fs::write(&ctrl, "hello\x00world").unwrap();
    assert!(put_artifact(&store, &idea.id, "plan", &ctrl).is_err());

    // Reject symlink
    #[cfg(unix)]
    {
        let link = src_dir.join("link.md");
        std::os::unix::fs::symlink(&f1, &link).unwrap();
        assert!(put_artifact(&store, &idea.id, "plan", &link).is_err());
    }

    // No plan-v3.md was created by any rejected put
    assert!(
        !store
            .root
            .join("ideas")
            .join(&idea.id)
            .join("artifacts")
            .join("plan-v3.md")
            .exists()
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_missing_plan_sections_ignores_code_blocks() {
    let md = "\
# Plan
```markdown
## Scope
## Estimates
```
## Scope
Real scope.
## Acceptance criteria
Tests pass.
## Risks
Low.
";
    let missing = missing_plan_sections(md);
    assert_eq!(missing, vec!["Estimates", "Workflow recommendation"]);

    let full = "\
## Scope
## Acceptance criteria
## Risks
## Estimates
## Workflow recommendation
";
    assert!(missing_plan_sections(full).is_empty());
}

#[test]
fn test_gate1_feasibility_assumptions_and_ready_block() {
    let ws = temp_ws("gate1");
    let store = FactoryStore::new(&ws);
    let actor = user_actor();
    let orch = orch_actor();

    let idea = store.create_idea("Draft", "raw", "tester", &actor).unwrap();
    let idea = store
        .execute(
            &idea.id,
            Command::Describe {
                title: "Export PDF".to_string(),
                text: "Detailed spec".to_string(),
            },
            &orch,
        )
        .unwrap();
    assert_eq!(idea.state, IdeaState::Ideation);
    assert_eq!(idea.title, "Export PDF");

    // Gate 1 fails without feasibility
    let err = store
        .execute(&idea.id, Command::SelectDirection, &actor)
        .unwrap_err();
    assert!(matches!(
        err,
        tandem::factory::FactoryError::Transition(TransitionError::GateNotMet { .. })
    ));

    // Store infeasible study -> Gate 1 still blocks
    let f_infeas = ws.join("f_infeas.md");
    fs::write(&f_infeas, "# No\n").unwrap();
    let art1 = put_artifact(&store, &idea.id, "feasibility", &f_infeas).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: art1,
                verdict: Some(FeasibilityVerdict::Infeasible),
                risk: Some(Risk::High),
                workflow: None,
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();
    assert!(
        store
            .execute(&idea.id, Command::SelectDirection, &actor)
            .is_err()
    );

    // Store feasible study -> Gate 1 passes
    let f_feas = ws.join("f_feas.md");
    fs::write(&f_feas, "# Yes\n").unwrap();
    let art2 = put_artifact(&store, &idea.id, "feasibility", &f_feas).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: art2,
                verdict: Some(FeasibilityVerdict::Feasible),
                risk: Some(Risk::Low),
                workflow: None,
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();
    let planned = store
        .execute(&idea.id, Command::SelectDirection, &actor)
        .unwrap();
    assert_eq!(planned.state, IdeaState::Planning);

    // Assumptions bounded to 30
    for i in 0..30 {
        store
            .execute(
                &idea.id,
                Command::AddAssumption {
                    text: format!("Assumption {i}"),
                },
                &orch,
            )
            .unwrap();
    }
    assert!(
        store
            .execute(
                &idea.id,
                Command::AddAssumption {
                    text: "Assumption 31".to_string(),
                },
                &orch,
            )
            .is_err()
    );

    // Store plan and mark ready -> cannot replace plan while ReadyForDevelopment
    let p1 = ws.join("p1.md");
    fs::write(
        &p1,
        "## Scope\n## Acceptance criteria\n## Risks\n## Estimates\n## Workflow recommendation\n",
    )
    .unwrap();
    let part1 = put_artifact(&store, &idea.id, "plan", &p1).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".to_string(),
                artifact: part1,
                verdict: None,
                risk: None,
                workflow: Some("feature-dev".to_string()),
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();
    let ready = store.execute(&idea.id, Command::MarkReady, &actor).unwrap();
    assert_eq!(ready.state, IdeaState::ReadyForDevelopment);

    let p2 = ws.join("p2.md");
    fs::write(&p2, "## Scope\n").unwrap();
    let part2 = put_artifact(&store, &idea.id, "plan", &p2).unwrap();
    assert!(
        store
            .execute(
                &idea.id,
                Command::StoreArtifact {
                    kind: "plan".to_string(),
                    artifact: part2,
                    verdict: None,
                    risk: None,
                    workflow: None,
                    missing_sections: vec!["Estimates".to_string()],
                },
                &orch,
            )
            .is_err()
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_ideation_cli_parse() {
    let cli = Cli::try_parse_from([
        "tandem",
        "factory",
        "artifact",
        "put",
        "i-7k2m9q4d",
        "--kind",
        "feasibility",
        "--file",
        "f.md",
        "--verdict",
        "feasible",
        "--risk",
        "low",
    ])
    .unwrap();
    match cli.command {
        Some(tandem::cli::Command::Factory(args)) => match args.command {
            FactoryCommand::Artifact(a) => match a.command {
                FactoryArtifactCommand::Put(p) => {
                    assert_eq!(p.id, "i-7k2m9q4d");
                    assert_eq!(p.kind, "feasibility");
                }
                other => panic!("expected Put, got {other:?}"),
            },
            other => panic!("expected Artifact, got {other:?}"),
        },
        other => panic!("expected Factory, got {other:?}"),
    }

    let sel = Cli::try_parse_from([
        "tandem",
        "factory",
        "idea",
        "select-direction",
        "i-7k2m9q4d",
    ])
    .unwrap();
    assert!(matches!(
        sel.command,
        Some(tandem::cli::Command::Factory(args))
            if matches!(args.command, FactoryCommand::Idea(ref i) if matches!(i.command, FactoryIdeaCommand::SelectDirection(_)))
    ));
}
