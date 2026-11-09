use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use std::sync::Arc;

use tandem::coordination::store::CoordStore;
use tandem::state::UiState;
use tandem::status::app::{StatusApp, StatusTab};
use tandem::status::settings_tab::{QUIT_ROW, SettingsAction, SettingsTab};
use tandem::tmux::client::{Result as TmuxResult, TmuxClient, TmuxRunner};
use tandem::tmux::grid::{FillOrder, GridCols, GridPrefs, GridRows, SidebarMode};

struct DummyTmuxRunner;

impl TmuxRunner for DummyTmuxRunner {
    fn run(&self, _argv: &[String]) -> TmuxResult {
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
fn test_status_tab_seven_tabs_indexing_and_cycling() {
    assert_eq!(StatusTab::ALL.len(), 7);
    assert_eq!(StatusTab::Settings.index(), 6);
    assert_eq!(StatusTab::from_index(6), Some(StatusTab::Settings));

    assert_eq!(StatusTab::Queries.next(), StatusTab::Web);
    assert_eq!(StatusTab::Web.next(), StatusTab::Settings);
    assert_eq!(StatusTab::Settings.next(), StatusTab::Status);

    assert_eq!(StatusTab::Status.prev(), StatusTab::Settings);
    assert_eq!(StatusTab::Settings.prev(), StatusTab::Web);

    assert_eq!(StatusTab::Settings.title(0), "Settings");
    assert_eq!(StatusTab::Settings.short_title(0), "Set");
    assert_eq!(StatusTab::Settings.number_title(0), "7");
}

#[test]
fn test_status_app_keys_1_through_7_and_alt_comma() {
    let temp = std::env::temp_dir().join(format!("tandem-test-tab6-{}", std::process::id()));
    let store = CoordStore::new(temp.join("coord"));
    let tmux = TmuxClient::with_runner("tandem-test", None, Arc::new(DummyTmuxRunner));
    let mut app = StatusApp::new(temp, store, Some(tmux), None);

    assert_eq!(app.tab, StatusTab::Status);

    // Key 6 switches to Web, key 7 to Settings
    app.handle_key(&KeyEvent::new(KeyCode::Char('6'), KeyModifiers::NONE));
    assert_eq!(app.tab, StatusTab::Web);
    app.handle_key(&KeyEvent::new(KeyCode::Char('7'), KeyModifiers::NONE));
    assert_eq!(app.tab, StatusTab::Settings);

    // Key 1 switches to Status
    app.handle_key(&KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
    assert_eq!(app.tab, StatusTab::Status);

    // Alt+, switches to Settings
    app.handle_key(&KeyEvent::new(KeyCode::Char(','), KeyModifiers::ALT));
    assert_eq!(app.tab, StatusTab::Settings);

    // Reset to Status
    app.handle_key(&KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
    assert_eq!(app.tab, StatusTab::Status);

    // macOS Option+, sends '≤'
    app.handle_key(&KeyEvent::new(KeyCode::Char('≤'), KeyModifiers::NONE));
    assert_eq!(app.tab, StatusTab::Settings);

    // Tab from Settings wraps to Status
    app.handle_key(&KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.tab, StatusTab::Status);

    // Shift+Tab from Status wraps to Settings
    app.handle_key(&KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
    assert_eq!(app.tab, StatusTab::Settings);
}

#[test]
fn test_settings_tab_navigation_and_option_cycling() {
    let mut tab = SettingsTab::default();

    assert_eq!(tab.focused_row, 0); // Columns
    assert_eq!(tab.prefs.cols, GridCols::Auto);

    // Cycle columns Right
    tab.handle_key(&KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(tab.prefs.cols, GridCols::Fixed(1));
    tab.handle_key(&KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(tab.prefs.cols, GridCols::Fixed(2));
    tab.handle_key(&KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(tab.prefs.cols, GridCols::Fixed(1));

    // Down to Rows per page
    tab.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(tab.focused_row, 1);
    assert_eq!(tab.prefs.rows, GridRows::Auto);

    // Cycle rows Right
    tab.handle_key(&KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(tab.prefs.rows, GridRows::Fixed(1));

    // Down to Fill order
    tab.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(tab.focused_row, 2);
    assert_eq!(tab.prefs.fill, FillOrder::RowFirst);
    tab.handle_key(&KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(tab.prefs.fill, FillOrder::ColumnFirst);

    // Down to Sidebar
    tab.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(tab.focused_row, 3);
    assert_eq!(tab.sidebar_mode, SidebarMode::Auto);
    tab.handle_key(&KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(tab.sidebar_mode, SidebarMode::Wide);

    // Down to Workspace name row
    tab.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(tab.focused_row, 4);

    // Down to Reset UI button
    tab.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(tab.focused_row, 5);

    // Down to Quit button
    tab.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(tab.focused_row, QUIT_ROW);

    // Down wraps to 0
    tab.handle_key(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(tab.focused_row, 0);

    // Up wraps to the Quit button
    tab.handle_key(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(tab.focused_row, QUIT_ROW);
}

#[test]
fn test_settings_tab_confirm_reset_modal_lifecycle() {
    let mut tab = SettingsTab {
        focused_row: 5, // Reset UI button (RESET_ROW)
        ..Default::default()
    };

    assert!(!tab.confirm_reset);

    // Enter opens confirmation modal
    tab.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(tab.confirm_reset);
    assert_eq!(tab.modal_focus, 1); // Cancel is default safe focus

    // Left switches focus to Reset
    tab.handle_key(&KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(tab.modal_focus, 0); // Reset

    // Right switches back to Cancel
    tab.handle_key(&KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(tab.modal_focus, 1); // Cancel

    // Esc closes modal without reset
    tab.handle_key(&KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!tab.confirm_reset);

    // Reopen modal, move to Reset, press Enter -> should signal reset
    tab.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(tab.confirm_reset);
    tab.handle_key(&KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(tab.modal_focus, 0);

    let action = tab.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        action,
        Some(tandem::status::settings_tab::SettingsAction::ResetUi)
    );
    assert!(!tab.confirm_reset);
}

#[test]
fn test_settings_prefs_round_trip_state_v3() {
    let ui = UiState {
        grid_prefs: GridPrefs {
            cols: GridCols::Fixed(2),
            rows: GridRows::Fixed(3),
            fill: FillOrder::ColumnFirst,
        },
        sidebar_mode: SidebarMode::Compact,
        ..Default::default()
    };

    let json = serde_json::to_string(&ui).unwrap();
    let parsed: UiState = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.grid_prefs.cols, GridCols::Fixed(2));
    assert_eq!(parsed.grid_prefs.rows, GridRows::Fixed(3));
    assert_eq!(parsed.grid_prefs.fill, FillOrder::ColumnFirst);
    assert_eq!(parsed.sidebar_mode, SidebarMode::Compact);

    // Deserializing empty JSON populates defaults
    let default_parsed: UiState = serde_json::from_str("{}").unwrap();
    assert_eq!(default_parsed.grid_prefs.cols, GridCols::Auto);
    assert_eq!(default_parsed.grid_prefs.rows, GridRows::Auto);
    assert_eq!(default_parsed.grid_prefs.fill, FillOrder::RowFirst);
    assert_eq!(default_parsed.sidebar_mode, SidebarMode::Auto);
}

#[test]
fn test_settings_tab_render() {
    let tab = SettingsTab::default();
    let backend = TestBackend::new(80, 15);
    let mut term = Terminal::new(backend).unwrap();

    term.draw(|f| tab.render(f.area(), f.buffer_mut())).unwrap();
    let text = buffer_text(term.backend().buffer());

    assert!(text.contains("Grid layout"));
    assert!(text.contains("Columns"));
    assert!(text.contains("Rows per page"));
    assert!(text.contains("Fill order"));
    assert!(text.contains("Sidebar"));
    assert!(text.contains("[ Reset UI ]"));
    assert!(text.contains("github.com/hydrokat/agent-workspace"));
}

#[test]
fn test_settings_tab_quit_button() {
    let mut tab = SettingsTab {
        focused_row: QUIT_ROW,
        ..Default::default()
    };

    assert_eq!(QUIT_ROW, 6);

    // Left and Right on the Quit row return None
    assert_eq!(
        tab.handle_key(&KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
        None
    );
    assert_eq!(
        tab.handle_key(&KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
        None
    );

    // Enter and Space on the Quit row return QuitMenu
    assert_eq!(
        tab.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        Some(SettingsAction::QuitMenu)
    );
    assert_eq!(
        tab.handle_key(&KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)),
        Some(SettingsAction::QuitMenu)
    );

    // Mouse click testing
    let area = Rect::new(0, 0, 80, 15);
    tab.focused_row = 0;

    // First cell (x=14)
    let click_first = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 14,
        row: 0,
        modifiers: KeyModifiers::NONE,
    };
    assert_eq!(
        tab.handle_mouse(&click_first, area),
        Some(SettingsAction::QuitMenu)
    );
    assert_eq!(tab.focused_row, QUIT_ROW);

    // Last cell (x=22)
    let click_last = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 22,
        row: 0,
        modifiers: KeyModifiers::NONE,
    };
    assert_eq!(
        tab.handle_mouse(&click_last, area),
        Some(SettingsAction::QuitMenu)
    );

    // One cell before (x=13) -> None
    let click_before = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 13,
        row: 0,
        modifiers: KeyModifiers::NONE,
    };
    assert_eq!(tab.handle_mouse(&click_before, area), None);

    // One cell after (x=23) -> None
    let click_after = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 23,
        row: 0,
        modifiers: KeyModifiers::NONE,
    };
    assert_eq!(tab.handle_mouse(&click_after, area), None);

    // Click while Reset UI modal is open dismisses modal and returns None
    tab.confirm_reset = true;
    assert_eq!(tab.handle_mouse(&click_first, area), None);
    assert!(!tab.confirm_reset);

    // Render at height 7 contains [ Quit… ]
    let backend_h7 = TestBackend::new(80, 7);
    let mut term_h7 = Terminal::new(backend_h7).unwrap();
    term_h7
        .draw(|f| tab.render(f.area(), f.buffer_mut()))
        .unwrap();
    let text_h7 = buffer_text(term_h7.backend().buffer());
    assert!(text_h7.contains("[ Quit… ]"));
    assert!(text_h7.contains("save, detach, update, discard"));

    // Render at width 20 does not panic and hides button
    let backend_w20 = TestBackend::new(20, 7);
    let mut term_w20 = Terminal::new(backend_w20).unwrap();
    term_w20
        .draw(|f| tab.render(f.area(), f.buffer_mut()))
        .unwrap();
    let text_w20 = buffer_text(term_w20.backend().buffer());
    assert!(!text_w20.contains("[ Quit… ]"));

    // Render at width 50 includes button but hides long description
    let backend_w50 = TestBackend::new(50, 7);
    let mut term_w50 = Terminal::new(backend_w50).unwrap();
    term_w50
        .draw(|f| tab.render(f.area(), f.buffer_mut()))
        .unwrap();
    let text_w50 = buffer_text(term_w50.backend().buffer());
    assert!(text_w50.contains("[ Quit… ]"));
    assert!(!text_w50.contains("save, detach, update, discard"));
}
