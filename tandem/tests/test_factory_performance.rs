mod common;

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use tandem::config::Config;
use tandem::coordination::store::CoordStore;
use tandem::factory::audit::{self, AuditFilter};
use tandem::factory::commands::run_with_io;
use tandem::factory::events::{
    self, Event, append_with_max_bytes, full_log_read_count, indexed_read_count, read_all,
    read_idea_events, read_run_events, rebuild_index, reset_event_log_read_counters, sha256_hex,
};
use tandem::factory::machine::EventData;
use tandem::factory::model::{
    Actor, ActorKind, Idea, IdeaState, PrLink, Risk, Run, RunState, Stage, StageState,
};
use tandem::factory::recovery;
use tandem::factory::redact::RedactContext;
use tandem::factory::store::FactoryStore;
use tandem::factory::telemetry::{self, Cost, CostBasis};
use tandem::snapshot::{collect, collect_with_factory};
use tandem::status::factory_audit::FactoryAudit;
use tandem::status::factory_tab::{BOARD_PAGE_SIZE, FactoryTab, IdeaFilter};

fn system_actor() -> Actor {
    Actor {
        kind: ActorKind::System,
        id: "perf-test".to_string(),
    }
}

fn user_actor() -> Actor {
    Actor {
        kind: ActorKind::User,
        id: "tester".to_string(),
    }
}

fn run_factory_cli(
    args: &[&str],
    ws: &Path,
    cfg: &Config,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
) -> i32 {
    let mut full = vec!["tandem", "factory"];
    full.extend_from_slice(args);
    let cli = tandem::cli::parse_from(full).unwrap();
    let tandem::cli::Command::Factory(f_args) = cli.command.unwrap() else {
        panic!("expected factory command");
    };
    let mut stdin = std::io::empty();
    run_with_io(&f_args, ws, cfg, &mut stdin, stdout, stderr)
}

fn buffer_text(buf: &Buffer, area: Rect) -> String {
    let mut out = String::new();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn seed_large_factory_workspace(ws: &Path, num_ideas: usize, num_runs: usize, num_events: usize) {
    let store = FactoryStore::new(ws);
    store.ensure().expect("ensure factory store");
    fs::create_dir_all(store.root.join("runs")).expect("create runs dir");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(1_750_000_000.0);
    let mut idea_ids = Vec::with_capacity(num_ideas);

    for i in 0..num_ideas {
        let id = format!("i-{:08}", i + 1);
        let state = match i % 5 {
            0 => IdeaState::Inbox,
            1 => IdeaState::Planning,
            2 => IdeaState::InDevelopment,
            3 => IdeaState::Delivered,
            _ => IdeaState::Review,
        };
        let run_id = if i < num_runs {
            Some(format!("r-{:08}", i + 1))
        } else {
            None
        };
        let idea = Idea {
            id: id.clone(),
            title: format!("Perf Idea {}", i + 1),
            text: "Generated performance test idea".to_string(),
            state,
            owner: "tester".to_string(),
            created: now - 3600.0,
            updated: now - (i as f64),
            risk: Some(Risk::Low),
            orchestrator: None,
            feasibility: None,
            feasibility_verdict: None,
            plan: None,
            plan_snapshot: None,
            ready_via: None,
            ready_at: None,
            artifacts: Vec::new(),
            assumptions: Vec::new(),
            runs: run_id.iter().cloned().collect(),
            prs: Vec::new(),
            cancelled_from: None,
        };
        let idea_path = store.root.join("ideas").join(format!("{id}.json"));
        fs::write(&idea_path, serde_json::to_vec(&idea).unwrap()).unwrap();

        if let Some(ref rid) = run_id {
            let run_dir = store.run_dir(rid);
            fs::create_dir_all(&run_dir).unwrap();
            let run = Run {
                id: rid.clone(),
                idea: id.clone(),
                workflow: "default-delivery".to_string(),
                workflow_version: "1".to_string(),
                workflow_sha256: "abc".to_string(),
                workflow_source: "default".to_string(),
                plan_sha256: "def".to_string(),
                deploy_requested: false,
                target: Some("tandem".to_string()),
                branch: Some(format!("factory/{rid}")),
                base_commit: Some("000000".to_string()),
                worktree_path: None,
                pause_reason: None,
                action_commands: Vec::new(),
                baseline_passed: Some(true),
                state: RunState::Running,
                stages: vec![Stage {
                    name: "develop".to_string(),
                    agent: "senior-backend-dev".to_string(),
                    state: StageState::Running,
                    attempts: 1,
                    interrupted_attempts: 0,
                    started: Some(now - 100.0),
                    finished: None,
                    artifacts: Vec::new(),
                    rationale: None,
                    retry_hypothesis: None,
                }],
                evidence: Vec::new(),
                prs: Vec::new(),
                budget: None,
                cost: Some(Cost {
                    value: Some(0.25),
                    basis: CostBasis::Actual,
                }),
                created: now - 200.0,
                updated: now - 10.0,
            };
            fs::write(run_dir.join("run.json"), serde_json::to_vec(&run).unwrap()).unwrap();
        }
        idea_ids.push(id);
    }

    let events_path = store.root.join("events.jsonl");
    let mut raw_log = String::with_capacity(num_events * 160);
    let mut prev_hash: Option<String> = None;
    for seq in 1..=num_events {
        let idea_idx = (seq - 1) % num_ideas;
        let run_opt = if idea_idx < num_runs {
            Some(format!("r-{:08}", idea_idx + 1))
        } else {
            None
        };
        let (ev_type, data) = if let Some(ref rid) = run_opt {
            (
                "stage.started",
                serde_json::json!({ "run": rid, "stage": "develop", "attempt": 1 }),
            )
        } else {
            (
                "idea.created",
                serde_json::json!({
                    "id": idea_ids[idea_idx],
                    "title": "Perf",
                    "text": "Body",
                    "owner": "tester",
                }),
            )
        };
        let ev = Event {
            v: 1,
            seq: seq as u64,
            ts: now - (num_events - seq) as f64 * 0.01,
            actor: system_actor(),
            r#type: ev_type.to_string(),
            idea: idea_ids[idea_idx].clone(),
            run: run_opt,
            data,
            prev: prev_hash.clone(),
        };
        let line = serde_json::to_string(&ev).unwrap();
        prev_hash = Some(sha256_hex(line.as_bytes()));
        raw_log.push_str(&line);
        raw_log.push('\n');
    }
    fs::write(&events_path, raw_log.as_bytes()).unwrap();
    rebuild_index(&events_path).expect("build initial events.index");
}

#[test]
fn test_factory_snapshot_refresh_materialized_only_and_budget() {
    let tmp = common::TempDir::new();
    let ws = tmp.path();
    seed_large_factory_workspace(ws, 200, 50, 50_000);

    let coord = CoordStore::new(ws.join(".agent-workspace/coordination"));

    reset_event_log_read_counters();
    let t_factory = Instant::now();
    let snap = collect_with_factory(ws, &coord, Duration::from_millis(200), true);
    let factory_dur = t_factory.elapsed();

    assert_eq!(
        full_log_read_count(),
        0,
        "factory snapshot refresh must never read the full event log"
    );
    assert_eq!(
        indexed_read_count(),
        0,
        "factory snapshot refresh must read materialized files only, not the event log"
    );
    assert_eq!(
        snap.factory.as_ref().map(|f| f.ideas.len()),
        Some(200),
        "expected 200 ideas in factory snapshot"
    );
    assert!(
        factory_dur < Duration::from_millis(1500),
        "factory snapshot refresh took {:?}, exceeded 1500ms budget",
        factory_dur
    );

    reset_event_log_read_counters();
    let t_normal = Instant::now();
    let normal_snap = collect(ws, &coord, Duration::from_millis(200));
    let normal_dur = t_normal.elapsed();

    assert!(normal_snap.factory.is_none());
    assert_eq!(full_log_read_count(), 0);
    assert_eq!(indexed_read_count(), 0);
    assert!(
        normal_dur < Duration::from_millis(500),
        "normal snapshot refresh took {:?}, exceeded 500ms budget",
        normal_dur
    );
}

#[test]
fn test_factory_tab_renders_200_ideas_within_frame_budget() {
    let tmp = common::TempDir::new();
    let ws = tmp.path();
    seed_large_factory_workspace(ws, 200, 50, 1_000);

    let coord = CoordStore::new(ws.join(".agent-workspace/coordination"));
    let snap = collect_with_factory(ws, &coord, Duration::from_millis(200), true);

    let mut tab = FactoryTab {
        workspace: Some(ws.to_path_buf()),
        ..Default::default()
    };
    tab.sync_snapshot(&snap, false);

    let area = Rect::new(0, 0, 120, 40);
    let mut buf = Buffer::empty(area);

    let start = Instant::now();
    tab.render(&snap, area, &mut buf);
    let render_dur = start.elapsed();

    assert!(
        render_dur < Duration::from_millis(50),
        "FactoryTab render for 200 ideas took {:?}, exceeded 50ms frame budget",
        render_dur
    );
}

#[test]
fn test_idea_show_and_audit_view_use_index_not_full_log() {
    let tmp = common::TempDir::new();
    let ws = tmp.path();
    seed_large_factory_workspace(ws, 200, 50, 50_000);

    let target_idea = "i-00000042";

    reset_event_log_read_counters();
    let audit_entries =
        audit::entries(&FactoryStore::new(ws), target_idea, &AuditFilter::default())
            .expect("audit entries");
    assert_eq!(
        full_log_read_count(),
        0,
        "audit::entries must use index, not full log read"
    );
    assert!(
        indexed_read_count() > 0,
        "audit::entries should perform an indexed read"
    );
    assert_eq!(audit_entries.len(), 250);

    reset_event_log_read_counters();
    let audit_view = FactoryAudit::load(ws, target_idea);
    assert_eq!(
        full_log_read_count(),
        0,
        "FactoryAudit::load must not read the full event log"
    );
    assert!(
        indexed_read_count() > 0,
        "FactoryAudit::load must use indexed read"
    );
    assert_eq!(audit_view.all_entries.len(), 250);

    reset_event_log_read_counters();
    let cfg = Config {
        factory: true,
        ..Default::default()
    };
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run_factory_cli(
        &["idea", "show", target_idea, "--json"],
        ws,
        &cfg,
        &mut out,
        &mut err,
    );
    assert_eq!(
        code,
        0,
        "idea show failed: {}",
        String::from_utf8_lossy(&err)
    );
    assert_eq!(
        full_log_read_count(),
        0,
        "factory idea show must not read the full event log"
    );
    assert!(
        indexed_read_count() > 0,
        "factory idea show must read through index"
    );
    let parsed: serde_json::Value = serde_json::from_slice(&out).expect("parse idea show json");
    assert_eq!(parsed["events"].as_array().unwrap().len(), 250);
}

#[test]
fn test_event_log_segment_rotation_and_hash_chain() {
    let tmp = common::TempDir::new();
    let ws = tmp.path();
    let store = FactoryStore::new(ws);
    store.ensure().expect("ensure store");
    let events_path = store.root.join("events.jsonl");
    let lock_path = store.root.join(".lock");
    let ctx = RedactContext::default();

    let idea = store
        .create_idea(
            "Segmented Idea",
            "Testing segment rotation",
            "tester",
            &user_actor(),
        )
        .expect("create idea");

    let mut total_appended = 1usize;
    for idx in 0..24 {
        let ev = Event::from_data(
            1_750_000_000.0 + idx as f64,
            system_actor(),
            idea.id.clone(),
            Some("run-seg-1".to_string()),
            &EventData::StageStarted {
                run: "run-seg-1".to_string(),
                stage: format!("stage-{}", idx % 3),
                attempt: (idx as u32) + 1,
                grant: None,
            },
        )
        .unwrap();
        let redacted = events::redact_event(ev, &ctx);
        append_with_max_bytes(&events_path, &lock_path, &[redacted], 700).expect("append");
        total_appended += 1;
    }

    assert!(
        store.root.join("events-0001.jsonl").is_file(),
        "expected closed segment events-0001.jsonl"
    );
    assert!(
        store.root.join("events-0002.jsonl").is_file(),
        "expected closed segment events-0002.jsonl"
    );
    assert!(events_path.is_file(), "expected active events.jsonl");

    let verify = audit::verify(&events_path).expect("verify across segments");
    assert!(
        verify.intact,
        "hash chain broke across segments: {:?}",
        verify.first_break
    );
    assert_eq!(verify.total_events, total_appended);

    let all = read_all(&events_path).expect("read_all across segments");
    assert_eq!(all.events.len(), total_appended);
    for (idx, ev) in all.events.iter().enumerate() {
        assert_eq!(ev.seq, (idx + 1) as u64);
    }

    let by_idea = read_idea_events(&events_path, &idea.id).expect("read_idea_events");
    assert_eq!(by_idea.len(), total_appended);

    let by_run = read_run_events(&events_path, "run-seg-1").expect("read_run_events");
    assert_eq!(by_run.len(), total_appended - 1);

    let index_path = store.root.join("events.index");
    assert!(
        index_path.is_file(),
        "expected events.index to be maintained"
    );
    fs::remove_file(&index_path).unwrap();

    rebuild_index(&events_path).expect("rebuild_index");
    let by_idea_rebuilt =
        read_idea_events(&events_path, &idea.id).expect("read_idea_events after rebuild");
    assert_eq!(by_idea_rebuilt.len(), total_appended);

    let rebuild_rep = store.rebuild().expect("store.rebuild across segments");
    assert_eq!(rebuild_rep.ideas, 1);

    let diag = recovery::diagnose(&store, ws).expect("recovery::diagnose across segments");
    assert!(
        diag.chain_intact,
        "doctor should see intact chain across segments"
    );
    assert!(!diag.truncated_log);
}

#[test]
fn test_legacy_single_file_log_compatibility() {
    let tmp = common::TempDir::new();
    let ws = tmp.path();
    let store = FactoryStore::new(ws);
    store.ensure().expect("ensure store");
    let events_path = store.root.join("events.jsonl");

    let idea = store
        .create_idea("Legacy Idea", "Single file log", "tester", &user_actor())
        .expect("create idea");
    let index_path = store.root.join("events.index");
    if index_path.exists() {
        fs::remove_file(&index_path).unwrap();
    }

    let verify = audit::verify(&events_path).expect("verify single file");
    assert!(verify.intact);
    assert_eq!(verify.total_events, 1);

    let by_idea = read_idea_events(&events_path, &idea.id).expect("read_idea_events without index");
    assert_eq!(by_idea.len(), 1);

    let diag = recovery::diagnose(&store, ws).expect("diagnose single file");
    assert!(diag.chain_intact);
}

#[test]
fn test_factory_tab_archived_filter_hides_old_finished_ideas() {
    let tmp = common::TempDir::new();
    let ws = tmp.path();
    let store = FactoryStore::new(ws);
    store.ensure().expect("ensure store");

    let now = 1_750_000_000.0;
    let old_ts = now - 35.0 * 86_400.0;
    let recent_ts = now - 5.0 * 86_400.0;

    let specs = [
        (
            "i-active01",
            "Active Idea",
            IdeaState::InDevelopment,
            recent_ts,
        ),
        (
            "i-recdel01",
            "Recent Delivered",
            IdeaState::Delivered,
            recent_ts,
        ),
        ("i-olddel01", "Old Delivered", IdeaState::Delivered, old_ts),
        ("i-oldcan01", "Old Cancelled", IdeaState::Cancelled, old_ts),
    ];

    for (id, title, state, updated) in specs {
        let idea = Idea {
            id: id.to_string(),
            title: title.to_string(),
            text: "body".to_string(),
            state,
            owner: "tester".to_string(),
            created: updated - 100.0,
            updated,
            risk: None,
            orchestrator: None,
            feasibility: None,
            feasibility_verdict: None,
            plan: None,
            plan_snapshot: None,
            ready_via: None,
            ready_at: None,
            artifacts: Vec::new(),
            assumptions: Vec::new(),
            runs: Vec::new(),
            prs: Vec::new(),
            cancelled_from: None,
        };
        fs::write(
            store.root.join("ideas").join(format!("{id}.json")),
            serde_json::to_vec(&idea).unwrap(),
        )
        .unwrap();
    }

    let coord = CoordStore::new(ws.join(".agent-workspace/coordination"));
    let mut snap = collect_with_factory(ws, &coord, Duration::from_millis(200), true);
    snap.now = now;

    let mut tab = FactoryTab::default();
    tab.sync_snapshot(&snap, false);

    let default_filtered = tab.filtered_ideas(&snap);
    let default_ids: Vec<&str> = default_filtered.iter().map(|i| i.id.as_str()).collect();
    assert!(default_ids.contains(&"i-active01"));
    assert!(default_ids.contains(&"i-recdel01"));
    assert!(!default_ids.contains(&"i-olddel01"));
    assert!(!default_ids.contains(&"i-oldcan01"));

    let area = Rect::new(0, 0, 120, 20);
    let mut buf = Buffer::empty(area);
    tab.render(&snap, area, &mut buf);
    let default_text = buffer_text(&buf, area);
    assert!(
        default_text.contains("2 archived"),
        "header should show archived count: {default_text}"
    );

    tab.filter = IdeaFilter::Archived;
    tab.sync_snapshot(&snap, false);
    let archived_filtered = tab.filtered_ideas(&snap);
    let archived_ids: Vec<&str> = archived_filtered.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(archived_ids.len(), 2);
    assert!(archived_ids.contains(&"i-olddel01"));
    assert!(archived_ids.contains(&"i-oldcan01"));

    assert!(store.root.join("ideas/i-olddel01.json").is_file());
    assert!(store.root.join("ideas/i-oldcan01.json").is_file());
}

#[test]
fn test_factory_tab_pages_over_200_ideas() {
    let tmp = common::TempDir::new();
    let ws = tmp.path();
    seed_large_factory_workspace(ws, 250, 0, 250);

    let coord = CoordStore::new(ws.join(".agent-workspace/coordination"));
    let snap = collect_with_factory(ws, &coord, Duration::from_millis(200), true);

    let mut tab = FactoryTab::default();
    tab.sync_snapshot(&snap, false);

    assert_eq!(BOARD_PAGE_SIZE, 200);
    assert_eq!(tab.page_count(&snap), 2);
    assert_eq!(tab.current_page(), 0);
    assert_eq!(tab.paged_ideas(&snap).len(), 200);

    tab.handle_key(&KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE), &snap);
    assert_eq!(tab.current_page(), 1);
    assert_eq!(tab.paged_ideas(&snap).len(), 50);

    let area = Rect::new(0, 0, 120, 20);
    let mut buf = Buffer::empty(area);
    tab.render(&snap, area, &mut buf);
    let text = buffer_text(&buf, area);
    assert!(
        text.contains("page 2/2"),
        "expected page indicator in header: {text}"
    );
}

#[test]
fn test_pilot_report_json_and_markdown() {
    let tmp = common::TempDir::new();
    let ws = tmp.path();
    let store = FactoryStore::new(ws);
    store.ensure().expect("ensure store");
    let events_path = store.root.join("events.jsonl");
    let lock_path = store.root.join(".lock");
    let ctx = RedactContext::default();

    let idea1 = "i-rep00001";
    let run1 = "r-rep00001";
    let base_ts = 1_750_000_000.0;

    let raw_events = vec![
        (
            base_ts + 0.0,
            EventData::IdeaCreated {
                id: idea1.to_string(),
                title: "Pilot Idea 1".to_string(),
                text: "Body".to_string(),
                owner: "tester".to_string(),
            },
        ),
        (base_ts + 60.0, EventData::IdeaDiscoveryStarted {}),
        (base_ts + 120.0, EventData::IdeaDirectionSelected {}),
        (base_ts + 180.0, EventData::IdeaReady {}),
        (
            base_ts + 240.0,
            EventData::RunCreated {
                run: run1.to_string(),
                workflow: "default-delivery".to_string(),
                version: "1".to_string(),
                sha256: "wf1".to_string(),
                source: "default".to_string(),
                plan_sha256: "snap1".to_string(),
                deploy_requested: false,
                stages: vec![Stage {
                    name: "develop".to_string(),
                    agent: "senior-backend-dev".to_string(),
                    state: StageState::Pending,
                    attempts: 0,
                    interrupted_attempts: 0,
                    started: None,
                    finished: None,
                    artifacts: Vec::new(),
                    rationale: None,
                    retry_hypothesis: None,
                }],
                budget: None,
            },
        ),
        (
            base_ts + 300.0,
            EventData::RunStarted {
                run: run1.to_string(),
            },
        ),
        (
            base_ts + 300.0,
            EventData::StageStarted {
                run: run1.to_string(),
                stage: "develop".to_string(),
                attempt: 1,
                grant: None,
            },
        ),
        (
            base_ts + 330.0,
            EventData::StageFailed {
                run: run1.to_string(),
                stage: "develop".to_string(),
                reason: "lint error".to_string(),
            },
        ),
        (
            base_ts + 340.0,
            EventData::StageInterrupted {
                run: run1.to_string(),
                stage: "develop".to_string(),
            },
        ),
        (
            base_ts + 350.0,
            EventData::StageModelUpgraded {
                run: run1.to_string(),
                stage: "develop".to_string(),
                from: "haiku".to_string(),
                to: "sonnet".to_string(),
            },
        ),
        (
            base_ts + 360.0,
            EventData::StageStarted {
                run: run1.to_string(),
                stage: "develop".to_string(),
                attempt: 2,
                grant: None,
            },
        ),
        (
            base_ts + 420.0,
            EventData::JobUsage {
                run: run1.to_string(),
                stage: "develop".to_string(),
                attempt: 2,
                provider: "claude".to_string(),
                model: "sonnet".to_string(),
                tier: "standard".to_string(),
                duration_secs: 60.0,
                input_tokens: Some(1000),
                output_tokens: Some(500),
                cost: Cost {
                    value: Some(1.25),
                    basis: CostBasis::Actual,
                },
                cost_usd: Some(1.25),
            },
        ),
        (
            base_ts + 420.0,
            EventData::StageSucceeded {
                run: run1.to_string(),
                stage: "review".to_string(),
            },
        ),
        (
            base_ts + 480.0,
            EventData::RunStagesComplete {
                run: run1.to_string(),
            },
        ),
        (
            base_ts + 540.0,
            EventData::IdeaDelivered {
                run: run1.to_string(),
                prs: vec![PrLink {
                    repo: "repo".to_string(),
                    number: 10,
                    url: "https://example.com/pr/10".to_string(),
                    state: Some("merged".to_string()),
                    checks_summary: Some("pass".to_string()),
                }],
            },
        ),
    ];

    let mut redacted_batch = Vec::new();
    for (ts, data) in &raw_events {
        let ev = Event::from_data(
            *ts,
            system_actor(),
            idea1.to_string(),
            Some(run1.to_string()),
            data,
        )
        .unwrap();
        redacted_batch.push(events::redact_event(ev, &ctx));
    }
    events::append(&events_path, &lock_path, &redacted_batch).unwrap();
    store.rebuild().unwrap();

    let report = telemetry::pilot_report(&store, base_ts + 600.0).expect("pilot_report");
    assert_eq!(report.totals.total_ideas, 1);
    assert_eq!(report.totals.delivered_ideas, 1);
    assert_eq!(report.totals.rework.review_rounds, 1);
    assert_eq!(report.totals.rework.model_upgrades, 1);
    assert_eq!(report.totals.recoveries_performed, 1);
    assert_eq!(report.totals.pr_counts.get("merged").copied(), Some(1));
    assert_eq!(report.totals.cost.value, Some(1.25));
    assert_eq!(report.totals.cost.basis, CostBasis::Actual);

    let cfg = Config {
        factory: true,
        ..Default::default()
    };
    let mut out_json = Vec::new();
    let mut err = Vec::new();
    let code = run_factory_cli(&["report", "--json"], ws, &cfg, &mut out_json, &mut err);
    assert_eq!(
        code,
        0,
        "report --json failed: {}",
        String::from_utf8_lossy(&err)
    );
    let json_val: serde_json::Value = serde_json::from_slice(&out_json).expect("valid report json");
    assert_eq!(json_val["totals"]["delivered_ideas"], 1);

    let mut out_md = Vec::new();
    let code_md = run_factory_cli(&["report", "--markdown"], ws, &cfg, &mut out_md, &mut err);
    assert_eq!(
        code_md,
        0,
        "report --markdown failed: {}",
        String::from_utf8_lossy(&err)
    );
    let md_str = String::from_utf8(out_md).unwrap();
    assert!(md_str.contains("# Factory Pilot Report"));
    assert!(md_str.contains(idea1));
}
