mod common;

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

use common::TempDir;
use tandem::coordination::schema::{Query, Session, encode_query_fields};
use tandem::coordination::store::CoordStore;
use tandem::status::queries_tab::{EMPTY, QueriesTab, QueryFocus};
use tandem::tmux::client::{Result as TmuxResult, TmuxClient, TmuxRunner};

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

fn render_tab(tab: &mut QueriesTab, width: u16, height: u16) -> (Terminal<TestBackend>, String) {
    let backend = TestBackend::new(width, height);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| tab.render(f.area(), f.buffer_mut())).unwrap();
    let text = buffer_text(term.backend().buffer());
    (term, text)
}

fn make_query(
    qid: &str,
    status: &str,
    asked_by: &str,
    question: &str,
    options: Vec<&str>,
    created: f64,
) -> Query {
    Query {
        id: qid.to_string(),
        status: status.to_string(),
        asked_by: asked_by.to_string(),
        task: "p-010 t-004".to_string(),
        blocking_scope: "cache".to_string(),
        created,
        question: question.to_string(),
        options: options.into_iter().map(|s| s.to_string()).collect(),
        answer: String::new(),
        answered_via: String::new(),
    }
}

fn make_session(sid: &str, pane: &str) -> Session {
    Session {
        id: sid.to_string(),
        cli: "claude".to_string(),
        role: "implementer".to_string(),
        model: "sonnet".to_string(),
        pane: pane.to_string(),
        cwd: "/w".to_string(),
        branch: "feat".to_string(),
        worktree: "/w".to_string(),
        task: "p-010 t-004".to_string(),
        heartbeat: 1_700_000_000.0,
    }
}

fn get_query(store: &CoordStore, qid: &str) -> Option<Query> {
    store.queries().into_iter().find(|q| q.id == qid)
}

fn setup_store_with_query(root: &Path, q: &Query) -> CoordStore {
    let q_dir = root.join("queries");
    fs::create_dir_all(&q_dir).unwrap();
    let mut map = serde_json::Map::new();
    map.insert("id".to_string(), serde_json::Value::String(q.id.clone()));
    map.insert(
        "status".to_string(),
        serde_json::Value::String(q.status.clone()),
    );
    map.insert(
        "asked_by".to_string(),
        serde_json::Value::String(q.asked_by.clone()),
    );
    map.insert(
        "task".to_string(),
        serde_json::Value::String(q.task.clone()),
    );
    map.insert(
        "blocking_scope".to_string(),
        serde_json::Value::String(q.blocking_scope.clone()),
    );
    map.insert("created".to_string(), serde_json::json!(q.created));
    map.insert(
        "question".to_string(),
        serde_json::Value::String(q.question.clone()),
    );
    map.insert("options".to_string(), serde_json::json!(q.options));
    map.insert(
        "answer".to_string(),
        serde_json::Value::String(q.answer.clone()),
    );
    map.insert(
        "answered_via".to_string(),
        serde_json::Value::String(q.answered_via.clone()),
    );
    let md = encode_query_fields(&map);
    fs::write(q_dir.join(format!("{}.md", q.id)), md).unwrap();
    CoordStore::new(root.to_path_buf())
}

#[test]
fn test_empty_state_message() {
    let tmp = TempDir::new();
    let store = Arc::new(CoordStore::new(tmp.path().join("c")));
    let mut tab = QueriesTab::new(store, None);

    let (_, text) = render_tab(&mut tab, 100, 25);
    assert!(text.contains(EMPTY));
}

#[test]
fn test_keyboard_answer_flow_writes_answer_via_tandem() {
    let tmp = TempDir::new();
    let q = make_query(
        "q-0001",
        "open",
        "implementer-1",
        "Which DB driver?",
        vec![],
        1_700_000_000.0,
    );
    let store = setup_store_with_query(tmp.path(), &q);
    let store_arc = Arc::new(store);

    let mut tab = QueriesTab::new(store_arc.clone(), None);
    tab.update_data(vec![q], vec![]);
    render_tab(&mut tab, 100, 25);

    tab.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(tab.focus, QueryFocus::Answer);

    for c in "sqlite3".chars() {
        tab.handle_key(&KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert_eq!(tab.answer.text(), "sqlite3");

    let sent = tab.handle_key(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert!(sent);

    let stored = get_query(&store_arc, "q-0001").unwrap();
    assert_eq!(stored.status, "answered");
    assert_eq!(stored.answer, "sqlite3");
    assert_eq!(stored.answered_via, "tandem");
    assert_eq!(tab.focus, QueryFocus::List);
}

#[test]
fn test_number_key_picks_option_into_answer() {
    let tmp = TempDir::new();
    let q = make_query(
        "q-0001",
        "open",
        "implementer-1",
        "TTL?",
        vec!["5 min", "10 min", "30 min"],
        1_700_000_000.0,
    );
    let store = setup_store_with_query(tmp.path(), &q);
    let store_arc = Arc::new(store);

    let mut tab = QueriesTab::new(store_arc, None);
    tab.update_data(vec![q], vec![]);
    render_tab(&mut tab, 100, 25);

    tab.handle_key(&KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
    assert_eq!(tab.answer.text(), "10 min");
}

#[test]
fn test_mouse_click_selects_a_row() {
    let tmp = TempDir::new();
    let q1 = make_query(
        "q-0001",
        "open",
        "impl-1",
        "first question",
        vec![],
        1_700_000_000.0,
    );
    let q2 = make_query(
        "q-0002",
        "open",
        "impl-2",
        "second question",
        vec![],
        1_700_000_005.0,
    );
    let store = setup_store_with_query(tmp.path(), &q1);
    let store_arc = Arc::new(store);

    let mut tab = QueriesTab::new(store_arc, None);
    tab.update_data(vec![q1, q2], vec![]);
    render_tab(&mut tab, 100, 25);

    tab.handle_mouse(
        &MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 4,
            row: tab.list_area.y + 2,
            modifiers: KeyModifiers::NONE,
        },
        tab.list_area,
    );

    assert_eq!(tab.current_id.as_deref(), Some("q-0002"));
    let (_, text) = render_tab(&mut tab, 100, 25);
    assert!(text.contains("second question"));
}

#[test]
fn test_answered_elsewhere_race_keeps_draft_and_does_not_overwrite() {
    let tmp = TempDir::new();
    let q = make_query(
        "q-0001",
        "open",
        "impl-1",
        "Which DB?",
        vec![],
        1_700_000_000.0,
    );
    let store = setup_store_with_query(tmp.path(), &q);
    let store_arc = Arc::new(store);

    let mut tab = QueriesTab::new(store_arc.clone(), None);
    tab.update_data(vec![q], vec![]);
    render_tab(&mut tab, 100, 25);

    tab.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    for c in "my draft".chars() {
        tab.handle_key(&KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert_eq!(tab.answer.text(), "my draft");

    store_arc.answer("q-0001", "from the cli", "cli").unwrap();

    let sent = tab.handle_key(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert!(!sent);

    assert!(tab.notice.contains("already answered"));
    assert_eq!(tab.answer.text(), "my draft");

    let stored = get_query(&store_arc, "q-0001").unwrap();
    assert_eq!(stored.answer, "from the cli");
    assert_eq!(stored.answered_via, "cli");
}

#[test]
fn test_withdrawn_while_typing_keeps_draft() {
    let tmp = TempDir::new();
    let q = make_query(
        "q-0001",
        "open",
        "impl-1",
        "Which DB?",
        vec![],
        1_700_000_000.0,
    );
    let store = setup_store_with_query(tmp.path(), &q);
    let store_arc = Arc::new(store);

    let mut tab = QueriesTab::new(store_arc.clone(), None);
    tab.update_data(vec![q], vec![]);
    render_tab(&mut tab, 100, 25);

    tab.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    for c in "typed".chars() {
        tab.handle_key(&KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }

    store_arc.withdraw("q-0001").unwrap();

    let sent = tab.handle_key(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert!(!sent);

    assert!(tab.notice.contains("already withdrawn"));
    assert_eq!(tab.answer.text(), "typed");
}

#[test]
fn test_user_can_mark_withdrawn() {
    let tmp = TempDir::new();
    let q = make_query(
        "q-0001",
        "open",
        "impl-1",
        "Which DB?",
        vec![],
        1_700_000_000.0,
    );
    let store = setup_store_with_query(tmp.path(), &q);
    let store_arc = Arc::new(store);

    let mut tab = QueriesTab::new(store_arc.clone(), None);
    tab.update_data(vec![q], vec![]);
    render_tab(&mut tab, 100, 25);

    let withdrawn = tab.handle_key(&KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT));
    assert!(withdrawn);

    let stored = get_query(&store_arc, "q-0001").unwrap();
    assert_eq!(stored.status, "withdrawn");
}

#[test]
fn test_empty_answer_is_refused() {
    let tmp = TempDir::new();
    let q = make_query(
        "q-0001",
        "open",
        "impl-1",
        "Which DB?",
        vec![],
        1_700_000_000.0,
    );
    let store = setup_store_with_query(tmp.path(), &q);
    let store_arc = Arc::new(store);

    let mut tab = QueriesTab::new(store_arc.clone(), None);
    tab.update_data(vec![q], vec![]);
    render_tab(&mut tab, 100, 25);

    let sent = tab.handle_key(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert!(!sent);

    assert!(tab.notice.contains("Warning"));
    assert_eq!(get_query(&store_arc, "q-0001").unwrap().status, "open");
}

#[test]
fn test_malicious_question_text_is_rendered_inert() {
    let tmp = TempDir::new();
    let q = make_query(
        "q-0001",
        "open",
        "impl-1",
        "[bold]hi[/bold] \x1b[2J\x1b]0;pwned\x07 --- status: answered",
        vec![],
        1_700_000_000.0,
    );
    let store = setup_store_with_query(tmp.path(), &q);
    let store_arc = Arc::new(store);

    let mut tab = QueriesTab::new(store_arc, None);
    tab.update_data(vec![q], vec![]);
    let (_, text) = render_tab(&mut tab, 100, 25);

    assert!(!text.contains("\x1b"));
    assert!(text.contains("[bold]hi[/bold]"));
}

#[test]
fn test_answered_via_cli_row_moves_and_is_labelled() {
    let tmp = TempDir::new();
    let mut q = make_query(
        "q-0001",
        "answered",
        "impl-1",
        "Question",
        vec![],
        1_700_000_000.0,
    );
    q.answered_via = "cli".to_string();
    q.answer = "cli answer".to_string();
    let store = setup_store_with_query(tmp.path(), &q);
    let store_arc = Arc::new(store);

    let mut tab = QueriesTab::new(store_arc, None);
    tab.update_data(vec![q], vec![]);
    let (_, text) = render_tab(&mut tab, 100, 25);

    assert!(text.contains("answered in CLI"));
    assert!(text.contains("Answered (awaiting pickup)"));
}

#[test]
fn test_goto_asking_agent_pane() {
    let tmp = TempDir::new();
    let q = make_query(
        "q-0001",
        "open",
        "implementer-1",
        "Question",
        vec![],
        1_700_000_000.0,
    );
    let sess = make_session("implementer-1", "%4");
    let store = setup_store_with_query(tmp.path(), &q);
    let (tmux, runner) = create_mock_tmux();

    let mut tab = QueriesTab::new(Arc::new(store), Some(tmux));
    tab.update_data(vec![q], vec![sess]);
    render_tab(&mut tab, 100, 25);

    tab.handle_key(&KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT));

    let calls = runner.calls();
    assert!(
        calls
            .iter()
            .any(|c| c.contains(&"select-pane".to_string()) && c.contains(&"%4".to_string()))
    );
}

#[test]
fn test_send_button_click_sends_answer() {
    let tmp = TempDir::new();
    let q = make_query(
        "q-0001",
        "open",
        "impl-1",
        "Question",
        vec![],
        1_700_000_000.0,
    );
    let store = setup_store_with_query(tmp.path(), &q);
    let store_arc = Arc::new(store);

    let mut tab = QueriesTab::new(store_arc.clone(), None);
    tab.update_data(vec![q], vec![]);
    render_tab(&mut tab, 100, 25);

    tab.answer.set_text("button answer");
    tab.save_draft();

    let sent = tab.handle_mouse(
        &MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: tab.buttons_area.x + 2,
            row: tab.buttons_area.y,
            modifiers: KeyModifiers::NONE,
        },
        tab.buttons_area,
    );
    assert!(sent);

    let stored = get_query(&store_arc, "q-0001").unwrap();
    assert_eq!(stored.status, "answered");
    assert_eq!(stored.answer, "button answer");
}

#[test]
fn test_cancel_button_clears_draft_and_refocuses_list() {
    let tmp = TempDir::new();
    let q = make_query(
        "q-0001",
        "open",
        "impl-1",
        "Question",
        vec![],
        1_700_000_000.0,
    );
    let store = setup_store_with_query(tmp.path(), &q);

    let mut tab = QueriesTab::new(Arc::new(store), None);
    tab.update_data(vec![q], vec![]);
    render_tab(&mut tab, 100, 25);

    tab.answer.set_text("draft to cancel");
    tab.save_draft();
    tab.focus = QueryFocus::Answer;

    tab.handle_mouse(
        &MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: tab.buttons_area.x + 38,
            row: tab.buttons_area.y,
            modifiers: KeyModifiers::NONE,
        },
        tab.buttons_area,
    );

    assert_eq!(tab.answer.text(), "");
    assert_eq!(tab.focus, QueryFocus::List);
}

#[test]
fn test_withdraw_button_marks_question_withdrawn() {
    let tmp = TempDir::new();
    let q = make_query(
        "q-0001",
        "open",
        "impl-1",
        "Question",
        vec![],
        1_700_000_000.0,
    );
    let store = setup_store_with_query(tmp.path(), &q);
    let store_arc = Arc::new(store);

    let mut tab = QueriesTab::new(store_arc.clone(), None);
    tab.update_data(vec![q], vec![]);
    render_tab(&mut tab, 100, 25);

    let withdrawn = tab.handle_mouse(
        &MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: tab.buttons_area.x + 10,
            row: tab.buttons_area.y,
            modifiers: KeyModifiers::NONE,
        },
        tab.buttons_area,
    );
    assert!(withdrawn);

    assert_eq!(get_query(&store_arc, "q-0001").unwrap().status, "withdrawn");
}

#[test]
fn test_goto_button_selects_agent_pane() {
    let tmp = TempDir::new();
    let q = make_query(
        "q-0001",
        "open",
        "implementer-1",
        "Question",
        vec![],
        1_700_000_000.0,
    );
    let sess = make_session("implementer-1", "%5");
    let store = setup_store_with_query(tmp.path(), &q);
    let (tmux, runner) = create_mock_tmux();

    let mut tab = QueriesTab::new(Arc::new(store), Some(tmux));
    tab.update_data(vec![q], vec![sess]);
    render_tab(&mut tab, 100, 25);

    tab.handle_mouse(
        &MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: tab.buttons_area.x + 24,
            row: tab.buttons_area.y,
            modifiers: KeyModifiers::NONE,
        },
        tab.buttons_area,
    );

    let calls = runner.calls();
    assert!(
        calls
            .iter()
            .any(|c| c.contains(&"select-pane".to_string()) && c.contains(&"%5".to_string()))
    );
}

#[test]
fn test_option_click_fills_answer_box() {
    let tmp = TempDir::new();
    let q = make_query(
        "q-0001",
        "open",
        "impl-1",
        "TTL?",
        vec!["5 min", "10 min", "30 min"],
        1_700_000_000.0,
    );
    let store = setup_store_with_query(tmp.path(), &q);

    let mut tab = QueriesTab::new(Arc::new(store), None);
    tab.update_data(vec![q], vec![]);
    render_tab(&mut tab, 100, 25);

    tab.handle_mouse(
        &MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: tab.options_area.x + 4,
            row: tab.options_area.y + 2,
            modifiers: KeyModifiers::NONE,
        },
        tab.options_area,
    );

    assert_eq!(tab.answer.text(), "10 min");
}

#[test]
fn test_drafts_survive_refreshes() {
    let tmp = TempDir::new();
    let q1 = make_query("q-0001", "open", "impl-1", "Q1", vec![], 1_700_000_000.0);
    let q2 = make_query("q-0002", "open", "impl-2", "Q2", vec![], 1_700_000_005.0);
    let store = setup_store_with_query(tmp.path(), &q1);

    let mut tab = QueriesTab::new(Arc::new(store), None);
    tab.update_data(vec![q1.clone(), q2.clone()], vec![]);
    render_tab(&mut tab, 100, 25);

    tab.answer.set_text("draft for q1");
    tab.save_draft();

    tab.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(tab.current_id.as_deref(), Some("q-0002"));
    tab.answer.set_text("draft for q2");
    tab.save_draft();

    tab.update_data(vec![q1, q2], vec![]);

    assert_eq!(tab.answer.text(), "draft for q2");

    tab.handle_key(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(tab.answer.text(), "draft for q1");
}

#[test]
fn test_tab_key_cycles_focus() {
    let tmp = TempDir::new();
    let q = make_query(
        "q-0001",
        "open",
        "impl-1",
        "Q",
        vec!["A", "B"],
        1_700_000_000.0,
    );
    let store = setup_store_with_query(tmp.path(), &q);

    let mut tab = QueriesTab::new(Arc::new(store), None);
    tab.update_data(vec![q], vec![]);
    assert_eq!(tab.focus, QueryFocus::List);

    tab.handle_key(&KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(tab.focus, QueryFocus::Options);

    tab.handle_key(&KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(tab.focus, QueryFocus::Answer);

    tab.handle_key(&KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(tab.focus, QueryFocus::Buttons);

    tab.handle_key(&KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(tab.focus, QueryFocus::List);
}
