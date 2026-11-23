use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tandem::coordination::store::CoordStore;
use tandem::factory::approval::mark_ready_with_workflow;
use tandem::factory::artifacts;
use tandem::factory::events::Event;
use tandem::factory::evidence::{self, AddEvidenceInput};
use tandem::factory::login::LoginProbe;
use tandem::factory::machine::{Command, EventData};
use tandem::factory::model::{Actor, ActorKind, FeasibilityVerdict, PrLink, Risk};
use tandem::factory::run::get_run;
use tandem::factory::stages::review::{
    Finding, FindingSeverity, ReviewRoundRecord, ReviewRoundsReport, ReviewVerdict,
};
use tandem::factory::store::FactoryStore;
use tandem::factory::workflow::schema::EvidenceKind;
use tandem::snapshot::collect_with_factory;
use tandem::status::factory_detail::{
    DetailAction, DetailKeyOutcome, DetailSubView, FactoryDetail,
};
use tandem::status::factory_run::{FactoryRun, RunSubView};

fn apply_event(
    store: &FactoryStore,
    idea_id: &str,
    run_id: &str,
    ev_data: EventData,
    actor: &Actor,
) {
    let ev = Event::from_data(
        2000.0,
        actor.clone(),
        idea_id.to_string(),
        Some(run_id.to_string()),
        &ev_data,
    )
    .unwrap();
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(store.root.join("events.jsonl"))
        .unwrap();
    writeln!(f, "{}", serde_json::to_string(&ev).unwrap()).unwrap();
    let _ = fs::remove_file(store.root.join("runs").join(run_id).join("run.json"));
}

fn temp_workspace(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tandem-test-evidence-view-{name}-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(path.join(".agent-workspace/coordination/queries")).unwrap();
    fs::create_dir_all(path.join("specs")).unwrap();
    path
}

fn buffer_text(buf: &Buffer, area: Rect) -> String {
    let mut lines = Vec::new();
    for y in area.y..area.y + area.height {
        let mut line = String::new();
        for x in area.x..area.x + area.width {
            line.push_str(buf[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

fn user_actor() -> Actor {
    Actor {
        kind: ActorKind::User,
        id: "argie".to_string(),
    }
}

fn orch_actor() -> Actor {
    Actor {
        kind: ActorKind::Orchestrator,
        id: "orchestrator".to_string(),
    }
}

fn sys_actor() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "runner".to_string(),
    }
}

fn full_plan_md() -> &'static str {
    "# Plan\n\n## Scope\nExport team activity as CSV.\n\n## Acceptance criteria\n- Exports CSV cleanly.\n\n## Risks\nLow risk.\n\n## Estimates\n1 day.\n\n## Workflow recommendation\ndefault-delivery\n"
}

fn setup_delivered_run(ws: &Path) -> (FactoryStore, String, String) {
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();
    let mut settings = store.settings().unwrap();
    settings.approval_grace_secs = 0;
    tandem::factory::settings::save(&store, &settings).unwrap();
    let user = user_actor();
    let orch = orch_actor();
    let sys = sys_actor();

    let idea = store
        .create_idea("Export team activity as CSV", "CSV export", &user.id, &user)
        .unwrap();
    store
        .execute(
            &idea.id,
            Command::BindOrchestrator {
                cli: "claude".to_string(),
                model: "opus".to_string(),
                conversation: Some("conv-1".to_string()),
            },
            &user,
        )
        .unwrap();
    store
        .execute(
            &idea.id,
            Command::Describe {
                title: "Export team activity as CSV".to_string(),
                text: "Allow exporting team activity.".to_string(),
            },
            &orch,
        )
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

    let plan_src = ws.join("plan.md");
    fs::write(&plan_src, full_plan_md()).unwrap();
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
            &orch,
        )
        .unwrap();

    let env_fn = |k: &str| {
        if k == "TANDEM_ROLE" {
            None
        } else {
            std::env::var(k).ok()
        }
    };
    let caller = tandem::factory::approval::Caller::tui(&env_fn, &|_| None);
    let ready = mark_ready_with_workflow(
        &store,
        ws,
        &idea.id,
        &idea.id,
        &user.id,
        Some("default-delivery"),
        false,
        &caller,
    )
    .unwrap();
    let run_id = ready.run.expect("run created").id;

    let redact_ctx = tandem::factory::redact::RedactContext::new();
    evidence::add(
        &store,
        AddEvidenceInput {
            idea_id: &idea.id,
            run_id: &run_id,
            kind: EvidenceKind::TestReport,
            stage: "test",
            attempt: 2,
            extension: "json",
            content: evidence::EvidenceContent::Text(tandem::factory::redact::redact(
                r#"{"attempt":2,"passed":true,"checks":[]}"#,
                &redact_ctx,
            )),
            summary: tandem::factory::redact::redact("tests 48 passed, 0 failed", &redact_ctx),
        },
        &sys,
    )
    .unwrap();
    evidence::add(
        &store,
        AddEvidenceInput {
            idea_id: &idea.id,
            run_id: &run_id,
            kind: EvidenceKind::LintReport,
            stage: "test",
            attempt: 2,
            extension: "json",
            content: evidence::EvidenceContent::Text(tandem::factory::redact::redact(
                r#"{"attempt":2,"passed":true,"checks":[]}"#,
                &redact_ctx,
            )),
            summary: tandem::factory::redact::redact("lint clean", &redact_ctx),
        },
        &sys,
    )
    .unwrap();
    evidence::add(
        &store,
        AddEvidenceInput {
            idea_id: &idea.id,
            run_id: &run_id,
            kind: EvidenceKind::Handoff,
            stage: "handoff",
            attempt: 1,
            extension: "md",
            content: evidence::EvidenceContent::Text(tandem::factory::redact::redact(
                "# Handoff\n\nAll 48 tests pass and PR #142 is ready.\n",
                &redact_ctx,
            )),
            summary: tandem::factory::redact::redact("handoff ready", &redact_ctx),
        },
        &sys,
    )
    .unwrap();

    let review_dir = store.run_dir(&run_id).join("artifacts/review");
    fs::create_dir_all(&review_dir).unwrap();
    let rounds = ReviewRoundsReport {
        rounds: vec![
            ReviewRoundRecord {
                round: 1,
                stated_verdict: ReviewVerdict::ChangesRequested,
                derived_verdict: ReviewVerdict::ChangesRequested,
                verdict_overridden: false,
                findings: vec![Finding {
                    id: "f-1".to_string(),
                    severity: FindingSeverity::Major,
                    file: "src/export.rs".to_string(),
                    line: 42,
                    title: "Missing CSV escaping".to_string(),
                    detail: "Quote fields".to_string(),
                    suggestion: "Use csv writer".to_string(),
                }],
                fixed_from_previous: vec![],
                remaining_from_previous: vec![],
            },
            ReviewRoundRecord {
                round: 2,
                stated_verdict: ReviewVerdict::Approve,
                derived_verdict: ReviewVerdict::Approve,
                verdict_overridden: false,
                findings: vec![],
                fixed_from_previous: vec!["f-1".to_string()],
                remaining_from_previous: vec![],
            },
        ],
    };
    fs::write(
        review_dir.join("review-rounds.json"),
        serde_json::to_vec_pretty(&rounds).unwrap(),
    )
    .unwrap();

    let pr = PrLink {
        repo: "hydrokat/agent-workspace".to_string(),
        number: 142,
        url: "https://github.com/hydrokat/agent-workspace/pull/142".to_string(),
        state: Some("open".to_string()),
        checks_summary: Some("checks passing".to_string()),
    };
    apply_event(
        &store,
        &idea.id,
        &run_id,
        EventData::PrCreated {
            run: run_id.clone(),
            pr: pr.clone(),
        },
        &sys,
    );
    apply_event(
        &store,
        &idea.id,
        &run_id,
        EventData::PrStatusRefreshed {
            run: run_id.clone(),
            pr: pr.clone(),
        },
        &sys,
    );
    apply_event(
        &store,
        &idea.id,
        &run_id,
        EventData::IdeaDelivered {
            run: run_id.clone(),
            prs: vec![pr],
        },
        &sys,
    );

    (store, idea.id, run_id)
}

#[test]
fn snapshot_run_summary_includes_evidence_rounds_and_prs() {
    let ws = temp_workspace("snapshot");
    let (_store, idea_id, run_id) = setup_delivered_run(&ws);

    let coord = CoordStore::new(ws.clone());
    let snap = collect_with_factory(&ws, &coord, Duration::from_secs(1), true);
    let factory = snap.factory.expect("factory summary");
    let card = factory
        .ideas
        .iter()
        .find(|c| c.id == idea_id)
        .expect("idea card");
    let run = card.run.as_ref().expect("run summary");

    assert_eq!(run.id, run_id);
    assert!(
        run.evidence_summary
            .as_deref()
            .unwrap_or("")
            .contains("tests 48 passed")
    );
    assert_eq!(run.evidence_complete, Some(false));
    assert_eq!(run.review_rounds.len(), 2);
    assert_eq!(run.review_rounds[0].round, 1);
    assert_eq!(run.review_rounds[0].open_findings, 1);
    assert_eq!(run.review_rounds[1].round, 2);
    assert_eq!(run.review_rounds[1].fixed_findings, 1);
    assert_eq!(run.prs.len(), 1);
    assert_eq!(run.prs[0].number, 142);
    assert_eq!(run.prs[0].state.as_deref(), Some("open"));
}

#[test]
fn factory_detail_delivered_header_osc8_handoff_and_ready_copy() {
    let ws = temp_workspace("detail");
    let (_store, idea_id, run_id) = setup_delivered_run(&ws);

    let mut detail = FactoryDetail::load(&ws, &idea_id, &[], false);
    let area = Rect::new(0, 0, 120, 24);
    let mut buf = Buffer::empty(area);
    detail.render(2000.0, area, &mut buf);
    let text = buffer_text(&buf, area);

    assert!(
        text.contains("Pull request"),
        "expected Pull request header: {text}"
    );
    assert!(text.contains("#142"), "expected PR #142: {text}");
    assert!(text.contains("open"), "expected PR state: {text}");
    assert!(
        text.contains("checks passing"),
        "expected checks summary: {text}"
    );
    assert!(text.contains("Evidence"), "expected Evidence line: {text}");
    assert!(text.contains("h handoff"), "expected handoff hint: {text}");
    assert!(
        text.contains("e evidence"),
        "expected evidence hint: {text}"
    );
    assert!(
        text.contains("r refresh PR status"),
        "expected refresh hint: {text}"
    );

    let links = detail.pr_terminal_links();
    assert_eq!(links.len(), 1);
    assert!(links[0].1.starts_with("\x1b]8;;https://github.com/"));

    let outcome_h = detail.handle_key(&KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
    assert_eq!(outcome_h, DetailKeyOutcome::Consumed);
    assert!(matches!(detail.sub_view, DetailSubView::Reader { .. }));
    let mut rbuf = Buffer::empty(area);
    detail.render(2000.0, area, &mut rbuf);
    let rtext = buffer_text(&rbuf, area);
    assert!(
        rtext.contains("All 48 tests pass"),
        "expected handoff text: {rtext}"
    );

    detail.handle_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    let outcome_e = detail.handle_key(&KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
    assert_eq!(
        outcome_e,
        DetailKeyOutcome::Action(DetailAction::OpenRunEvidence {
            run_id: run_id.clone()
        })
    );
}

#[test]
fn factory_detail_mark_ready_text_states_full_authorization() {
    let ws = temp_workspace("ready-copy");
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();
    let mut settings = store.settings().unwrap();
    settings.approval_grace_secs = 0;
    tandem::factory::settings::save(&store, &settings).unwrap();
    let user = user_actor();
    let orch = orch_actor();

    let idea = store
        .create_idea("Ready copy check", "Check ready copy", &user.id, &user)
        .unwrap();
    store
        .execute(
            &idea.id,
            Command::Describe {
                title: "Ready copy check".to_string(),
                text: "Check ready copy".to_string(),
            },
            &orch,
        )
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
    let plan_src = ws.join("plan.md");
    fs::write(&plan_src, full_plan_md()).unwrap();
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
            &orch,
        )
        .unwrap();

    let mut detail = FactoryDetail::load(&ws, &idea.id, &[], false);
    detail.handle_key(&KeyEvent::new(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert!(matches!(
        detail.sub_view,
        DetailSubView::ConfirmReady { .. }
    ));

    let area = Rect::new(0, 0, 120, 20);
    let mut buf = Buffer::empty(area);
    detail.render(2000.0, area, &mut buf);
    let text = buffer_text(&buf, area);
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");

    assert!(
        normalized.contains("spend budget"),
        "missing spend budget: {text}"
    );
    assert!(
        normalized.contains("new branch"),
        "missing new branch authorization: {text}"
    );
    assert!(
        normalized.contains("open a pull request"),
        "missing pull request authorization: {text}"
    );
}

#[test]
fn factory_run_evidence_list_reader_and_narrow_responsive() {
    let ws = temp_workspace("run-evidence");
    let (_store, idea_id, run_id) = setup_delivered_run(&ws);

    let mut run_view = FactoryRun::load(&ws, &idea_id, &run_id, "macos");
    run_view.handle_key(&KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
    assert!(matches!(run_view.sub_view, RunSubView::EvidenceList { .. }));

    let wide = Rect::new(0, 0, 120, 24);
    let mut wide_buf = Buffer::empty(wide);
    run_view.render(2000.0, wide, &mut wide_buf);
    let wide_text = buffer_text(&wide_buf, wide);
    assert!(
        wide_text.contains("tests 48 passed"),
        "wide should show summary: {wide_text}"
    );
    assert!(
        wide_text.contains("round 1"),
        "should show round 1: {wide_text}"
    );
    assert!(
        wide_text.contains("open"),
        "should mark open in words: {wide_text}"
    );
    assert!(
        wide_text.contains("fixed"),
        "should mark fixed in words: {wide_text}"
    );

    let narrow = Rect::new(0, 0, 60, 24);
    let mut narrow_buf = Buffer::empty(narrow);
    run_view.render(2000.0, narrow, &mut narrow_buf);
    let narrow_text = buffer_text(&narrow_buf, narrow);
    assert!(
        !narrow_text.contains("tests 48 passed"),
        "narrow drops summary column: {narrow_text}"
    );

    run_view.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        run_view.sub_view,
        RunSubView::EvidenceReader { .. }
    ));
}

struct ReadyClaudeProbe;

impl LoginProbe for ReadyClaudeProbe {
    fn which(&self, bin: &str) -> Option<PathBuf> {
        (bin == "claude").then(|| PathBuf::from("/usr/local/bin/claude"))
    }
    fn version(&self, _cli: &str) -> String {
        "1.0.0".to_string()
    }
    fn run(&self, argv: &[&str]) -> (i32, String, String) {
        if argv.first() == Some(&"claude") {
            (
                0,
                r#"{"loggedIn":true,"authMethod":"subscription"}"#.to_string(),
                String::new(),
            )
        } else {
            (1, String::new(), String::new())
        }
    }
}

#[test]
fn factory_run_confirm_unconfirmed_checks_requires_target_name() {
    let ws = temp_workspace("checks-confirm");
    fs::write(
        ws.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();
    let mut settings = store.settings().unwrap();
    settings.approval_grace_secs = 0;
    tandem::factory::settings::save(&store, &settings).unwrap();
    let user = user_actor();
    let orch = orch_actor();
    let sys = sys_actor();

    let idea = store
        .create_idea("Checks confirm", "Confirm checks", &user.id, &user)
        .unwrap();
    store
        .execute(
            &idea.id,
            Command::Describe {
                title: "Checks confirm".to_string(),
                text: "Confirm checks".to_string(),
            },
            &orch,
        )
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
    let plan_src = ws.join("plan.md");
    fs::write(&plan_src, full_plan_md()).unwrap();
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
            &orch,
        )
        .unwrap();
    let env_fn = |k: &str| {
        if k == "TANDEM_ROLE" {
            None
        } else {
            std::env::var(k).ok()
        }
    };
    let caller = tandem::factory::approval::Caller::tui(&env_fn, &|_| None);
    let ready = mark_ready_with_workflow(
        &store,
        &ws,
        &idea.id,
        &idea.id,
        &user.id,
        Some("default-delivery"),
        false,
        &caller,
    )
    .unwrap();
    let run_id = ready.run.expect("run").id;

    apply_event(
        &store,
        &idea.id,
        &run_id,
        EventData::RunStarted {
            run: run_id.clone(),
        },
        &sys,
    );
    apply_event(
        &store,
        &idea.id,
        &run_id,
        EventData::RunPaused {
            run: run_id.clone(),
            reason: "test and lint commands are not confirmed".to_string(),
            commands: vec!["tandem factory checks discover --target workspace".to_string()],
            cap: None,
            spent: None,
            limit: None,
        },
        &sys,
    );

    let mut run_view = FactoryRun::load(&ws, &idea.id, &run_id, "macos")
        .with_login_probe(Arc::new(ReadyClaudeProbe));
    let area = Rect::new(0, 0, 120, 20);
    let mut buf = Buffer::empty(area);
    run_view.render(2000.0, area, &mut buf);
    let text = buffer_text(&buf, area);
    assert!(
        text.contains("confirm check commands"),
        "expected confirm check commands action: {text}"
    );

    run_view.handle_key(&KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
    assert!(matches!(
        run_view.sub_view,
        RunSubView::ConfirmChecks { .. }
    ));

    let mut cbuf = Buffer::empty(area);
    run_view.render(2000.0, area, &mut cbuf);
    let ctext = buffer_text(&cbuf, area);
    assert!(
        ctext.contains("cargo test"),
        "expected discovered proposal: {ctext}"
    );

    run_view.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(
            run_view.sub_view,
            RunSubView::ConfirmChecks { error: Some(_), .. }
        ),
        "empty target name must be rejected"
    );

    for ch in "workspace".chars() {
        run_view.handle_key(&KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    run_view.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(run_view.sub_view, RunSubView::Main));

    let settings = store.settings().unwrap();
    assert!(settings.checks.contains_key("workspace"));
    let updated_run = get_run(&store, &run_id).unwrap().unwrap();
    assert_ne!(updated_run.state, tandem::factory::model::RunState::Paused);
}
