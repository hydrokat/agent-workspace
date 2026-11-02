mod common;

use std::fs;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use tandem::coordination::schema::Claim;
use tandem::coordination::store::CoordStore;
use tandem::snapshot::StatusSnapshot;
use tandem::sources::ongoing::{OngoingSpec, OngoingTask};
use tandem::sources::specs::{Phase, Task};
use tandem::status::app::{StatusApp, StatusTab};
use tandem::status::task_tab;
use tandem::tmux::client::{Result as TmuxResult, TmuxClient, TmuxRunner};
use tandem::ui::scroll::ScrollView;

struct MockTmuxRunner {
    calls: Mutex<Vec<Vec<String>>>,
}

impl MockTmuxRunner {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl TmuxRunner for MockTmuxRunner {
    fn run(&self, argv: &[String]) -> TmuxResult {
        self.calls.lock().unwrap().push(argv.to_vec());
        TmuxResult {
            code: 0,
            out: String::new(),
            err: String::new(),
        }
    }
}

fn create_mock_tmux() -> (TmuxClient, Arc<MockTmuxRunner>) {
    let runner = Arc::new(MockTmuxRunner::new());
    let client = TmuxClient::with_runner("tandem-test", None, runner.clone());
    (client, runner)
}

fn render_lines_to_text(lines: &[Line], width: u16, height: u16) -> String {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    let mut scroll = ScrollView::new();
    scroll.render_lines(lines, Rect::new(0, 0, width, height), &mut buf);
    buffer_text(&buf)
}

fn buffer_text(buf: &Buffer) -> String {
    let mut s = String::new();
    let area = buf.area;
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell((x, y)) {
                s.push_str(cell.symbol());
            }
        }
        s.push('\n');
    }
    s
}

#[test]
fn test_empty_state_when_no_task_in_progress() {
    let snapshot = StatusSnapshot::default();
    let lines = task_tab::lines(&snapshot, 80);
    assert_eq!(lines.len(), 1);
    let text = render_lines_to_text(&lines, 80, 5);
    assert!(text.contains("No task is in progress."));
}

#[test]
fn test_one_task_with_claim_and_progress() {
    let task = OngoingTask {
        phase: "p-014-agent-workspace-package".to_string(),
        task_id: "t-003".to_string(),
        title: "Sprint tab: the ongoing sprint in full".to_string(),
        status: "In Progress".to_string(),
        objectives_done: 3,
        objectives_total: 6,
        claim: Some(Claim {
            phase: "p-014-agent-workspace-package".to_string(),
            task: "t-003".to_string(),
            session: "s-96579395".to_string(),
            branch: "feat/tandem-status-tabs".to_string(),
            worktree: "agent-workspace-package-dx".to_string(),
            paths: vec![
                "tandem/src/status/sprint_tab.rs".to_string(),
                "tandem/tests/test_status_sprint_tab.rs".to_string(),
            ],
            heartbeat: 960.0,
        }),
        session_pane: Some("%3".to_string()),
        stale: false,
        status_mismatch: false,
        next_step: Some("Write the failing buffer tests".to_string()),
    };

    let snapshot = StatusSnapshot {
        ongoing_tasks: vec![task],
        now: 1000.0,
        ..Default::default()
    };

    let lines = task_tab::lines(&snapshot, 100);
    let text = render_lines_to_text(&lines, 100, 15);

    assert!(text.contains("p-014"));
    assert!(text.contains("t-003"));
    assert!(text.contains("Sprint tab: the ongoing sprint in full"));
    assert!(text.contains("objectives 3 / 6"));
    assert!(text.contains("owner"));
    assert!(text.contains("s-96579395 (pane %3)"));
    assert!(text.contains("heartbeat 40s ago"));
    assert!(text.contains("branch"));
    assert!(text.contains("feat/tandem-status-tabs"));
    assert!(text.contains("worktree"));
    assert!(text.contains("agent-workspace-package-dx"));
    assert!(text.contains("paths"));
    assert!(text.contains("tandem/src/status/sprint_tab.rs"));
    assert!(text.contains("tandem/tests/test_status_sprint_tab.rs"));
    assert!(text.contains("next"));
    assert!(text.contains("Write the failing buffer tests"));
}

#[test]
fn test_unclaimed_task_shows_unclaimed_flag() {
    let task = OngoingTask {
        phase: "p-014".to_string(),
        task_id: "t-004".to_string(),
        title: "Spec tab".to_string(),
        status: "In Progress".to_string(),
        objectives_done: 0,
        objectives_total: 5,
        claim: None,
        session_pane: None,
        stale: false,
        status_mismatch: false,
        next_step: None,
    };

    let snapshot = StatusSnapshot {
        ongoing_tasks: vec![task],
        ..Default::default()
    };

    let lines = task_tab::lines(&snapshot, 80);
    let text = render_lines_to_text(&lines, 80, 10);

    assert!(text.contains("unclaimed"));
}

#[test]
fn test_stale_claim_and_status_mismatch_flags() {
    let task = OngoingTask {
        phase: "p-012".to_string(),
        task_id: "t-006".to_string(),
        title: "Live upgrade rehearsal".to_string(),
        status: "Pending".to_string(),
        objectives_done: 0,
        objectives_total: 0,
        claim: Some(Claim {
            phase: "p-012".to_string(),
            task: "t-006".to_string(),
            session: "s-914485dd".to_string(),
            branch: String::new(),
            worktree: String::new(),
            paths: Vec::new(),
            heartbeat: 0.0,
        }),
        session_pane: None,
        stale: true,
        status_mismatch: true,
        next_step: None,
    };

    let snapshot = StatusSnapshot {
        ongoing_tasks: vec![task],
        ..Default::default()
    };

    let lines = task_tab::lines(&snapshot, 80);
    let text = render_lines_to_text(&lines, 80, 10);

    assert!(text.contains("stale"));
    assert!(text.contains("status mismatch"));
}

#[test]
fn test_bottom_summary_in_review_and_blocked() {
    let phase = Phase {
        name: "p-014".to_string(),
        title: String::new(),
        branch: String::new(),
        target: String::new(),
        tasks: vec![
            Task::new("t-001", "Task 1", "In Review"),
            Task::new("t-002", "Task 2", "Blocked"),
            Task::new("t-003", "Task 3", "In Progress"),
        ],
        warning: String::new(),
        archived: false,
    };

    let snapshot = StatusSnapshot {
        ongoing_specs: vec![OngoingSpec {
            phase,
            next_up: false,
        }],
        ongoing_tasks: vec![OngoingTask {
            phase: "p-014".to_string(),
            task_id: "t-003".to_string(),
            title: "Task 3".to_string(),
            status: "In Progress".to_string(),
            objectives_done: 0,
            objectives_total: 0,
            claim: None,
            session_pane: None,
            stale: false,
            status_mismatch: false,
            next_step: None,
        }],
        ..Default::default()
    };

    let lines = task_tab::lines(&snapshot, 80);
    let text = render_lines_to_text(&lines, 80, 10);

    assert!(text.contains("In Review: 1 · Blocked: 1"));
}

#[test]
fn test_untrusted_text_is_sanitized() {
    let task = OngoingTask {
        phase: "p-014\x1b[31m".to_string(),
        task_id: "t-001".to_string(),
        title: "Title\x1b[0mSanitized".to_string(),
        status: "In Progress".to_string(),
        objectives_done: 1,
        objectives_total: 2,
        claim: Some(Claim {
            phase: "p-014".to_string(),
            task: "t-001".to_string(),
            session: "sess\x1b[32mion".to_string(),
            branch: "br\x1b[33manch".to_string(),
            worktree: "wt\x1b[34mree".to_string(),
            paths: vec!["path\x1b[35m/one.rs".to_string()],
            heartbeat: 100.0,
        }),
        session_pane: Some("%1\x1b[0m".to_string()),
        stale: false,
        status_mismatch: false,
        next_step: Some("Next\x1b[36mStep".to_string()),
    };

    let snapshot = StatusSnapshot {
        ongoing_tasks: vec![task],
        now: 100.0,
        ..Default::default()
    };

    let lines = task_tab::lines(&snapshot, 80);
    let text = render_lines_to_text(&lines, 80, 15);

    assert!(!text.contains('\x1b'));
    assert!(text.contains("TitleSanitized"));
    assert!(text.contains("session"));
    assert!(text.contains("branch"));
    assert!(text.contains("wtree"));
    assert!(text.contains("path/one.rs"));
    assert!(text.contains("NextStep"));
}

#[test]
fn test_narrow_width_responsive_single_letter_labels() {
    let task = OngoingTask {
        phase: "p-014".to_string(),
        task_id: "t-001".to_string(),
        title: "Task".to_string(),
        status: "In Progress".to_string(),
        objectives_done: 0,
        objectives_total: 0,
        claim: Some(Claim {
            phase: "p-014".to_string(),
            task: "t-001".to_string(),
            session: "s-123".to_string(),
            branch: "feat".to_string(),
            worktree: "tree".to_string(),
            paths: vec!["file.rs".to_string()],
            heartbeat: 90.0,
        }),
        session_pane: Some("%2".to_string()),
        stale: false,
        status_mismatch: false,
        next_step: Some("step".to_string()),
    };

    let snapshot = StatusSnapshot {
        ongoing_tasks: vec![task],
        now: 100.0,
        ..Default::default()
    };

    let lines = task_tab::lines(&snapshot, 35);
    let text = render_lines_to_text(&lines, 35, 15);

    assert!(text.contains("o "));
    assert!(text.contains("b "));
    assert!(text.contains("p "));
    assert!(text.contains("n "));
}

#[test]
fn test_alt_g_goto_owning_pane_on_task_tab() {
    let tmp = common::TempDir::new();
    let ws = tmp.path().join("ws");
    fs::create_dir_all(&ws).unwrap();

    let (tmux, runner) = create_mock_tmux();
    let mut app = StatusApp::new(ws, CoordStore::new(tmp.path().join("c")), Some(tmux), None);

    app.snapshot.ongoing_tasks = vec![OngoingTask {
        phase: "p-014".to_string(),
        task_id: "t-003".to_string(),
        title: "Sprint tab".to_string(),
        status: "In Progress".to_string(),
        objectives_done: 0,
        objectives_total: 0,
        claim: None,
        session_pane: Some("%5".to_string()),
        stale: false,
        status_mismatch: false,
        next_step: None,
    }];

    app.set_tab(StatusTab::Task);

    app.handle_key(&KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT));

    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0],
        vec!["tmux", "-L", "tandem-test", "select-pane", "-t", "%5"]
    );
}

#[test]
fn test_alt_g_ignores_invalid_pane_id() {
    let tmp = common::TempDir::new();
    let ws = tmp.path().join("ws");
    fs::create_dir_all(&ws).unwrap();

    let (tmux, runner) = create_mock_tmux();
    let mut app = StatusApp::new(ws, CoordStore::new(tmp.path().join("c")), Some(tmux), None);

    app.snapshot.ongoing_tasks = vec![OngoingTask {
        phase: "p-014".to_string(),
        task_id: "t-003".to_string(),
        title: "Sprint tab".to_string(),
        status: "In Progress".to_string(),
        objectives_done: 0,
        objectives_total: 0,
        claim: None,
        session_pane: Some("not-a-pane".to_string()),
        stale: false,
        status_mismatch: false,
        next_step: None,
    }];

    app.set_tab(StatusTab::Task);

    app.handle_key(&KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT));

    let calls = runner.calls();
    assert!(calls.is_empty(), "invalid pane should not call tmux");
}
