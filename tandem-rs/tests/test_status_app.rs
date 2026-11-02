mod common;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

use common::TempDir;
use tandem::config::Config;
use tandem::coordination::schema::Query;
use tandem::coordination::store::CoordStore;
use tandem::notice::{mark_seen, write_notice};
use tandem::status::app::StatusApp;
use tandem::status::notify::{Notifier, order_queries};
use tandem::tmux::client::{Result as TmuxResult, TmuxClient, TmuxRunner};
use tandem::update_check::{CheckState, Runner as UpdateRunner, write_state};

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

fn write_session(root: &Path, sid: &str, pane: &str, task: &str) {
    let sess_dir = root.join("sessions");
    fs::create_dir_all(&sess_dir).unwrap();
    let rec = serde_json::json!({
        "id": sid,
        "cli": "claude",
        "role": "implementer",
        "model": "sonnet",
        "pane": pane,
        "cwd": "/w",
        "branch": "feat",
        "worktree": "/w",
        "task": task,
        "heartbeat": 1_700_000_000.0,
    });
    fs::write(sess_dir.join(format!("{sid}.json")), rec.to_string()).unwrap();
}

fn setup_workspace(root: &Path) -> PathBuf {
    let ws = root.join("ws");
    let phase = ws.join("specs").join("p-001-thing");
    fs::create_dir_all(&phase).unwrap();
    fs::write(phase.join("impl.md"), "- [x] t-001: a\n- [ ] t-002: b\n").unwrap();
    fs::write(phase.join("t-001.md"), "## Status\n- [x] Done\n").unwrap();
    ws
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

fn render_app(app: &mut StatusApp, width: u16, height: u16) -> (Terminal<TestBackend>, String) {
    let backend = TestBackend::new(width, height);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| app.render(f.area(), f.buffer_mut())).unwrap();
    let text = buffer_text(term.backend().buffer());
    (term, text)
}

fn make_config(ws: &Path, interval: u64) -> Config {
    let agent_dir = ws.join(".agent-workspace");
    fs::create_dir_all(&agent_dir).unwrap();
    fs::write(agent_dir.join("manifest.json"), r#"{"version": "1.3.0"}"#).unwrap();

    let mut env = HashMap::new();
    env.insert(
        "TANDEM_WORKSPACE".to_string(),
        ws.to_string_lossy().to_string(),
    );
    env.insert("TANDEM_UPDATE_INTERVAL".to_string(), interval.to_string());
    Config::from_env_source(&env).unwrap()
}

fn fake_runner(version: Option<&str>, calls: Option<Arc<Mutex<Vec<Vec<String>>>>>) -> UpdateRunner {
    let ver = version.map(|s| s.to_string());
    Arc::new(move |argv, _cwd, _timeout| {
        if let Some(ref c) = calls {
            c.lock()
                .unwrap()
                .push(argv.iter().map(|s| s.to_string()).collect());
        }
        if let Some(ref v) = ver {
            Ok((0, v.clone()))
        } else {
            Ok((1, String::new()))
        }
    })
}

#[test]
fn test_shows_summary_phases_and_sessions() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let coord = tmp.path().join("coord");
    write_session(&coord, "implementer-1", "%7", "p-001 t-003");

    let mut app = StatusApp::new(ws, CoordStore::new(coord.clone()), None, None);
    let (_, text) = render_app(&mut app, 120, 40);

    assert!(text.contains("1/2 tasks"));
    assert!(text.contains("1 agent"));
    assert!(text.contains("p-001-thing"));
    assert!(text.contains("t-002"));
    assert_eq!(app.session_items.len(), 1);
}

#[test]
fn test_empty_workspace_shows_helpful_states() {
    let tmp = TempDir::new();
    let ws = tmp.path().join("ws");
    fs::create_dir_all(&ws).unwrap();

    let mut app = StatusApp::new(ws, CoordStore::new(tmp.path().join("none")), None, None);
    let (_, text) = render_app(&mut app, 120, 40);

    assert!(text.contains("No specs yet"));
    assert!(text.contains("Coordination folder not found"));
    assert!(text.contains("workspace · not a git repository"));
}

#[test]
fn test_malformed_spec_does_not_break_the_rest() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    fs::create_dir_all(ws.join("specs").join("p-009-bad")).unwrap();

    let mut app = StatusApp::new(ws, CoordStore::new(tmp.path().join("c")), None, None);
    let (_, text) = render_app(&mut app, 120, 40);

    assert!(text.contains("WARNING"));
    assert!(text.contains("p-001-thing"));
}

#[test]
fn test_refresh_key_picks_up_new_sessions() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let coord = tmp.path().join("coord");

    let mut app = StatusApp::new(ws, CoordStore::new(coord.clone()), None, None);
    assert_eq!(app.session_items.len(), 0);

    write_session(&coord, "planner-1", "%1", "plan");
    app.handle_key(&KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT));
    assert_eq!(app.session_items.len(), 1);
}

#[test]
fn test_goto_focuses_the_agents_pane() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let coord = tmp.path().join("coord");
    write_session(&coord, "implementer-1", "%7", "p-001 t-003");

    let (tmux, runner) = create_mock_tmux();
    let mut app = StatusApp::new(ws, CoordStore::new(coord.clone()), Some(tmux), None);
    render_app(&mut app, 120, 40);

    app.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(&KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT));

    let calls = runner.calls();
    assert!(
        calls
            .iter()
            .any(|c| c.contains(&"select-pane".to_string()) && c.contains(&"%7".to_string()))
    );
}

#[test]
fn test_goto_ignores_invalid_pane_ids() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let coord = tmp.path().join("coord");
    write_session(&coord, "implementer-1", "bad-pane", "p-001 t-003");

    let (tmux, runner) = create_mock_tmux();
    let mut app = StatusApp::new(ws, CoordStore::new(coord.clone()), Some(tmux), None);
    render_app(&mut app, 120, 40);

    app.handle_key(&KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT));
    assert!(runner.calls().is_empty());
}

#[test]
fn test_compact_mode_shows_only_summary() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());

    let mut app = StatusApp::new(ws, CoordStore::new(tmp.path().join("c")), None, None);
    let (_, text) = render_app(&mut app, 100, 6);

    assert!(text.contains("1/2 tasks"));
    assert!(!text.contains("In progress"));
}

#[test]
fn test_escape_sequences_in_spec_titles_never_reach_the_screen() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    fs::write(
        ws.join("specs").join("p-001-thing").join("impl.md"),
        "- [ ] t-001: \x1b[2Jevil [bold]x[/bold]\n",
    )
    .unwrap();

    let mut app = StatusApp::new(ws, CoordStore::new(tmp.path().join("c")), None, None);
    let (_, text) = render_app(&mut app, 120, 40);

    assert!(!text.contains("\x1b"));
}

#[test]
fn test_queries_tab_exists_and_keys_switch() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());

    let mut app = StatusApp::new(ws, CoordStore::new(tmp.path().join("c")), None, None);
    assert_eq!(app.tab_idx, 0);

    app.handle_key(&KeyEvent::new(KeyCode::Char('q'), KeyModifiers::ALT));
    assert_eq!(app.tab_idx, 1);

    app.handle_key(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT));
    assert_eq!(app.tab_idx, 0);

    app.handle_key(&KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.tab_idx, 1);
    app.handle_key(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT));
    assert_eq!(app.tab_idx, 0);
}

#[test]
fn test_markup_in_untrusted_titles_is_shown_literally() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    fs::write(
        ws.join("specs").join("p-001-thing").join("impl.md"),
        "- [ ] t-001: [bold]loud[/bold] [red]x\n",
    )
    .unwrap();

    let mut app = StatusApp::new(ws, CoordStore::new(tmp.path().join("c")), None, None);
    let (_, text) = render_app(&mut app, 120, 40);

    assert!(text.contains("[bold]loud[/bold]"));
}

#[test]
fn test_status_window_autosaves_periodically() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let calls = Arc::new(Mutex::new(0));
    let calls_clone = calls.clone();

    let now_val = Arc::new(Mutex::new(1_700_000_000.0));
    let now_for_app = now_val.clone();

    let mut app = StatusApp::new(ws, CoordStore::new(tmp.path().join("c")), None, None)
        .with_refresh_seconds(60.0)
        .with_autosave(
            Arc::new(move || {
                *calls_clone.lock().unwrap() += 1;
            }),
            0.05,
        )
        .with_now(Arc::new(move || *now_for_app.lock().unwrap()));

    app.tick_timers();
    assert_eq!(*calls.lock().unwrap(), 1);

    *now_val.lock().unwrap() += 0.1;
    app.tick_timers();
    assert_eq!(*calls.lock().unwrap(), 2);
}

#[test]
fn test_update_banner_shown_for_unseen_notice() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    write_notice(&ws, "1.2.0", "1.3.0", Some(1_700_000_000)).unwrap();

    let mut app = StatusApp::new(ws, CoordStore::new(tmp.path().join("coord")), None, None);
    let (_, text) = render_app(&mut app, 120, 40);

    assert!(app.update_banner.is_some());
    assert!(text.contains("Updated to tandem 1.3.0"));
}

#[test]
fn test_update_banner_hidden_when_no_notice() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());

    let app = StatusApp::new(ws, CoordStore::new(tmp.path().join("coord")), None, None);
    assert!(app.update_banner.is_none());
}

#[test]
fn test_update_banner_hidden_when_notice_already_seen() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    write_notice(&ws, "1.2.0", "1.3.0", Some(1_700_000_000)).unwrap();
    mark_seen(&ws).unwrap();

    let app = StatusApp::new(ws, CoordStore::new(tmp.path().join("coord")), None, None);
    assert!(app.update_banner.is_none());
}

#[test]
fn test_update_banner_hidden_in_compact_mode() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    write_notice(&ws, "1.2.0", "1.3.0", Some(1_700_000_000)).unwrap();

    let mut app = StatusApp::new(ws, CoordStore::new(tmp.path().join("coord")), None, None);
    let (_, text) = render_app(&mut app, 100, 6);

    assert!(!text.contains("Updated to tandem"));
}

#[test]
fn test_update_banner_dismiss_marks_seen_and_removes_banner() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    write_notice(&ws, "1.2.0", "1.3.0", Some(1_700_000_000)).unwrap();

    let mut app = StatusApp::new(
        ws.clone(),
        CoordStore::new(tmp.path().join("coord")),
        None,
        None,
    );
    render_app(&mut app, 100, 30);
    assert!(app.update_banner.is_some());

    app.handle_mouse(&MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 98,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    assert!(app.update_banner.is_none());
    assert!(tandem::notice::read_notice(&ws).unwrap().seen);
}

#[test]
fn test_update_banner_click_text_triggers_tmux_popup() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    write_notice(&ws, "1.2.0", "1.3.0", Some(1_700_000_000)).unwrap();

    let (tmux, runner) = create_mock_tmux();
    let mut app = StatusApp::new(
        ws,
        CoordStore::new(tmp.path().join("coord")),
        Some(tmux),
        None,
    );
    render_app(&mut app, 100, 30);

    app.handle_mouse(&MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 10,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });

    let calls = runner.calls();
    assert!(
        calls
            .iter()
            .any(|c| c.contains(&"display-popup".to_string()))
    );
}

#[test]
fn test_footer_present_in_compact_and_full() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());

    let mut app = StatusApp::new(ws, CoordStore::new(tmp.path().join("c")), None, None);
    let (_, full_text) = render_app(&mut app, 120, 40);
    assert!(full_text.contains("Refresh"));
    assert!(full_text.contains("Switch tabs"));

    let (_, compact_text) = render_app(&mut app, 120, 6);
    assert!(compact_text.contains("Refresh"));
}

#[test]
fn test_session_row_enter_goes_to_agent() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let coord = tmp.path().join("coord");
    write_session(&coord, "implementer-1", "%7", "p-001 t-003");

    let (tmux, runner) = create_mock_tmux();
    let mut app = StatusApp::new(ws, CoordStore::new(coord.clone()), Some(tmux), None);
    render_app(&mut app, 120, 40);

    app.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let calls = runner.calls();
    assert!(
        calls
            .iter()
            .any(|c| c.contains(&"select-pane".to_string()) && c.contains(&"%7".to_string()))
    );
}

#[test]
fn test_session_row_click_goes_to_agent() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let coord = tmp.path().join("coord");
    write_session(&coord, "implementer-1", "%7", "p-001 t-003");

    let (tmux, runner) = create_mock_tmux();
    let mut app = StatusApp::new(ws, CoordStore::new(coord.clone()), Some(tmux), None);
    render_app(&mut app, 120, 40);

    app.handle_mouse(&MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 5,
        row: app.sessions_area.y,
        modifiers: KeyModifiers::NONE,
    });

    let calls = runner.calls();
    assert!(
        calls
            .iter()
            .any(|c| c.contains(&"select-pane".to_string()) && c.contains(&"%7".to_string()))
    );
}

#[test]
fn test_offer_banner_shown_when_newer_version_available() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let cfg = make_config(&ws, 600);

    let mut app = StatusApp::new(
        ws,
        CoordStore::new(tmp.path().join("coord")),
        None,
        Some(cfg),
    )
    .with_update_runner(fake_runner(Some("1.4.0"), None))
    .with_now(Arc::new(|| 1_700_000_000.0));

    app.maybe_check_update();
    assert!(app.offer_banner.is_some());

    let (_, text) = render_app(&mut app, 120, 40);
    assert!(text.contains("1.3.0"));
    assert!(text.contains("1.4.0"));
}

#[test]
fn test_offer_banner_not_shown_when_up_to_date() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let cfg = make_config(&ws, 600);

    let mut app = StatusApp::new(
        ws,
        CoordStore::new(tmp.path().join("coord")),
        None,
        Some(cfg),
    )
    .with_update_runner(fake_runner(Some("1.3.0"), None))
    .with_now(Arc::new(|| 1_700_000_000.0));

    app.maybe_check_update();
    assert!(app.offer_banner.is_none());
}

#[test]
fn test_offer_banner_not_shown_when_check_fails() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let cfg = make_config(&ws, 600);

    let mut app = StatusApp::new(
        ws,
        CoordStore::new(tmp.path().join("coord")),
        None,
        Some(cfg),
    )
    .with_update_runner(fake_runner(None, None))
    .with_now(Arc::new(|| 1_700_000_000.0));

    app.maybe_check_update();
    assert!(app.offer_banner.is_none());
}

#[test]
fn test_offer_banner_later_hides_it() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let cfg = make_config(&ws, 600);

    let mut app = StatusApp::new(
        ws,
        CoordStore::new(tmp.path().join("coord")),
        None,
        Some(cfg),
    )
    .with_update_runner(fake_runner(Some("1.4.0"), None))
    .with_now(Arc::new(|| 1_700_000_000.0));

    app.maybe_check_update();
    assert!(app.offer_banner.is_some());
    render_app(&mut app, 120, 40);

    // Click [Later] button
    let buttons_width = 12 + 2 + 7 + 2 + 19;
    let start_x = 120 - buttons_width;
    let later_x = start_x + 14 + 2;

    app.handle_mouse(&MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: later_x,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    assert!(app.offer_banner.is_none());
}

#[test]
fn test_offer_banner_skip_writes_skip_version() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let cfg = make_config(&ws, 600);

    let mut app = StatusApp::new(
        ws.clone(),
        CoordStore::new(tmp.path().join("coord")),
        None,
        Some(cfg),
    )
    .with_update_runner(fake_runner(Some("1.4.0"), None))
    .with_now(Arc::new(|| 1_700_000_000.0));

    app.maybe_check_update();
    assert!(app.offer_banner.is_some());
    render_app(&mut app, 120, 40);

    let buttons_width = 12 + 2 + 7 + 2 + 19;
    let start_x = 120 - buttons_width;
    let skip_x = start_x + 12 + 2 + 7 + 2 + 2;

    app.handle_mouse(&MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: skip_x,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    assert!(app.offer_banner.is_none());

    let state = tandem::update_check::read_state(&ws);
    assert_eq!(state.skip, "1.4.0");
}

#[test]
fn test_offer_banner_update_now_triggers_popup() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let cfg = make_config(&ws, 600);
    let (tmux, runner) = create_mock_tmux();

    let mut app = StatusApp::new(
        ws,
        CoordStore::new(tmp.path().join("coord")),
        Some(tmux),
        Some(cfg),
    )
    .with_update_runner(fake_runner(Some("1.4.0"), None))
    .with_now(Arc::new(|| 1_700_000_000.0));

    app.maybe_check_update();
    render_app(&mut app, 120, 40);

    let buttons_width = 12 + 2 + 7 + 2 + 19;
    let start_x = 120 - buttons_width;

    app.handle_mouse(&MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: start_x + 1,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });

    let calls = runner.calls();
    assert!(
        calls
            .iter()
            .any(|c| c.contains(&"display-popup".to_string())
                && c.last().map(|s| s.contains("--yes")).unwrap_or(false))
    );
}

#[test]
fn test_offer_banner_update_now_key_triggers_popup() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let cfg = make_config(&ws, 600);
    let (tmux, runner) = create_mock_tmux();

    let mut app = StatusApp::new(
        ws,
        CoordStore::new(tmp.path().join("coord")),
        Some(tmux),
        Some(cfg),
    )
    .with_update_runner(fake_runner(Some("1.4.0"), None))
    .with_now(Arc::new(|| 1_700_000_000.0));

    app.maybe_check_update();
    render_app(&mut app, 120, 40);

    app.handle_key(&KeyEvent::new(KeyCode::Char('u'), KeyModifiers::ALT));

    let calls = runner.calls();
    assert!(
        calls
            .iter()
            .any(|c| c.contains(&"display-popup".to_string()))
    );
}

#[test]
fn test_offer_banner_hidden_in_compact_mode() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let cfg = make_config(&ws, 600);

    let mut app = StatusApp::new(
        ws,
        CoordStore::new(tmp.path().join("coord")),
        None,
        Some(cfg),
    )
    .with_update_runner(fake_runner(Some("1.4.0"), None))
    .with_now(Arc::new(|| 1_700_000_000.0));

    app.maybe_check_update();
    assert!(app.offer_banner.is_some());

    let (_, text) = render_app(&mut app, 100, 6);
    assert!(!text.contains("available"));
}

#[test]
fn test_offer_banner_not_shown_when_disabled() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let mut env = HashMap::new();
    env.insert(
        "TANDEM_WORKSPACE".to_string(),
        ws.to_string_lossy().to_string(),
    );
    env.insert("TANDEM_UPDATE_CHECK".to_string(), "0".to_string());
    let cfg = Config::from_env_source(&env).unwrap();

    let mut app = StatusApp::new(
        ws,
        CoordStore::new(tmp.path().join("coord")),
        None,
        Some(cfg),
    )
    .with_update_runner(fake_runner(Some("1.4.0"), None))
    .with_now(Arc::new(|| 1_700_000_000.0));

    app.maybe_check_update();
    assert!(app.offer_banner.is_none());
}

#[test]
fn test_offer_banner_not_shown_for_skipped_version() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let cfg = make_config(&ws, 600);
    write_state(
        &ws,
        &CheckState {
            last: 0.0,
            latest: "1.4.0".to_string(),
            skip: "1.4.0".to_string(),
        },
    )
    .unwrap();

    let mut app = StatusApp::new(
        ws,
        CoordStore::new(tmp.path().join("coord")),
        None,
        Some(cfg),
    )
    .with_update_runner(fake_runner(Some("1.4.0"), None))
    .with_now(Arc::new(|| 1_700_000_000.0));

    app.maybe_check_update();
    assert!(app.offer_banner.is_none());
}

#[test]
fn test_offer_banner_not_due_when_recently_checked() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let cfg = make_config(&ws, 3600);
    let now = 1_700_000_000.0;
    write_state(
        &ws,
        &CheckState {
            last: now - 100.0,
            latest: String::new(),
            skip: String::new(),
        },
    )
    .unwrap();

    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut app = StatusApp::new(
        ws,
        CoordStore::new(tmp.path().join("coord")),
        None,
        Some(cfg),
    )
    .with_update_runner(fake_runner(Some("1.4.0"), Some(calls.clone())))
    .with_now(Arc::new(move || now));

    app.maybe_check_update();
    assert!(calls.lock().unwrap().is_empty());
    assert!(app.offer_banner.is_none());
}

#[test]
fn test_offer_banner_tmux_message_sent_once() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let cfg = make_config(&ws, 600);
    let (tmux, runner) = create_mock_tmux();

    let mut app = StatusApp::new(
        ws,
        CoordStore::new(tmp.path().join("coord")),
        Some(tmux),
        Some(cfg),
    )
    .with_update_runner(fake_runner(Some("1.4.0"), None))
    .with_now(Arc::new(|| 1_700_000_000.0));

    app.maybe_check_update();
    let msgs: Vec<_> = runner
        .calls()
        .into_iter()
        .filter(|c| c.contains(&"display-message".to_string()))
        .collect();
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].iter().any(|s| s.contains("1.4.0")));

    app.maybe_check_update();
    let msgs2: Vec<_> = runner
        .calls()
        .into_iter()
        .filter(|c| c.contains(&"display-message".to_string()))
        .collect();
    assert_eq!(msgs2.len(), 1);
}

#[test]
fn test_offer_update_now_key_no_op_without_offer() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let cfg = make_config(&ws, 600);
    let (tmux, runner) = create_mock_tmux();

    let mut app = StatusApp::new(
        ws,
        CoordStore::new(tmp.path().join("coord")),
        Some(tmux),
        Some(cfg),
    )
    .with_update_runner(fake_runner(Some("1.3.0"), None))
    .with_now(Arc::new(|| 1_700_000_000.0));

    app.maybe_check_update();
    assert!(app.offer_banner.is_none());

    let calls_before = runner.calls().len();
    app.handle_key(&KeyEvent::new(KeyCode::Char('u'), KeyModifiers::ALT));
    assert_eq!(runner.calls().len(), calls_before);
}

#[test]
fn test_update_now_footer_binding_visibility() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());
    let cfg = make_config(&ws, 600);

    let mut app = StatusApp::new(
        ws,
        CoordStore::new(tmp.path().join("coord")),
        None,
        Some(cfg),
    )
    .with_update_runner(fake_runner(Some("1.4.0"), None))
    .with_now(Arc::new(|| 1_700_000_000.0));

    let (_, text_before) = render_app(&mut app, 120, 40);
    assert!(!text_before.contains("Update now"));
    assert!(!app.check_action("update_now"));

    app.maybe_check_update();
    let (_, text_after) = render_app(&mut app, 120, 40);
    assert!(text_after.contains("Update now"));
    assert!(app.check_action("update_now"));
}

#[test]
fn test_collapsible_toggle_clicks() {
    let tmp = TempDir::new();
    let ws = setup_workspace(tmp.path());

    let mut app = StatusApp::new(ws, CoordStore::new(tmp.path().join("c")), None, None);
    render_app(&mut app, 120, 40);

    assert!(app.collapsible_repos.expanded);
    app.handle_mouse(&MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row: app.repos_header_y,
        modifiers: KeyModifiers::NONE,
    });
    assert!(!app.collapsible_repos.expanded);

    app.handle_mouse(&MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 2,
        row: app.repos_header_y,
        modifiers: KeyModifiers::NONE,
    });
    assert!(app.collapsible_repos.expanded);
}

fn make_query(qid: &str, status: &str, created: f64) -> Query {
    Query {
        id: qid.to_string(),
        status: status.to_string(),
        asked_by: "a".to_string(),
        task: "t".to_string(),
        blocking_scope: String::new(),
        created,
        question: "Q".to_string(),
        options: Vec::new(),
        answer: String::new(),
        answered_via: String::new(),
    }
}

#[test]
fn test_order_queries_groups() {
    let queries = vec![
        make_query("q-a", "acknowledged", 1.0),
        make_query("q-b", "open", 5.0),
        make_query("q-c", "answered", 3.0),
        make_query("q-d", "open", 2.0),
        make_query("q-e", "withdrawn", 9.0),
    ];
    let groups = order_queries(&queries);
    let ids: Vec<(&str, Vec<&str>)> = groups
        .iter()
        .map(|(name, list)| (*name, list.iter().map(|q| q.id.as_str()).collect()))
        .collect();

    assert_eq!(
        ids,
        vec![
            ("Open", vec!["q-d", "q-b"]),
            ("Answered (awaiting pickup)", vec!["q-c"]),
            ("Acknowledged (last 20)", vec!["q-a"]),
        ]
    );
}

#[test]
fn test_acknowledged_capped_at_20_newest_first() {
    let queries: Vec<_> = (0..30)
        .map(|i| make_query(&format!("q-{i:04}"), "acknowledged", i as f64))
        .collect();
    let groups = order_queries(&queries);
    let acked = &groups[0].1;
    assert_eq!(acked.len(), 20);
    assert_eq!(acked[0].id, "q-0029");
}

#[test]
fn test_notifier_seeds_then_reports_new_open_queries() {
    let mut n = Notifier::new(None);
    assert_eq!(
        n.new_queries(&[make_query("q-1", "open", 1.0)]),
        Vec::<Query>::new()
    );
    let fresh = n.new_queries(&[
        make_query("q-1", "open", 1.0),
        make_query("q-2", "open", 2.0),
        make_query("q-3", "answered", 3.0),
    ]);
    assert_eq!(fresh.len(), 1);
    assert_eq!(fresh[0].id, "q-2");
    assert_eq!(
        n.new_queries(&[
            make_query("q-1", "open", 1.0),
            make_query("q-2", "open", 2.0),
        ]),
        Vec::<Query>::new()
    );
}

#[test]
fn test_notifier_uses_non_focus_stealing_message_and_escapes() {
    let (tmux, runner) = create_mock_tmux();
    let mut n = Notifier::new(Some(tmux));
    n.notify(&[]);

    let mut q = make_query("q-2", "open", 1.0);
    q.asked_by = "impl#1".to_string();
    q.question = "ok #{evil}\x1b[2J".to_string();

    n.notify(&[q]);

    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    let msg_call = &calls[0];
    assert!(msg_call.iter().any(|s| s == "display-message"));
    assert!(msg_call.iter().any(|s| s == "4000"));
    let text = msg_call.last().unwrap();
    assert!(text.contains("impl##1"));
    assert!(!text.contains("#{evil}"));
    assert!(!text.contains("\x1b"));
}
