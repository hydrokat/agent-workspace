use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tandem::coordination::schema::Query;
use tandem::factory::artifacts;
use tandem::factory::events;
use tandem::factory::machine::Command;
use tandem::factory::model::{Actor, ActorKind, FeasibilityVerdict, IdeaState, Risk};
use tandem::factory::store::FactoryStore;
use tandem::snapshot::{FactorySummary, IdeaCard, StatusSnapshot};
use tandem::status::factory_detail::{DetailKeyOutcome, DetailSubView, FactoryDetail};
use tandem::status::factory_tab::{FactoryTab, FactoryView};

fn temp_workspace(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tandem-test-factory-detail-{name}-{}",
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

fn full_plan_md() -> &'static str {
    "# Plan\n\n## Scope\nExport CSV.\n\n## Acceptance criteria\n- Works cleanly.\n\n## Risks\nLow risk.\n\n## Estimates\n1 day.\n\n## Workflow recommendation\nspec-implement\n"
}

fn setup_planning_idea(ws: &Path, complete_plan: bool) -> (FactoryStore, String) {
    let store = FactoryStore::new(ws);
    store.ensure().unwrap();
    let user = user_actor();
    let orch = orch_actor();

    let idea = store
        .create_idea("Export team activity as CSV", "CSV export", &user.id, &user)
        .unwrap();
    store
        .execute(
            &idea.id,
            Command::BindOrchestrator {
                cli: "claude".to_string(),
                model: "opus".to_string(),
                conversation: Some("conv-123".to_string()),
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
    fs::write(&feas_src, "# Feasibility\n\nFeasible with minor risks.\n").unwrap();
    let feas_ref = artifacts::put(&store, &idea.id, "feasibility", &feas_src).unwrap();
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "feasibility".to_string(),
                artifact: feas_ref,
                verdict: Some(FeasibilityVerdict::FeasibleWithRisks),
                risk: Some(Risk::Medium),
                workflow: None,
                missing_sections: Vec::new(),
            },
            &orch,
        )
        .unwrap();

    store
        .execute(&idea.id, Command::SelectDirection, &user)
        .unwrap();
    store
        .execute(
            &idea.id,
            Command::AddAssumption {
                text: "UTF-8 CSV output\x1b[31m\x07".to_string(),
            },
            &orch,
        )
        .unwrap();

    let plan_src = ws.join("plan.md");
    let plan_body = if complete_plan {
        full_plan_md()
    } else {
        "# Plan\n\n## Scope\nPartial only.\n"
    };
    fs::write(&plan_src, plan_body).unwrap();
    let plan_ref = artifacts::put(&store, &idea.id, "plan", &plan_src).unwrap();
    let missing = artifacts::missing_plan_sections(plan_body);
    store
        .execute(
            &idea.id,
            Command::StoreArtifact {
                kind: "plan".to_string(),
                artifact: plan_ref,
                verdict: None,
                risk: None,
                workflow: Some("spec-implement".to_string()),
                missing_sections: missing,
            },
            &orch,
        )
        .unwrap();

    (store, idea.id)
}

#[test]
fn test_factory_detail_sections_and_reader() {
    let ws = temp_workspace("sections");
    let (store, id) = setup_planning_idea(&ws, true);
    let queries = vec![Query {
        id: "q-001".to_string(),
        asked_by: "orchestrator".to_string(),
        task: format!("idea:{id}"),
        question: "Include archived users?".to_string(),
        blocking_scope: "planning".to_string(),
        status: "open".to_string(),
        created: 1_700_000_000.0,
        options: Vec::new(),
        answer: String::new(),
        answered_via: String::new(),
    }];

    let mut detail = FactoryDetail::load(&ws, &id, &queries, true);
    detail.platform = "linux".to_string();

    let wide = Rect::new(0, 0, 125, 16);
    let mut buf = Buffer::empty(wide);
    detail.render(1_700_000_100.0, wide, &mut buf);
    let out = buffer_text(&buf, wide);

    assert!(out.contains(&id));
    assert!(out.contains("Export team activity as CSV"));
    assert!(out.contains("Planning"));
    assert!(out.contains("risk: medium"));
    assert!(out.contains("Orchestrator: claude / opus"));
    assert!(out.contains("conversation saved"));
    assert!(out.contains("pane: open"));
    assert!(out.contains("Feasibility  v1  feasible with risks"));
    assert!(out.contains("Plan  v1  5/5 sections"));
    assert!(out.contains("Assumptions 1"));
    assert!(out.contains("1 open, 1 blocking → Queries tab"));
    assert!(out.contains("Timeline"));
    assert!(out.contains("Alt+y mark ready"));

    // Open feasibility reader with 'f' (also verify untrusted escape sequences are sanitized)
    fs::write(
        store
            .root
            .join("ideas")
            .join(&id)
            .join("artifacts/feasibility-v1.md"),
        "# Feasibility\n\nFeasible with minor risks.\x1b[31m!\x1b[0m\n",
    )
    .unwrap();
    detail.handle_key(&KeyEvent::new(KeyCode::Char('f'), KeyModifiers::empty()));
    assert!(matches!(detail.sub_view, DetailSubView::Reader { .. }));
    let mut buf = Buffer::empty(wide);
    detail.render(1_700_000_100.0, wide, &mut buf);
    let reader_out = buffer_text(&buf, wide);
    assert!(reader_out.contains("Feasible with minor risks.!"));
    assert!(!reader_out.contains("\x1b[31m"));

    // Esc returns to detail main
    detail.handle_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
    assert!(matches!(detail.sub_view, DetailSubView::Main));

    // Open assumptions reader with 'a' and verify sanitization
    detail.handle_key(&KeyEvent::new(KeyCode::Char('a'), KeyModifiers::empty()));
    let mut buf = Buffer::empty(wide);
    detail.render(1_700_000_100.0, wide, &mut buf);
    let assump_out = buffer_text(&buf, wide);
    assert!(assump_out.contains("UTF-8 CSV output"));
    assert!(!assump_out.contains("\x1b[31m"));
    assert!(!assump_out.contains('\x07'));

    let _ = store;
    let _ = fs::remove_dir_all(ws);
}

#[test]
fn test_factory_detail_gate_reasons_and_mark_ready_via_tui() {
    let ws = temp_workspace("gate-and-ready");
    let (store, id) = setup_planning_idea(&ws, false);

    // 1. Incomplete plan blocks mark ready with a one-line reason
    let mut detail = FactoryDetail::load(&ws, &id, &[], false);
    detail.handle_key(&KeyEvent::new(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert!(matches!(detail.sub_view, DetailSubView::Main));
    assert!(detail.notice.contains("missing required sections"));

    // 2. Store complete plan, but add a blocking question -> gate blocks with reason
    let plan_src = ws.join("plan-full.md");
    fs::write(&plan_src, full_plan_md()).unwrap();
    let plan_ref = artifacts::put(&store, &id, "plan", &plan_src).unwrap();
    store
        .execute(
            &id,
            Command::StoreArtifact {
                kind: "plan".to_string(),
                artifact: plan_ref,
                verdict: None,
                risk: None,
                workflow: Some("spec-implement".to_string()),
                missing_sections: Vec::new(),
            },
            &orch_actor(),
        )
        .unwrap();

    let blocking_q = vec![Query {
        id: "q-002".to_string(),
        asked_by: "orchestrator".to_string(),
        task: format!("idea:{id}"),
        question: "Which timezone?".to_string(),
        blocking_scope: "planning".to_string(),
        status: "open".to_string(),
        created: 1_700_000_000.0,
        options: Vec::new(),
        answer: String::new(),
        answered_via: String::new(),
    }];
    let mut detail = FactoryDetail::load(&ws, &id, &blocking_q, false);
    detail.handle_key(&KeyEvent::new(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert!(matches!(detail.sub_view, DetailSubView::Main));
    assert!(detail.notice.contains("1 blocking question"));

    // 3. No blocking questions -> opens ConfirmReady modal
    let mut detail = FactoryDetail::load(&ws, &id, &[], false);
    detail.owner = "argie".to_string();
    detail.handle_key(&KeyEvent::new(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert!(matches!(
        detail.sub_view,
        DetailSubView::ConfirmReady { .. }
    ));

    // Mismatch id keeps modal open with error
    for ch in "wrong-id".chars() {
        detail.handle_key(&KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty()));
    }
    detail.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
    assert!(matches!(
        detail.sub_view,
        DetailSubView::ConfirmReady { .. }
    ));

    // Clear and type exact idea id
    for _ in 0..8 {
        detail.handle_key(&KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
    }
    for ch in id.chars() {
        detail.handle_key(&KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty()));
    }
    let outcome = detail.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
    assert!(matches!(outcome, DetailKeyOutcome::Action(_)));
    assert!(matches!(detail.sub_view, DetailSubView::Main));

    // Verify in-process approval recorded via = "tui" and run queued
    let updated = store.get_idea(&id).unwrap().unwrap();
    assert_eq!(updated.state, IdeaState::Queued);
    let evs = events::read_all(&store.root.join("events.jsonl"))
        .unwrap()
        .events;
    let frozen = evs
        .iter()
        .find(|e| e.idea == id && e.r#type == "idea.plan_snapshot_frozen")
        .expect("snapshot frozen event must exist");
    assert_eq!(frozen.data["via"], "tui");
    assert_eq!(frozen.data["approved_by"], "argie");

    // Withdraw queued run to return to ReadyForDevelopment
    detail.handle_key(&KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty()));
    assert!(matches!(
        detail.sub_view,
        DetailSubView::ConfirmWithdraw { .. }
    ));
    detail.handle_key(&KeyEvent::new(KeyCode::Char('y'), KeyModifiers::empty()));
    assert_eq!(
        store.get_idea(&id).unwrap().unwrap().state,
        IdeaState::ReadyForDevelopment
    );

    // Ready banner renders approval + snapshot verified
    let wide = Rect::new(0, 0, 125, 16);
    let mut buf = Buffer::empty(wide);
    detail.render(1_700_000_100.0, wide, &mut buf);
    let out = buffer_text(&buf, wide);
    assert!(out.contains("Ready for development"));
    assert!(out.contains("approved by argie via tui"));
    assert!(out.contains("snapshot verified"));

    // 4. Revoke in ReadyForDevelopment
    detail.handle_key(&KeyEvent::new(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert!(matches!(detail.sub_view, DetailSubView::ConfirmRevoke));
    detail.handle_key(&KeyEvent::new(KeyCode::Char('y'), KeyModifiers::empty()));
    assert!(matches!(detail.sub_view, DetailSubView::Main));
    let revoked = store.get_idea(&id).unwrap().unwrap();
    assert_eq!(revoked.state, IdeaState::Planning);

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn test_factory_detail_responsive_narrow_short_and_snapshot_reload() {
    let ws = temp_workspace("responsive");
    let (store, id) = setup_planning_idea(&ws, true);
    let idea = store.get_idea(&id).unwrap().unwrap();

    let mut tab = FactoryTab::new(
        Some(ws.clone()),
        Default::default(),
        std::sync::Arc::new(|_| Ok(Vec::new())),
    );
    let snap = StatusSnapshot {
        now: 1_700_000_100.0,
        factory: Some(FactorySummary {
            enabled: true,
            ideas: vec![IdeaCard {
                id: idea.id.clone(),
                title: idea.title.clone(),
                state: idea.state,
                owner: idea.owner.clone(),
                risk: idea.risk,
                blockers: 0,
                updated: idea.updated,
                run: None,
            }],
            needs_you: 0,
            ..FactorySummary::default()
        }),
        ..StatusSnapshot::default()
    };
    tab.sync_snapshot(&snap, false);

    // Enter opens detail
    tab.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()), &snap);
    assert_eq!(tab.view, FactoryView::Detail);

    // Narrow (< 80)
    let narrow = Rect::new(0, 0, 60, 16);
    let mut buf = Buffer::empty(narrow);
    tab.render(&snap, narrow, &mut buf);
    let out_narrow = buffer_text(&buf, narrow);
    assert!(out_narrow.contains(&id));
    assert!(out_narrow.contains("Esc back"));

    // Short (6 rows): timeline shrinks first
    let short = Rect::new(0, 0, 125, 6);
    let mut buf = Buffer::empty(short);
    tab.render(&snap, short, &mut buf);
    let out_short = buffer_text(&buf, short);
    assert!(out_short.contains(&id));
    assert!(out_short.contains("Orchestrator:"));

    // Snapshot change reloads detail without timer
    store
        .execute(
            &id,
            Command::AddAssumption {
                text: "Reloaded assumption".to_string(),
            },
            &orch_actor(),
        )
        .unwrap();
    let updated_idea = store.get_idea(&id).unwrap().unwrap();
    let mut snap2 = snap.clone();
    snap2.factory.as_mut().unwrap().ideas[0].updated = updated_idea.updated + 1.0;
    tab.sync_snapshot(&snap2, false);
    assert_eq!(
        tab.detail
            .as_ref()
            .and_then(|d| d.idea.as_ref())
            .map(|i| i.assumptions.len()),
        Some(2)
    );

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn test_detail_confirm_ready_workflow_selection_run_section_and_withdraw() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use tandem::factory::model::IdeaState;
    use tandem::status::factory_detail::{DetailSubView, FactoryDetail};

    let ws = std::env::temp_dir().join(format!(
        "tandem-test-detail-wf-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&ws);
    fs::create_dir_all(&ws).unwrap();
    let (store, id) = setup_planning_idea(&ws, true);

    // Add a second valid workflow and one invalid workflow
    fs::create_dir_all(ws.join("workflows")).unwrap();
    let default_yaml = tandem::factory::workflow::DEFAULT_WORKFLOW_YAML;
    fs::write(ws.join("workflows/default-delivery.yaml"), default_yaml).unwrap();
    fs::write(
        ws.join("workflows/fast-fix.yaml"),
        default_yaml.replace("name: default-delivery", "name: fast-fix"),
    )
    .unwrap();
    fs::write(ws.join("workflows/broken-wf.yaml"), "name: broken-wf\n").unwrap();

    let mut detail = FactoryDetail::load(&ws, &id, &[], false);

    // Trigger mark ready via Ctrl+Alt+y -> enters ConfirmReady
    let ready_key = KeyEvent::new(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    detail.handle_key(&ready_key);
    assert!(matches!(
        detail.sub_view,
        DetailSubView::ConfirmReady { .. }
    ));

    // Render ConfirmReady: shows workflow line, invalid workflow diagnostic, and deployment toggle
    let wide = Rect::new(0, 0, 120, 20);
    let mut buf = Buffer::empty(wide);
    detail.render(1000.0, wide, &mut buf);
    let text = buffer_text(&buf, wide);
    assert!(text.contains("Workflow"));
    assert!(text.contains("w change"));
    assert!(text.contains("broken-wf"));
    assert!(text.contains("invalid:"));
    assert!(text.contains("Deployment"));
    assert!(text.contains("d toggle"));

    // Cycle workflow with 'w' and toggle deploy with 'd'
    detail.handle_key(&KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE));
    detail.handle_key(&KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
    if let DetailSubView::ConfirmReady {
        ref workflows,
        selected_wf,
        deploy_requested,
        ..
    } = detail.sub_view
    {
        assert!(workflows[selected_wf].valid);
        assert!(deploy_requested);
    } else {
        panic!("expected ConfirmReady");
    }

    // Type idea id and press Enter -> creates run and moves to Queued
    for ch in id.chars() {
        detail.handle_key(&KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    detail.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(detail.sub_view, DetailSubView::Main));
    assert_eq!(
        store.get_idea(&id).unwrap().unwrap().state,
        IdeaState::Queued
    );

    // Wide render shows Run section with horizontal strip and legend
    let mut buf = Buffer::empty(wide);
    detail.render(1000.0, wide, &mut buf);
    let out_wide = buffer_text(&buf, wide);
    assert!(out_wide.contains("Run r-"));
    assert!(out_wide.contains("Queued"));
    assert!(out_wide.contains("x withdraw"));
    assert!(!out_wide.contains("Execution arrives with the execution phase."));
    assert!(out_wide.contains("○ pending"));

    // Narrow render wraps stage strip to one stage per line with state as word
    let narrow = Rect::new(0, 0, 60, 25);
    let mut buf = Buffer::empty(narrow);
    detail.render(1000.0, narrow, &mut buf);
    let out_narrow = buffer_text(&buf, narrow);
    assert!(out_narrow.contains("plan: pending"));

    // Press 'x' then 'y' to withdraw queued run
    detail.handle_key(&KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    assert!(matches!(
        detail.sub_view,
        DetailSubView::ConfirmWithdraw { .. }
    ));
    detail.handle_key(&KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    assert_eq!(
        store.get_idea(&id).unwrap().unwrap().state,
        IdeaState::ReadyForDevelopment
    );

    let _ = fs::remove_dir_all(ws);
}
