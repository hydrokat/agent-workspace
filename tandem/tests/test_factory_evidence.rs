use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use tandem::config::Config;
use tandem::factory::approval::mark_ready;
use tandem::factory::artifacts;
use tandem::factory::broker::checks::CheckOutcome;
use tandem::factory::checks::CheckKind;
use tandem::factory::commands::{
    FactoryArgs, FactoryCommand, FactoryRunArgs, FactoryRunCommand, FactoryRunEvidenceArgs,
    run_with_io,
};
use tandem::factory::events::read_all;
use tandem::factory::evidence::{
    AddEvidenceInput, EvidenceContent, add, bundle_summary, list, missing, read_item,
    record_committed_diff_stat, record_create_tests_exception, record_review_findings,
    record_test_stage_evidence,
};
use tandem::factory::machine::{Command, EventData};
use tandem::factory::model::{Actor, ActorKind, Risk, StageState};
use tandem::factory::redact::{RedactContext, redact};
use tandem::factory::run::{CreateRunOptions, create_run, get_run};
use tandem::factory::stages::test::{TestStageReport, format_failures_summary};
use tandem::factory::store::FactoryStore;
use tandem::factory::workflow::load_default_workflow;
use tandem::factory::workflow::schema::EvidenceKind;
use tandem::factory::workflow::validate::validate;

fn temp_ws(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-evidence-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn enable_factory_mode(ws: &Path) {
    fs::create_dir_all(ws.join(".agent-workspace")).unwrap();
    fs::write(ws.join(".agent-workspace").join("factory-mode"), "1\n").unwrap();
}

fn user_actor() -> Actor {
    Actor {
        kind: ActorKind::User,
        id: "user".to_string(),
    }
}

fn orch_actor() -> Actor {
    Actor {
        kind: ActorKind::Orchestrator,
        id: "orch-1".to_string(),
    }
}

fn setup_run(ws: &Path) -> (FactoryStore, String, String) {
    enable_factory_mode(ws);
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();

    let idea = store
        .create_idea(
            "Evidence test",
            "Need evidence bundle",
            "user",
            &user_actor(),
        )
        .unwrap();
    store
        .execute(&idea.id, Command::StartDiscovery, &orch_actor())
        .unwrap();

    let feas_src = ws.join("feas.md");
    fs::write(&feas_src, "# Feasibility\n\nFeasible.\n").unwrap();
    let feas_ref = artifacts::put(&store, &idea.id, "feasibility", &feas_src).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: feas_ref,
                verdict: Some(tandem::factory::model::FeasibilityVerdict::Feasible),
                risk: Some(Risk::Low),
                workflow: None,
                missing_sections: Vec::new(),
            },
            &orch_actor(),
        )
        .unwrap();

    let plan_src = ws.join("plan.md");
    fs::write(
        &plan_src,
        "## Scope\nA\n## Acceptance criteria\nB\n## Risks\nC\n## Estimates\nD\n## Workflow recommendation\ndefault-delivery\n",
    )
    .unwrap();
    let plan_ref = artifacts::put(&store, &idea.id, "plan", &plan_src).unwrap();
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
            &orch_actor(),
        )
        .unwrap();
    store
        .execute(&idea.id, Command::SelectDirection, &user_actor())
        .unwrap();
    mark_ready(&store, &idea.id, &idea.id, "cli", "user", &|_| None).unwrap();

    let run = create_run(
        &store,
        ws,
        &idea.id,
        CreateRunOptions::default(),
        &user_actor(),
    )
    .unwrap();
    (store, idea.id, run.id)
}

#[test]
fn add_creates_hashed_append_only_items_and_emits_event() {
    let ws = temp_ws("append");
    let (store, idea_id, run_id) = setup_run(&ws);

    let rctx = RedactContext::new();
    let item1 = add(
        &store,
        AddEvidenceInput {
            idea_id: &idea_id,
            run_id: &run_id,
            kind: EvidenceKind::TestReport,
            stage: "test",
            attempt: 1,
            extension: "json",
            content: EvidenceContent::Text(redact(r#"{"passed":false,"checks":[]}"#, &rctx)),
            summary: redact("tests: 1 failed", &rctx),
        },
        &orch_actor(),
    )
    .unwrap();

    let item2 = add(
        &store,
        AddEvidenceInput {
            idea_id: &idea_id,
            run_id: &run_id,
            kind: EvidenceKind::TestReport,
            stage: "test",
            attempt: 2,
            extension: "json",
            content: EvidenceContent::Text(redact(r#"{"passed":true,"checks":[]}"#, &rctx)),
            summary: redact("tests: 12 passed", &rctx),
        },
        &orch_actor(),
    )
    .unwrap();

    assert_ne!(item1.id, item2.id);
    assert_ne!(item1.path, item2.path);
    assert_ne!(item1.sha256, item2.sha256);
    assert_eq!(item1.sha256.len(), 64);
    assert!(item1.path.starts_with(&format!("runs/{run_id}/evidence/")));
    assert!(store.root.join(&item1.path).is_file());
    assert!(store.root.join(&item2.path).is_file());

    let listed = list(&store, &run_id).unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0], item1);
    assert_eq!(listed[1], item2);

    let events = read_all(&store.root.join("events.jsonl")).unwrap().events;
    let added_events: Vec<_> = events
        .iter()
        .filter(|e| e.r#type == "evidence.added")
        .collect();
    assert_eq!(added_events.len(), 2);
    let ev0 = added_events[0].to_event_data().unwrap();
    match ev0 {
        EventData::EvidenceAdded { run, item } => {
            assert_eq!(run, run_id);
            assert_eq!(item.sha256, item1.sha256);
        }
        other => panic!("unexpected event: {other:?}"),
    }

    let persisted_run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(persisted_run.evidence.len(), 2);
}

#[test]
fn test_stage_producer_references_raw_logs_without_copying() {
    let ws = temp_ws("test-producer");
    let (store, idea_id, run_id) = setup_run(&ws);

    let checks_dir = store
        .root
        .join("runs")
        .join(&run_id)
        .join("artifacts")
        .join("checks");
    fs::create_dir_all(&checks_dir).unwrap();
    let test_log = checks_dir.join("test-attempt1-0.log");
    let lint_log = checks_dir.join("lint-attempt1-0.log");
    let secret_raw_line = "RAW_LOG_SENTINEL_DO_NOT_COPY_INTO_EVIDENCE";
    fs::write(&test_log, format!("running 48 tests\n{secret_raw_line}\n")).unwrap();
    fs::write(&lint_log, format!("clippy ok\n{secret_raw_line}\n")).unwrap();

    let outcomes = vec![
        CheckOutcome {
            kind: CheckKind::Test,
            argv: vec!["cargo".into(), "test".into()],
            exit_code: Some(0),
            duration_ms: 420,
            log_path: test_log.clone(),
            timed_out: false,
            log_cap_exceeded: false,
            summary: None,
            error: None,
        },
        CheckOutcome {
            kind: CheckKind::Lint,
            argv: vec!["cargo".into(), "clippy".into()],
            exit_code: Some(0),
            duration_ms: 180,
            log_path: lint_log.clone(),
            timed_out: false,
            log_cap_exceeded: false,
            summary: None,
            error: None,
        },
    ];
    let report = TestStageReport {
        failures_summary: format_failures_summary(&outcomes),
        outcomes,
        all_passed: true,
    };

    let added =
        record_test_stage_evidence(&store, &idea_id, &run_id, 1, &report, &orch_actor()).unwrap();
    assert_eq!(added.len(), 2);
    assert_eq!(added[0].kind, EvidenceKind::TestReport);
    assert_eq!(added[1].kind, EvidenceKind::LintReport);

    let test_body = read_item(&store, &run_id, &added[0].id).unwrap();
    assert!(test_body.contains("test-attempt1-0.log"));
    assert!(test_body.contains("420"));
    assert!(!test_body.contains(secret_raw_line));

    let lint_body = read_item(&store, &run_id, &added[1].id).unwrap();
    assert!(lint_body.contains("lint-attempt1-0.log"));
    assert!(lint_body.contains("180"));
    assert!(!lint_body.contains(secret_raw_line));
}

#[test]
fn review_exception_and_diff_stat_producers() {
    let ws = temp_ws("other-producers");
    let (store, idea_id, run_id) = setup_run(&ws);

    let r1 = record_review_findings(
        &store,
        &idea_id,
        &run_id,
        1,
        br#"{"round":1,"findings":[{"severity":"major","msg":"fix nil"}]}"#,
        "review round 1: 1 finding".to_string(),
        &orch_actor(),
    )
    .unwrap();
    let r2 = record_review_findings(
        &store,
        &idea_id,
        &run_id,
        1,
        br#"{"round":2,"findings":[]}"#,
        "review round 2: clean".to_string(),
        &orch_actor(),
    )
    .unwrap();
    assert_eq!(r1.kind, EvidenceKind::ReviewFindings);
    assert_eq!(r2.kind, EvidenceKind::ReviewFindings);
    assert_ne!(r1.id, r2.id);

    let exc = record_create_tests_exception(
        &store,
        &idea_id,
        &run_id,
        1,
        "refactor only, existing suite covers behaviour",
        &orch_actor(),
    )
    .unwrap();
    assert_eq!(exc.kind, EvidenceKind::Exception);
    let exc_body = read_item(&store, &run_id, &exc.id).unwrap();
    assert!(exc_body.contains("no_behaviour_change"));

    let repo = ws.join("repo");
    fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let out = StdCommand::new("git")
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test"]);
    fs::write(repo.join("lib.rs"), "pub fn a() {}\n").unwrap();
    git(&["add", "lib.rs"]);
    git(&["commit", "-m", "init"]);
    fs::write(repo.join("lib.rs"), "pub fn a() {}\npub fn b() {}\n").unwrap();
    git(&["add", "lib.rs"]);
    git(&["commit", "-m", "feat: add b"]);

    let diff = record_committed_diff_stat(
        &store,
        &idea_id,
        &run_id,
        "develop",
        1,
        &repo,
        &orch_actor(),
    )
    .unwrap();
    assert_eq!(diff.kind, EvidenceKind::DiffStat);
    let diff_body = read_item(&store, &run_id, &diff.id).unwrap();
    assert!(diff_body.contains("lib.rs"));
}

#[test]
fn default_workflow_evidence_requirements_and_completeness() {
    let ws = temp_ws("completeness");
    let (store, idea_id, run_id) = setup_run(&ws);

    let (raw, _) = load_default_workflow(&ws).unwrap();
    let settings = store.settings().unwrap();
    let wf = validate(&raw, &settings).unwrap();

    let test_stage = wf.stages.iter().find(|s| s.name == "test").unwrap();
    assert_eq!(
        test_stage.evidence,
        vec![EvidenceKind::TestReport, EvidenceKind::LintReport]
    );
    let review_stage = wf.stages.iter().find(|s| s.name == "review").unwrap();
    assert_eq!(review_stage.evidence, vec![EvidenceKind::ReviewFindings]);

    let mut run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(
        missing(&run, &wf),
        vec![
            EvidenceKind::TestReport,
            EvidenceKind::LintReport,
            EvidenceKind::ReviewFindings,
        ]
    );

    let rctx = RedactContext::new();
    add(
        &store,
        AddEvidenceInput {
            idea_id: &idea_id,
            run_id: &run_id,
            kind: EvidenceKind::TestReport,
            stage: "test",
            attempt: 1,
            extension: "json",
            content: EvidenceContent::Text(redact("{}", &rctx)),
            summary: redact("tests: 48 passed", &rctx),
        },
        &orch_actor(),
    )
    .unwrap();
    add(
        &store,
        AddEvidenceInput {
            idea_id: &idea_id,
            run_id: &run_id,
            kind: EvidenceKind::LintReport,
            stage: "test",
            attempt: 1,
            extension: "json",
            content: EvidenceContent::Text(redact("{}", &rctx)),
            summary: redact("lint: clean", &rctx),
        },
        &orch_actor(),
    )
    .unwrap();
    run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(missing(&run, &wf), vec![EvidenceKind::ReviewFindings]);

    add(
        &store,
        AddEvidenceInput {
            idea_id: &idea_id,
            run_id: &run_id,
            kind: EvidenceKind::ReviewFindings,
            stage: "review",
            attempt: 1,
            extension: "json",
            content: EvidenceContent::Text(redact("{}", &rctx)),
            summary: redact("review: 2 findings fixed", &rctx),
        },
        &orch_actor(),
    )
    .unwrap();
    run = get_run(&store, &run_id).unwrap().unwrap();
    assert!(missing(&run, &wf).is_empty());

    if let Some(st) = run.stages.iter_mut().find(|s| s.name == "review") {
        st.state = StageState::Skipped;
    }
    run.evidence
        .retain(|e| e.kind != EvidenceKind::ReviewFindings);
    assert!(missing(&run, &wf).is_empty());
}

#[test]
fn cli_run_evidence_lists_summarizes_and_shows_sanitized_item() {
    let ws = temp_ws("cli");
    let (store, idea_id, run_id) = setup_run(&ws);

    let rctx = RedactContext::new();
    let item = add(
        &store,
        AddEvidenceInput {
            idea_id: &idea_id,
            run_id: &run_id,
            kind: EvidenceKind::TestReport,
            stage: "test",
            attempt: 1,
            extension: "json",
            content: EvidenceContent::Text(redact(
                r#"{"note":"hello \u001b[31mred\u001b[0m"}"#,
                &rctx,
            )),
            summary: redact("tests 48 passed", &rctx),
        },
        &orch_actor(),
    )
    .unwrap();
    // Write raw ANSI escape directly to verify --show sanitizes terminal escapes.
    fs::write(
        store.root.join(&item.path),
        "line1\x1b[31mRED\x1b[0m\nline2\n",
    )
    .unwrap();

    let cfg = Config::default();
    let list_args = FactoryArgs {
        command: FactoryCommand::Run(FactoryRunArgs {
            command: FactoryRunCommand::Evidence(FactoryRunEvidenceArgs {
                run: run_id.clone(),
                show: None,
                json: true,
            }),
        }),
    };
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut stdin = std::io::empty();
    let code = run_with_io(&list_args, &ws, &cfg, &mut stdin, &mut out, &mut err);
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    let val: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(val["run"], run_id);
    assert_eq!(val["complete"], false);
    assert_eq!(val["items"].as_array().unwrap().len(), 1);
    assert!(!val["summary"].as_str().unwrap().is_empty());
    assert!(bundle_summary(std::slice::from_ref(&item)).contains("tests 48 passed"));

    let show_args = FactoryArgs {
        command: FactoryCommand::Run(FactoryRunArgs {
            command: FactoryRunCommand::Evidence(FactoryRunEvidenceArgs {
                run: run_id,
                show: Some(item.id),
                json: false,
            }),
        }),
    };
    let mut show_out = Vec::new();
    let mut show_err = Vec::new();
    let mut show_in = std::io::empty();
    let show_code = run_with_io(
        &show_args,
        &ws,
        &cfg,
        &mut show_in,
        &mut show_out,
        &mut show_err,
    );
    assert_eq!(
        show_code,
        0,
        "stderr: {}",
        String::from_utf8_lossy(&show_err)
    );
    let shown = String::from_utf8(show_out).unwrap();
    assert!(!shown.contains('\x1b'));
    assert!(shown.contains("RED"));
}
