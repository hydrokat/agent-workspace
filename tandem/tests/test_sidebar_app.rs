mod common;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use std::sync::{Arc, Mutex};

use common::TempDir;
use tandem::coordination::store::CoordStore;
use tandem::sidebar::app::SidebarApp;
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
        let joined = argv.join(" ");
        if joined.contains("list-panes") {
            let p1 = "%1\tmain\tagent\tplanner\t1\tclaude·opus\t0\t\t\t\t";
            let p2 = "%2\tstash-implementer-1\tagent\timplementer\t1\tclaude·sonnet\t0\t\t\t\t";
            return TmuxResult {
                code: 0,
                out: format!("{p1}\n{p2}\n"),
                err: String::new(),
            };
        }
        TmuxResult {
            code: 0,
            out: String::new(),
            err: String::new(),
        }
    }
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
fn test_sidebar_app_key_and_mouse_lifecycle() {
    let temp = TempDir::new();
    let store = CoordStore::new(temp.path().join("coord"));
    let runner = Arc::new(MockTmuxRunner::new());
    let tmux = TmuxClient::with_runner("tandem-test", None, runner.clone());

    let mut app = SidebarApp::new(temp.path().to_path_buf(), store, Some(tmux));

    // Rows should have: PLANNERS header (0), %1 planner (1), IMPLEMENTERS header (2), %2 impl (3), AddAgent (4)
    assert_eq!(app.rows.len(), 5);
    assert_eq!(app.selected, 1);
    assert!(app.dirty);

    // Render test
    let backend = TestBackend::new(26, 10);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| app.render(f.area(), f.buffer_mut())).unwrap();
    let text = buffer_text(term.backend().buffer());

    assert!(text.contains("tandem"));
    assert!(text.contains("PLANNERS"));
    assert!(text.contains("claude·opus"));
    assert!(text.contains("IMPLEMENTERS"));
    assert!(text.contains("+ Add agent"));

    // Key navigation
    app.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.selected, 3); // skipped header 2

    app.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.selected, 4); // AddAgent

    app.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(app.brand_focused);

    app.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(!app.brand_focused);
    assert_eq!(app.selected, 1);

    app.handle_key(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(app.brand_focused);

    app.handle_key(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(!app.brand_focused);
    assert_eq!(app.selected, 4);

    // Enter activates
    app.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let calls = runner.calls();
    assert!(calls.iter().any(|c| c.join(" ").contains("add-agent")));

    // Mouse click selects and activates
    app.handle_mouse(
        &MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: 2, // Row 0 is brand, row 1 is PLANNERS header, row 2 is planner-1 (idx 1)
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 26, 10),
    );
    assert_eq!(app.selected, 1);
    let calls_after = runner.calls();
    assert!(calls_after.iter().any(|c| c.join(" ").contains("select-pane -t %1") || c.join(" ").contains("swap-pane")));
}
