use std::io;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

use tandem::coordination::store::CoordStore;
use tandem::sidebar::app::SidebarApp;
use tandem::tmux::client::{Result as TmuxResult, TmuxClient, TmuxRunner};
use tandem::ui::link::{LinkError, REPO_URL, UrlSpawner, open_url, osc8};

struct MockUrlSpawner {
    calls: Mutex<Vec<(String, Vec<String>)>>,
    should_fail: bool,
}

impl MockUrlSpawner {
    fn new(should_fail: bool) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            should_fail,
        }
    }

    fn calls(&self) -> Vec<(String, Vec<String>)> {
        self.calls.lock().unwrap().clone()
    }
}

impl UrlSpawner for MockUrlSpawner {
    fn spawn(&self, program: &str, args: &[&str]) -> io::Result<()> {
        self.calls.lock().unwrap().push((
            program.to_string(),
            args.iter().map(|s| s.to_string()).collect(),
        ));
        if self.should_fail {
            Err(io::Error::new(io::ErrorKind::NotFound, "command not found"))
        } else {
            Ok(())
        }
    }
}

struct DummyTmuxRunner;

impl TmuxRunner for DummyTmuxRunner {
    fn run(&self, argv: &[String]) -> TmuxResult {
        let joined = argv.join(" ");
        if joined.contains("list-panes") {
            let p1 = "%1\tmain\tagent\tplanner\t1\tclaude·opus\t0\t\t\t\t";
            TmuxResult {
                code: 0,
                out: format!("{p1}\n"),
                err: String::new(),
            }
        } else {
            TmuxResult {
                code: 0,
                out: String::new(),
                err: String::new(),
            }
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
fn test_repo_url_constant() {
    assert_eq!(REPO_URL, "https://github.com/hydrokat/agent-workspace");
}

#[test]
fn test_open_url_macos() {
    let spawner = MockUrlSpawner::new(false);
    let res = open_url(REPO_URL, "macos", &spawner);
    assert_eq!(res, Ok(()));
    let calls = spawner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "open");
    assert_eq!(calls[0].1, vec![REPO_URL]);
}

#[test]
fn test_open_url_linux() {
    let spawner = MockUrlSpawner::new(false);
    let res = open_url(REPO_URL, "linux", &spawner);
    assert_eq!(res, Ok(()));
    let calls = spawner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "xdg-open");
    assert_eq!(calls[0].1, vec![REPO_URL]);
}

#[test]
fn test_open_url_rejects_disallowed_url() {
    let spawner = MockUrlSpawner::new(false);
    let res = open_url("https://malicious.example.com", "macos", &spawner);
    assert!(matches!(res, Err(LinkError::DisallowedUrl(_))));
    assert!(spawner.calls().is_empty());
}

#[test]
fn test_open_url_unsupported_platform() {
    let spawner = MockUrlSpawner::new(false);
    let res = open_url(REPO_URL, "windows", &spawner);
    assert!(matches!(res, Err(LinkError::UnsupportedPlatform(_))));
    assert!(spawner.calls().is_empty());
}

#[test]
fn test_open_url_spawn_failure() {
    let spawner = MockUrlSpawner::new(true);
    let res = open_url(REPO_URL, "macos", &spawner);
    assert!(matches!(res, Err(LinkError::SpawnFailed(_))));
}

#[test]
fn test_osc8_escape_format() {
    let escaped = osc8("source code", REPO_URL);
    let expected = format!("\x1b]8;;{REPO_URL}\x1b\\source code\x1b]8;;\x1b\\");
    assert_eq!(escaped, expected);
}

#[test]
fn test_sidebar_brand_rendering_wide_and_compact() {
    let temp_dir = std::env::temp_dir().join(format!("tandem-test-{}", std::process::id()));
    let store = CoordStore::new(temp_dir.join("coord"));
    let tmux = TmuxClient::with_runner("tandem-test", None, Arc::new(DummyTmuxRunner));
    let mut app = SidebarApp::new(temp_dir, store, Some(tmux));

    // Wide: width 26
    let backend_wide = TestBackend::new(26, 10);
    let mut term_wide = Terminal::new(backend_wide).unwrap();
    app.brand_focused = false;
    term_wide
        .draw(|f| app.render(f.area(), f.buffer_mut()))
        .unwrap();
    let text_wide = buffer_text(term_wide.backend().buffer());
    assert!(text_wide.lines().next().unwrap().contains("tandem"));
    assert!(text_wide.lines().next().unwrap().contains("↗"));

    // Wide focused
    app.brand_focused = true;
    term_wide
        .draw(|f| app.render(f.area(), f.buffer_mut()))
        .unwrap();
    let text_wide_f = buffer_text(term_wide.backend().buffer());
    assert!(text_wide_f.lines().next().unwrap().contains("▶"));

    // Compact: width 14
    let backend_compact = TestBackend::new(14, 10);
    let mut term_compact = Terminal::new(backend_compact).unwrap();
    app.brand_focused = false;
    term_compact
        .draw(|f| app.render(f.area(), f.buffer_mut()))
        .unwrap();
    let text_compact = buffer_text(term_compact.backend().buffer());
    assert!(text_compact.lines().next().unwrap().contains("tdm"));

    // Compact focused
    app.brand_focused = true;
    term_compact
        .draw(|f| app.render(f.area(), f.buffer_mut()))
        .unwrap();
    let text_compact_f = buffer_text(term_compact.backend().buffer());
    assert!(text_compact_f.lines().next().unwrap().contains("▶"));
}

#[test]
fn test_sidebar_brand_focus_navigation() {
    let temp_dir = std::env::temp_dir().join(format!("tandem-test-nav-{}", std::process::id()));
    let store = CoordStore::new(temp_dir.join("coord"));
    let tmux = TmuxClient::with_runner("tandem-test", None, Arc::new(DummyTmuxRunner));
    let mut app = SidebarApp::new(temp_dir, store, Some(tmux));

    // Home focuses brand
    app.handle_key(&KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    assert!(app.brand_focused);

    // Down from brand moves to first agent
    app.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(!app.brand_focused);
    assert_eq!(app.selected, 1);

    // Up from first agent moves back to brand
    app.handle_key(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(app.brand_focused);

    // Up from brand wraps to last selectable row (AddAgent)
    app.handle_key(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert!(!app.brand_focused);
    assert_eq!(app.selected, 2);

    // Down from last selectable row wraps back to brand
    app.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(app.brand_focused);
}

#[test]
fn test_sidebar_brand_activation_and_status_message() {
    let temp_dir = std::env::temp_dir().join(format!("tandem-test-act-{}", std::process::id()));
    let store = CoordStore::new(temp_dir.join("coord"));
    let tmux = TmuxClient::with_runner("tandem-test", None, Arc::new(DummyTmuxRunner));
    let mut app = SidebarApp::new(temp_dir, store, Some(tmux));

    let spawner_ok = MockUrlSpawner::new(false);
    app.brand_focused = true;
    app.open_link_with("macos", &spawner_ok);
    assert_eq!(app.status_msg.as_deref(), Some("Opened in browser"));
    assert_eq!(spawner_ok.calls().len(), 1);

    let spawner_fail = MockUrlSpawner::new(true);
    app.open_link_with("macos", &spawner_fail);
    assert_eq!(
        app.status_msg.as_deref(),
        Some("Could not open the link: https://github.com/hydrokat/agent-workspace")
    );

    // Render with failure message: status line is visible in buffer
    let backend = TestBackend::new(80, 10);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| app.render(f.area(), f.buffer_mut())).unwrap();
    let text = buffer_text(term.backend().buffer());
    assert!(text.contains("Could not open the link: https://github.com/hydrokat/agent-workspace"));
}

#[test]
fn truncate_to_width_never_splits_a_multibyte_char() {
    use tandem::sidebar::app::truncate_to_width;
    assert_eq!(truncate_to_width("▶tdm 1.10 ↗", 14), "▶tdm 1.10 ↗");
    assert_eq!(truncate_to_width("▶tdm 1.10 ↗", 10), "▶tdm 1.10 ");
    assert_eq!(truncate_to_width("▶tdm 1.10 ↗", 0), "");
    assert_eq!(truncate_to_width("abc", 2), "ab");
    assert_eq!(truncate_to_width("↗↗↗", 2), "↗↗");
}

#[test]
fn compact_brand_with_two_digit_minor_version_does_not_panic_at_narrow_widths() {
    let temp_dir = std::env::temp_dir().join(format!("tandem-test-narrow-{}", std::process::id()));
    let store = CoordStore::new(temp_dir.join("coord"));
    let tmux = TmuxClient::with_runner("tandem-test", None, Arc::new(DummyTmuxRunner));
    let mut app = SidebarApp::new(temp_dir, store, Some(tmux));
    for width in 1..=30u16 {
        for focused in [false, true] {
            app.brand_focused = focused;
            let mut term = Terminal::new(TestBackend::new(width, 6)).unwrap();
            term.draw(|f| app.render(f.area(), f.buffer_mut())).unwrap();
        }
    }
}
