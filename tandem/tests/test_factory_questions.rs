use std::fs;
use std::path::PathBuf;

use clap::Parser;
use tandem::cli::Cli;
use tandem::coordination::schema::Query;
use tandem::coordination::store::CoordStore;
use tandem::factory::commands::{FactoryCommand, FactoryQuestionCommand};
use tandem::factory::model::{Actor, ActorKind};
use tandem::factory::questions::{OPEN_QUESTION_LIMIT, for_idea};
use tandem::factory::store::FactoryStore;
use tandem::snapshot::collect_with_factory;

fn make_query(id: &str, task: &str, status: &str, blocking_scope: &str) -> Query {
    Query {
        id: id.to_string(),
        status: status.to_string(),
        asked_by: "orchestrator-1".to_string(),
        task: task.to_string(),
        blocking_scope: blocking_scope.to_string(),
        created: 100.0,
        question: format!("Question {id}?"),
        options: Vec::new(),
        answer: String::new(),
        answered_via: String::new(),
    }
}

#[test]
fn test_for_idea_counts_open_and_blocking() {
    let queries = vec![
        make_query("q-1", "idea:i-7k2m9q4d", "open", "ready-for-development"),
        make_query("q-2", "idea:i-7k2m9q4d", "open", ""),
        make_query(
            "q-3",
            "idea:i-7k2m9q4d",
            "answered",
            "ready-for-development",
        ),
        make_query("q-4", "idea:i-7k2m9q4d", "withdrawn", "all"),
        make_query("q-5", "idea:i-other001", "open", "all"),
        make_query("q-6", "p-014/t-003", "open", "task"),
    ];

    let summary = for_idea(&queries, "i-7k2m9q4d");
    assert_eq!(summary.open, 2);
    assert_eq!(summary.blocking, 1);
    assert_eq!(summary.items.len(), 4);
    assert!(summary.items[0].blocking);
    assert!(!summary.items[1].blocking);
    assert!(!summary.items[2].blocking);
}

#[test]
fn test_open_question_ceiling() {
    assert_eq!(OPEN_QUESTION_LIMIT, 5);
    let mut queries = Vec::new();
    for i in 0..5 {
        queries.push(make_query(
            &format!("q-{i}"),
            "idea:i-7k2m9q4d",
            "open",
            "task",
        ));
    }
    let summary = for_idea(&queries, "i-7k2m9q4d");
    assert_eq!(summary.open, 5);
    assert!(summary.at_limit());
}

#[test]
fn test_snapshot_populates_blockers_and_needs_you() {
    let ws = PathBuf::from(format!("/tmp/tandem-test-fq-{}", std::process::id()));
    let _ = fs::remove_dir_all(&ws);
    fs::create_dir_all(&ws).unwrap();

    let coord = CoordStore::new(ws.join(".agent-workspace/coordination"));
    let fstore = FactoryStore::new(&ws);
    let actor = Actor {
        kind: ActorKind::User,
        id: "tester".to_string(),
    };
    let idea1 = fstore
        .create_idea("Idea 1", "d1", "tester", &actor)
        .unwrap();
    let idea2 = fstore
        .create_idea("Idea 2", "d2", "tester", &actor)
        .unwrap();

    // Write two queries for idea1 (1 blocking, 1 non-blocking) and 0 for idea2
    let qdir = ws.join(".agent-workspace/coordination/queries");
    fs::create_dir_all(&qdir).unwrap();
    fs::write(
        qdir.join("q-aaa11111.md"),
        format!(
            "---\nid: \"q-aaa11111\"\nstatus: \"open\"\nasked_by: \"orchestrator-1\"\ntask: \"idea:{}\"\nblocking_scope: \"gate\"\ncreated: 1.0\nquestion: \"Q1?\"\n---\n",
            idea1.id
        ),
    )
    .unwrap();
    fs::write(
        qdir.join("q-bbb22222.md"),
        format!(
            "---\nid: \"q-bbb22222\"\nstatus: \"open\"\nasked_by: \"orchestrator-1\"\ntask: \"idea:{}\"\nblocking_scope: \"\"\ncreated: 2.0\nquestion: \"Q2?\"\n---\n",
            idea1.id
        ),
    )
    .unwrap();

    let snap = collect_with_factory(&ws, &coord, std::time::Duration::from_secs(1), true);
    let factory = snap.factory.as_ref().unwrap();
    assert_eq!(factory.needs_you, 1);
    let c1 = factory.ideas.iter().find(|c| c.id == idea1.id).unwrap();
    let c2 = factory.ideas.iter().find(|c| c.id == idea2.id).unwrap();
    assert_eq!(c1.blockers, 1);
    assert_eq!(c2.blockers, 0);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_factory_question_check_cli_parse() {
    let cli =
        Cli::try_parse_from(["tandem", "factory", "question", "check", "i-7k2m9q4d"]).unwrap();
    match cli.command {
        Some(tandem::cli::Command::Factory(args)) => match args.command {
            FactoryCommand::Question(q) => match q.command {
                FactoryQuestionCommand::Check(c) => {
                    assert_eq!(c.id, "i-7k2m9q4d");
                }
            },
            other => panic!("expected Question, got {other:?}"),
        },
        other => panic!("expected Factory, got {other:?}"),
    }
}
