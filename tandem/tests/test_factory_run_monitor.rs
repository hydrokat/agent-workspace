use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tandem::coordination::store::CoordStore;
use tandem::factory::approval::mark_ready_with_workflow;
use tandem::factory::artifacts;
use tandem::factory::broker::context::AssembledContext;
use tandem::factory::broker::job::{JobSpec, JobState, prepare_job, set_job_state};
use tandem::factory::broker::result::{JobResult, JobResultStatus, JobUsage};
use tandem::factory::events::Event;
use tandem::factory::machine::{Command, EventData};
use tandem::factory::model::{Actor, ActorKind, FeasibilityVerdict, Risk, RunState};
use tandem::factory::run::get_run;
use tandem::factory::runner::pause_run_by_user;
use tandem::factory::store::FactoryStore;
use tandem::snapshot::collect_with_factory;
use tandem::status::factory_detail::{DetailAction, DetailKeyOutcome, FactoryDetail};
use tandem::status::factory_run::{FactoryRun, RunSubView};

fn temp_workspace(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tandem-test-run-monitor-{name}-{}",
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

fn setup_run(ws: &Path) -> (FactoryStore, String, String) {
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

    let ready = mark_ready_with_workflow(
        &store,
        ws,
        &idea.id,
        &idea.id,
        "tui",
        &user.id,
        Some("default-delivery"),
        false,
        &|k| {
            if k == "TANDEM_ROLE" {
                None
            } else {
                std::env::var(k).ok()
            }
        },
    )
    .unwrap();
    assert!(!ready.message.contains("Execution arrives"));
    let run_id = ready.run.expect("run created").id;
    (store, idea.id, run_id)
}

fn append_run_event(store: &FactoryStore, idea_id: &str, run_id: &str, ts: f64, data: EventData) {
    let ev = Event::from_data(
        ts,
        sys_actor(),
        idea_id.to_string(),
        Some(run_id.to_string()),
        &data,
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

#[allow(clippy::too_many_arguments)]
fn prepare_stage_job(
    store: &FactoryStore,
    run_id: &str,
    stage: &str,
    attempt: u32,
    provider: &str,
    model: &str,
    state: JobState,
    result: Option<JobResult>,
) -> PathBuf {
    let spec = JobSpec {
        run: run_id.to_string(),
        stage: stage.to_string(),
        attempt,
        agent: "implementer".to_string(),
        provider: provider.to_string(),
        model: model.to_string(),
        tool_grants: vec![
            "repo_read".to_string(),
            "repo_write".to_string(),
            "shell".to_string(),
        ],
        cwd: ".".to_string(),
        time_limit_secs: 600,
    };
    let ctx = AssembledContext {
        prompt: "Do the work".to_string(),
        manifest: Default::default(),
    };
    let prepared = prepare_job(store, &spec, &ctx).unwrap();
    set_job_state(&prepared.dir, state).unwrap();
    if let Some(res) = result {
        fs::write(
            &prepared.result_path,
            serde_json::to_vec_pretty(&res).unwrap(),
        )
        .unwrap();
    }
    prepared.log_path
}

fn no_agent_env(key: &str) -> Option<String> {
    if key == "TANDEM_ROLE" {
        None
    } else {
        std::env::var(key).ok()
    }
}

#[test]
fn test_snapshot_run_summary_stages_jobs_cost_elapsed_and_paused_needs_you() {
    let ws = temp_workspace("snapshot");
    let (store, idea_id, run_id) = setup_run(&ws);
    let coord = CoordStore::new(ws.clone());

    append_run_event(
        &store,
        &idea_id,
        &run_id,
        100.0,
        EventData::RunStarted {
            run: run_id.clone(),
        },
    );
    append_run_event(
        &store,
        &idea_id,
        &run_id,
        110.0,
        EventData::StageStarted {
            run: run_id.clone(),
            stage: "plan".to_string(),
            attempt: 1,
        },
    );
    let _ = prepare_stage_job(
        &store,
        &run_id,
        "plan",
        1,
        "claude",
        "sonnet",
        JobState::Finished,
        Some(JobResult {
            status: JobResultStatus::Succeeded,
            summary: "6 slices".to_string(),
            files_changed: vec![],
            artifacts: vec![],
            rationale: None,
            hypothesis: None,
            usage: None,
            no_behaviour_change: false,
        }),
    );
    append_run_event(
        &store,
        &idea_id,
        &run_id,
        120.0,
        EventData::StageSucceeded {
            run: run_id.clone(),
            stage: "plan".to_string(),
        },
    );
    append_run_event(
        &store,
        &idea_id,
        &run_id,
        130.0,
        EventData::StageStarted {
            run: run_id.clone(),
            stage: "develop".to_string(),
            attempt: 1,
        },
    );
    let _ = prepare_stage_job(
        &store,
        &run_id,
        "develop",
        1,
        "claude",
        "claude-sonnet-4-6",
        JobState::Running,
        None,
    );

    let snap = collect_with_factory(&ws, &coord, Duration::from_secs(1), true);
    let factory = snap.factory.as_ref().unwrap();
    assert_eq!(factory.needs_you, 0);
    let card = factory.ideas.iter().find(|i| i.id == idea_id).unwrap();
    let rs = card.run.as_ref().unwrap();
    assert_eq!(rs.id, run_id);
    assert_eq!(rs.state, RunState::Running);
    assert_eq!(rs.running_jobs, 1);
    assert_eq!(rs.job_limit, 3);
    assert!(rs.elapsed_secs.is_some());
    assert_eq!(rs.cost_usd, None);
    assert_eq!(rs.pause_reason, None);
    let dev_stage = rs.stages.iter().find(|s| s.name == "develop").unwrap();
    assert_eq!(dev_stage.attempt, 1);
    assert_eq!(dev_stage.provider.as_deref(), Some("claude"));
    assert_eq!(dev_stage.model.as_deref(), Some("claude-sonnet-4-6"));
    assert_eq!(dev_stage.started, Some(130.0));

    // When a job reports usage.cost_usd, snapshot sums it; when paused, needs_you increments.
    let _ = prepare_stage_job(
        &store,
        &run_id,
        "develop",
        1,
        "claude",
        "claude-sonnet-4-6",
        JobState::Finished,
        Some(JobResult {
            status: JobResultStatus::Succeeded,
            summary: "implemented".to_string(),
            files_changed: vec![],
            artifacts: vec![],
            rationale: None,
            hypothesis: None,
            usage: Some(JobUsage {
                input_tokens: Some(100),
                output_tokens: Some(50),
                cost_usd: Some(0.42),
            }),
            no_behaviour_change: false,
        }),
    );
    pause_run_by_user(&store, &run_id, false, &user_actor(), &no_agent_env).unwrap();

    let snap2 = collect_with_factory(&ws, &coord, Duration::from_secs(1), true);
    let factory2 = snap2.factory.as_ref().unwrap();
    assert_eq!(factory2.needs_you, 1);
    let rs2 = factory2.ideas[0].run.as_ref().unwrap();
    assert_eq!(rs2.state, RunState::Paused);
    assert_eq!(rs2.pause_reason.as_deref(), Some("paused by user"));
    assert!((rs2.cost_usd.unwrap() - 0.42).abs() < 1e-6);

    // Normal sessions (factory_enabled = false) remain unaffected.
    let normal = collect_with_factory(&ws, &coord, Duration::from_secs(1), false);
    assert!(normal.factory.is_none());

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn test_detail_opens_run_monitor_and_responsive_variants() {
    let ws = temp_workspace("responsive");
    let (store, idea_id, run_id) = setup_run(&ws);

    append_run_event(
        &store,
        &idea_id,
        &run_id,
        100.0,
        EventData::RunStarted {
            run: run_id.clone(),
        },
    );
    append_run_event(
        &store,
        &idea_id,
        &run_id,
        110.0,
        EventData::StageStarted {
            run: run_id.clone(),
            stage: "plan".to_string(),
            attempt: 1,
        },
    );
    append_run_event(
        &store,
        &idea_id,
        &run_id,
        120.0,
        EventData::StageSucceeded {
            run: run_id.clone(),
            stage: "plan".to_string(),
        },
    );
    append_run_event(
        &store,
        &idea_id,
        &run_id,
        130.0,
        EventData::StageStarted {
            run: run_id.clone(),
            stage: "develop".to_string(),
            attempt: 1,
        },
    );
    let _ = prepare_stage_job(
        &store,
        &run_id,
        "develop",
        1,
        "claude",
        "sonnet",
        JobState::Running,
        None,
    );

    let mut detail = FactoryDetail::load(&ws, &idea_id, &[], false);
    // Detail must not contain the old p-015 placeholder text.
    let wide = Rect::new(0, 0, 130, 30);
    let mut buf = Buffer::empty(wide);
    detail.render(200.0, wide, &mut buf);
    let detail_out = buffer_text(&buf, wide);
    assert!(!detail_out.contains("Execution arrives with the execution phase"));

    // Pressing Enter in FactoryDetail opens RunMonitor.
    let outcome = detail.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        outcome,
        DetailKeyOutcome::Action(DetailAction::OpenRunMonitor {
            run_id: run_id.clone()
        })
    );

    let mut monitor = FactoryRun::load(&ws, &idea_id, &run_id, "darwin");

    // Wide (>= 120)
    let mut buf = Buffer::empty(wide);
    monitor.render(850.0, wide, &mut buf);
    let out_wide = buffer_text(&buf, wide);
    assert!(out_wide.contains(&run_id));
    assert!(out_wide.contains("Export team activity as CSV"));
    assert!(out_wide.contains("cost unknown"));
    assert!(out_wide.contains("Agents 1 of 3"));
    assert!(out_wide.contains("claude / sonnet"));
    assert!(out_wide.contains("attempt 1"));
    assert!(out_wide.contains("Ctrl+Opt+p"));

    // Medium (80..119)
    let med = Rect::new(0, 0, 90, 24);
    let mut buf = Buffer::empty(med);
    monitor.render(850.0, med, &mut buf);
    let out_med = buffer_text(&buf, med);
    assert!(out_med.contains(&run_id));
    assert!(out_med.contains("Agents 1 of 3"));

    // Narrow (< 80): stages listed vertically, compact activity
    let narrow = Rect::new(0, 0, 60, 24);
    let mut buf = Buffer::empty(narrow);
    monitor.render(850.0, narrow, &mut buf);
    let out_narrow = buffer_text(&buf, narrow);
    assert!(out_narrow.contains("plan"));
    assert!(out_narrow.contains("develop"));

    // Short (height <= 6): agents merged into header
    let short = Rect::new(0, 0, 130, 6);
    let mut buf = Buffer::empty(short);
    monitor.render(850.0, short, &mut buf);
    let out_short = buffer_text(&buf, short);
    assert!(out_short.contains("Agents 1 of 3"));

    // Linux shortcut display uses Alt
    let mut monitor_linux = FactoryRun::load(&ws, &idea_id, &run_id, "linux");
    let mut buf = Buffer::empty(wide);
    monitor_linux.render(850.0, wide, &mut buf);
    let out_linux = buffer_text(&buf, wide);
    assert!(out_linux.contains("Ctrl+Alt+p"));

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn test_stage_detail_and_sanitized_64k_log_reader() {
    let ws = temp_workspace("stage-log");
    let (store, idea_id, run_id) = setup_run(&ws);

    append_run_event(
        &store,
        &idea_id,
        &run_id,
        100.0,
        EventData::RunStarted {
            run: run_id.clone(),
        },
    );
    append_run_event(
        &store,
        &idea_id,
        &run_id,
        110.0,
        EventData::StageStarted {
            run: run_id.clone(),
            stage: "develop".to_string(),
            attempt: 1,
        },
    );
    let log_path = prepare_stage_job(
        &store,
        &run_id,
        "develop",
        1,
        "claude",
        "sonnet",
        JobState::Finished,
        Some(JobResult {
            status: JobResultStatus::Failed,
            summary: "2 tests failed\x1b[31m".to_string(),
            files_changed: vec!["src/export.rs".to_string()],
            artifacts: vec![],
            rationale: Some("empty team returns 500\x07".to_string()),
            hypothesis: Some("handle empty slice\x1b[0m".to_string()),
            usage: None,
            no_behaviour_change: false,
        }),
    );

    let mut monitor = FactoryRun::load(&ws, &idea_id, &run_id, "darwin");
    // Move selection to `develop` stage
    let dev_idx = monitor
        .run
        .as_ref()
        .unwrap()
        .stages
        .iter()
        .position(|s| s.name == "develop")
        .unwrap();
    monitor.selected_stage = dev_idx;

    // Open stage detail via Enter
    monitor.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::StageDetail { .. }));

    let area = Rect::new(0, 0, 120, 25);
    let mut buf = Buffer::empty(area);
    monitor.render(200.0, area, &mut buf);
    let out = buffer_text(&buf, area);
    assert!(out.contains("implementer"));
    assert!(out.contains("repo_read"));
    assert!(out.contains("2 tests failed"));
    assert!(out.contains("empty team returns 500"));
    assert!(out.contains("handle empty slice"));
    assert!(!out.contains('\x1b'));
    assert!(!out.contains('\x07'));

    // Esc returns to Main
    monitor.handle_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::Main));

    // Empty log state
    monitor.handle_key(&KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::LogReader { .. }));
    let mut buf = Buffer::empty(area);
    monitor.render(200.0, area, &mut buf);
    let out_empty = buffer_text(&buf, area);
    assert!(out_empty.contains("No log output yet."));

    // Write > 64 KiB log with ANSI escapes and refresh ('r')
    let mut big_log = String::from("VERY_FIRST_LINE_SHOULD_BE_TRUNCATED\n");
    big_log.push_str(&"FILLER_LINE_FOR_64K_TAIL_TEST\n".repeat(3000));
    big_log.push_str("\x1b[32mLATEST_SANITIZED_LINE\x1b[0m\x07\n");
    fs::write(&log_path, &big_log).unwrap();

    monitor.handle_key(&KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    if let RunSubView::LogReader {
        ref raw_text,
        truncated,
        ..
    } = monitor.sub_view
    {
        assert!(truncated);
        assert!(raw_text.len() <= 65_536);
        assert!(!raw_text.contains("VERY_FIRST_LINE_SHOULD_BE_TRUNCATED"));
        assert!(raw_text.contains("LATEST_SANITIZED_LINE"));
        assert!(!raw_text.contains('\x1b'));
        assert!(!raw_text.contains('\x07'));
    } else {
        panic!("expected LogReader");
    }

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn test_pause_resume_cancel_and_retry_controls() {
    let ws = temp_workspace("controls");
    let (store, idea_id, run_id) = setup_run(&ws);
    tandem::factory::broker::gate::consent_provider(
        &store,
        "claude",
        tandem::factory::broker::gate::AuthType::ApiKey,
        "claude",
        "argie",
        &no_agent_env,
    )
    .unwrap();
    let mut monitor = FactoryRun::load(&ws, &idea_id, &run_id, "darwin");

    // Pause via Ctrl+Alt+p
    monitor.handle_key(&KeyEvent::new(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert_eq!(
        get_run(&store, &run_id).unwrap().unwrap().state,
        RunState::Paused
    );

    // Resume via 'r' when paused by user
    monitor.handle_key(&KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    assert_eq!(
        get_run(&store, &run_id).unwrap().unwrap().state,
        RunState::Running
    );

    // Pause again and test Retry with hypothesis
    pause_run_by_user(&store, &run_id, false, &user_actor(), &no_agent_env).unwrap();
    append_run_event(
        &store,
        &idea_id,
        &run_id,
        200.0,
        EventData::RunPaused {
            run: run_id.clone(),
            reason: "stage \"test\" failed and retries are used up".to_string(),
            commands: vec![format!(
                "tandem factory run retry {run_id} test --hypothesis \"<new hypothesis>\""
            )],
            cap: None,
            spent: None,
            limit: None,
        },
    );
    monitor.reload();
    monitor.handle_key(&KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::RetryPrompt { .. }));
    assert!(monitor.captures_input());

    // Empty hypothesis rejected
    monitor.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::RetryPrompt { .. }));

    for ch in "fix empty slice".chars() {
        monitor.handle_key(&KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    monitor.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::Main));
    assert_eq!(
        get_run(&store, &run_id).unwrap().unwrap().state,
        RunState::Running
    );

    // Cancel requires typed run id
    monitor.handle_key(&KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert!(matches!(monitor.sub_view, RunSubView::ConfirmCancel { .. }));

    // Wrong run id rejected
    for ch in "wrong-id".chars() {
        monitor.handle_key(&KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    monitor.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::ConfirmCancel { .. }));

    // Clear and type exact run id
    for _ in 0..8 {
        monitor.handle_key(&KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    }
    for ch in run_id.chars() {
        monitor.handle_key(&KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    monitor.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        get_run(&store, &run_id).unwrap().unwrap().state,
        RunState::Cancelled
    );

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn test_paused_consent_dialog_and_unclear_gate_and_ambiguous_target() {
    let ws = temp_workspace("consent-target");
    let (store, idea_id, run_id) = setup_run(&ws);

    // 1. Paused: gate closed, consent possible (claude)
    append_run_event(
        &store,
        &idea_id,
        &run_id,
        150.0,
        EventData::RunPaused {
            run: run_id.clone(),
            reason: "the claude gate is closed (no consent).".to_string(),
            commands: vec![
                "tandem factory provider consent claude --auth api-key --confirm claude"
                    .to_string(),
            ],
            cap: None,
            spent: None,
            limit: None,
        },
    );
    let mut monitor = FactoryRun::load(&ws, &idea_id, &run_id, "darwin");
    let area = Rect::new(0, 0, 120, 25);
    let mut buf = Buffer::empty(area);
    monitor.render(200.0, area, &mut buf);
    let out = buffer_text(&buf, area);
    assert!(out.contains("Paused:"));
    assert!(out.contains("claude gate is closed"));

    // Open consent dialog ('a')
    monitor.handle_key(&KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::ConsentDialog { .. }));

    // Switch auth type to api-key (Right arrow from subscription -> api-key)
    monitor.handle_key(&KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    let mut buf = Buffer::empty(area);
    monitor.render(200.0, area, &mut buf);
    let out_consent = buffer_text(&buf, area);
    assert!(
        out_consent.contains("Factory Mode will run claude in headless mode for this workspace.")
    );
    assert!(out_consent.contains("It uses your own claude login. tandem handles no credentials."));
    assert!(out_consent.contains("Terms record:"));
    assert!(out_consent.contains("Authentication"));

    // Type "claude" and press Enter -> opens gate
    for ch in "claude".chars() {
        monitor.handle_key(&KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    monitor.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::Main));
    let settings = store.settings().unwrap();
    let claude_gate = settings
        .provider_gates
        .iter()
        .find(|g| g.provider == "claude")
        .unwrap();
    assert_eq!(
        claude_gate.state,
        tandem::factory::model::ProviderGateState::Open
    );

    // 2. Paused: gate cannot open (agy unclear)
    append_run_event(
        &store,
        &idea_id,
        &run_id,
        160.0,
        EventData::RunPaused {
            run: run_id.clone(),
            reason: "the agy gate is closed (terms verdict: unclear).".to_string(),
            commands: vec![],
            cap: None,
            spent: None,
            limit: None,
        },
    );
    monitor.reload();
    let mut buf = Buffer::empty(area);
    monitor.render(200.0, area, &mut buf);
    let out_unclear = buffer_text(&buf, area);
    assert!(out_unclear.contains("agy gate is closed"));
    assert!(out_unclear.contains("Choose another provider or cancel"));

    // 3. Paused: ambiguous target -> target picker
    let repo_a = ws.join("codebase-symlinks/repo-a");
    let repo_b = ws.join("codebase-symlinks/repo-b");
    fs::create_dir_all(repo_a.join(".git")).unwrap();
    fs::create_dir_all(repo_b.join(".git")).unwrap();
    append_run_event(
        &store,
        &idea_id,
        &run_id,
        170.0,
        EventData::RunPaused {
            run: run_id.clone(),
            reason: "choose a target repository: repo-a, repo-b".to_string(),
            commands: vec![
                format!("tandem factory run target {run_id} repo-a"),
                format!("tandem factory run target {run_id} repo-b"),
            ],
            cap: None,
            spent: None,
            limit: None,
        },
    );
    monitor.reload();
    monitor.handle_key(&KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::TargetPicker { .. }));

    let mut buf = Buffer::empty(area);
    monitor.render(200.0, area, &mut buf);
    let out_target = buffer_text(&buf, area);
    assert!(out_target.contains("repo-a"));
    assert!(out_target.contains("repo-b"));

    // Select second repo (Down -> Enter)
    monitor.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    monitor.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::Main));
    let updated_run = get_run(&store, &run_id).unwrap().unwrap();
    assert_eq!(updated_run.target.as_deref(), Some("repo-b"));

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn read_log_tail_sanitized_rejects_symlink_and_trims_partial_utf8() {
    let ws = temp_workspace("log-tail");
    let target = ws.join("real.log");
    fs::write(&target, "secret").unwrap();
    let link = ws.join("link.log");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let (text, truncated) = tandem::status::factory_run::read_log_tail_sanitized(&link);
        assert_eq!(text, "No log output yet.");
        assert!(!truncated);
    }

    // Write > 64 KiB with a multi-byte UTF-8 char straddling the 64 KiB boundary
    let big = ws.join("big.log");
    let prefix_len = tandem::status::factory_run::LOG_TAIL_BYTES - 1;
    let mut bytes = vec![b'a'; prefix_len + 10];
    // Place 'é' (0xC3 0xA9) across the cut boundary (last byte of tail start is 0xA9)
    let cut_offset = bytes.len() - tandem::status::factory_run::LOG_TAIL_BYTES;
    bytes[cut_offset - 1] = 0xC3;
    bytes[cut_offset] = 0xA9;
    bytes[cut_offset + 1] = b'Z';
    fs::write(&big, &bytes).unwrap();

    let (tail, truncated) = tandem::status::factory_run::read_log_tail_sanitized(&big);
    assert!(truncated);
    assert!(!tail.starts_with('\u{FFFD}'));
    assert!(tail.starts_with('Z'));

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn test_monitor_navigation_dialogs_errors_and_esc_paths() {
    use tandem::status::factory_run::RunKeyOutcome;

    let ws = temp_workspace("nav-esc");
    let (store, idea_id, run_id) = setup_run(&ws);
    tandem::factory::broker::gate::consent_provider(
        &store,
        "claude",
        tandem::factory::broker::gate::AuthType::ApiKey,
        "claude",
        "argie",
        &no_agent_env,
    )
    .unwrap();

    let log_path = prepare_stage_job(
        &store,
        &run_id,
        "plan",
        1,
        "claude",
        "sonnet",
        JobState::Finished,
        None,
    );
    let mut big_log = "LINE\n".repeat(15_000);
    big_log.push_str("END_OF_LOG\n");
    fs::write(&log_path, &big_log).unwrap();

    let mut monitor = FactoryRun::load(&ws, &idea_id, &run_id, "darwin");
    let area = Rect::new(0, 0, 120, 26);

    // Main does not capture input; Esc returns BackToDetail
    assert!(!monitor.captures_input());
    monitor.handle_key(&KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(monitor.selected_stage, 1);
    monitor.handle_key(&KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(monitor.selected_stage, 0);
    assert_eq!(
        monitor.handle_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        RunKeyOutcome::BackToDetail
    );

    // StageDetail -> 'l' opens LogReader -> scroll + render truncated -> Esc returns Main
    monitor.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::StageDetail { .. }));
    assert!(!monitor.captures_input());
    monitor.handle_key(&KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::LogReader { .. }));
    monitor.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    monitor.handle_key(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    let mut buf = Buffer::empty(area);
    monitor.render(200.0, area, &mut buf);
    let log_out = buffer_text(&buf, area);
    assert!(log_out.contains("LINE"));
    monitor.handle_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::Main));

    // Paused with retries exhausted -> 'r' opens RetryPrompt -> render error -> Esc back
    pause_run_by_user(&store, &run_id, false, &user_actor(), &no_agent_env).unwrap();
    append_run_event(
        &store,
        &idea_id,
        &run_id,
        150.0,
        EventData::RunPaused {
            run: run_id.clone(),
            reason: "stage \"plan\" failed and retries are used up".to_string(),
            commands: vec![format!(
                "tandem factory run retry {run_id} plan --hypothesis \"<new hypothesis>\""
            )],
            cap: None,
            spent: None,
            limit: None,
        },
    );
    monitor.reload();
    monitor.handle_key(&KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::RetryPrompt { .. }));
    assert!(monitor.captures_input());
    monitor.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let mut buf = Buffer::empty(area);
    monitor.render(200.0, area, &mut buf);
    assert!(buffer_text(&buf, area).contains("Hypothesis must not be empty"));
    monitor.handle_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::Main));

    // Paused -> 'c' opens ConfirmCancel -> wrong id error render -> Esc back
    monitor.handle_key(&KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::ConfirmCancel { .. }));
    for ch in "bad".chars() {
        monitor.handle_key(&KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    monitor.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let mut buf = Buffer::empty(area);
    monitor.render(200.0, area, &mut buf);
    assert!(buffer_text(&buf, area).contains(&run_id));
    monitor.handle_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::Main));

    // Gate closed -> 'a' opens ConsentDialog -> Up/Down/Left/Right + wrong confirm render + Esc
    append_run_event(
        &store,
        &idea_id,
        &run_id,
        160.0,
        EventData::RunPaused {
            run: run_id.clone(),
            reason: "the claude gate is closed (no consent).".to_string(),
            commands: vec![
                "tandem factory provider consent claude --auth api-key --confirm claude"
                    .to_string(),
            ],
            cap: None,
            spent: None,
            limit: None,
        },
    );
    monitor.reload();
    monitor.handle_key(&KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::ConsentDialog { .. }));
    monitor.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    monitor.handle_key(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    monitor.handle_key(&KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    monitor.handle_key(&KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    monitor.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let mut buf = Buffer::empty(area);
    monitor.render(200.0, area, &mut buf);
    assert!(buffer_text(&buf, area).contains("confirm consent"));
    monitor.handle_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::Main));

    // Target ambiguous -> 't' opens TargetPicker -> Down/Up + Esc
    fs::create_dir_all(ws.join("codebase-symlinks/repo-a/.git")).unwrap();
    fs::create_dir_all(ws.join("codebase-symlinks/repo-b/.git")).unwrap();
    append_run_event(
        &store,
        &idea_id,
        &run_id,
        170.0,
        EventData::RunPaused {
            run: run_id.clone(),
            reason: "choose a target repository: repo-a, repo-b".to_string(),
            commands: vec![
                format!("tandem factory run target {run_id} repo-a"),
                format!("tandem factory run target {run_id} repo-b"),
            ],
            cap: None,
            spent: None,
            limit: None,
        },
    );
    monitor.reload();
    monitor.handle_key(&KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::TargetPicker { .. }));
    monitor.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    monitor.handle_key(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    monitor.handle_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(matches!(monitor.sub_view, RunSubView::Main));

    // sync_if_changed with None
    monitor.sync_if_changed(None);

    let _ = fs::remove_dir_all(ws);
}
