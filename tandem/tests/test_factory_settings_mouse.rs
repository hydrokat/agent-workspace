mod common;

use common::TempDir;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tandem::status::settings_tab::{
    FactoryConsentState, FactoryRevokeState, QUIT_ROW, SettingsAction, SettingsTab,
};

fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }
}

fn click(x: u16, y: u16) -> MouseEvent {
    mouse(MouseEventKind::Down(MouseButton::Left), x, y)
}

fn screen(tab: &SettingsTab, area: Rect) -> Buffer {
    let mut buf = Buffer::empty(area);
    tab.render(area, &mut buf);
    buf
}

fn find(buf: &Buffer, area: Rect, needle: &str) -> (u16, u16) {
    for y in area.y..area.y + area.height {
        let mut line = String::new();
        let mut xs = Vec::new();
        for x in area.x..area.x + area.width {
            let sym = buf[(x, y)].symbol();
            for _ in sym.chars() {
                xs.push(x);
            }
            line.push_str(sym);
        }
        if let Some(byte) = line.find(needle) {
            return (xs[line[..byte].chars().count()], y);
        }
    }
    panic!("{needle:?} not on screen");
}

fn click_text(tab: &mut SettingsTab, area: Rect, needle: &str) -> Option<SettingsAction> {
    let buf = screen(tab, area);
    let (x, y) = find(&buf, area, needle);
    tab.handle_mouse(&click(x, y), area)
}

fn factory_tab(ws: &TempDir) -> SettingsTab {
    let mut tab = SettingsTab::default();
    tab.configure_factory(true, Some(ws.path().to_path_buf()));
    tab
}

const TALL: Rect = Rect {
    x: 0,
    y: 0,
    width: 100,
    height: 40,
};

#[test]
fn test_click_factory_row_opens_edit_modal_and_modal_buttons_work() {
    let ws = TempDir::new();
    let mut tab = factory_tab(&ws);

    click_text(&mut tab, TALL, "Max parallel");
    assert!(tab.factory_edit.is_some(), "row click opens the edit modal");
    assert_eq!(tab.focused_row, QUIT_ROW + 1);

    click_text(&mut tab, TALL, "Esc cancel");
    assert!(tab.factory_edit.is_none());

    click_text(&mut tab, TALL, "Max parallel");
    tab.factory_edit.as_mut().unwrap().buffer = "4".to_string();
    click_text(&mut tab, TALL, "Enter save");
    assert!(tab.factory_edit.is_none());
    assert_eq!(tab.factory_settings.max_parallel_agents, 4);
}

#[test]
fn test_edit_modal_invalid_value_stays_open_with_error() {
    let ws = TempDir::new();
    let mut tab = factory_tab(&ws);
    click_text(&mut tab, TALL, "Max parallel");
    tab.factory_edit.as_mut().unwrap().buffer = "99".to_string();
    click_text(&mut tab, TALL, "Enter save");
    let edit = tab.factory_edit.as_ref().expect("modal stays open");
    assert!(edit.error.is_some());
}

#[test]
fn test_click_outside_modal_does_not_leak_to_rows_underneath() {
    let ws = TempDir::new();
    let mut tab = factory_tab(&ws);
    click_text(&mut tab, TALL, "Max parallel");
    let cols_before = tab.prefs.cols;
    let focus_before = tab.focused_row;

    tab.handle_mouse(&click(2, 1), TALL);
    tab.handle_mouse(&click(2, 12), TALL);

    assert!(tab.factory_edit.is_some());
    assert_eq!(tab.prefs.cols, cols_before);
    assert_eq!(tab.focused_row, focus_before);
}

#[test]
fn test_provider_row_click_opens_consent_and_auth_and_cancel_buttons() {
    let ws = TempDir::new();
    let mut tab = factory_tab(&ws);
    let provider = tab.factory_providers[0].provider.clone();

    click_text(&mut tab, TALL, &provider);
    assert!(tab.factory_consent.is_some() || tab.factory_revoke.is_some());
    if tab.factory_consent.is_none() {
        return;
    }

    let before = tab.factory_consent.as_ref().unwrap().auth_idx;
    click_text(&mut tab, TALL, ">");
    assert_ne!(tab.factory_consent.as_ref().unwrap().auth_idx, before);

    click_text(&mut tab, TALL, "Enter consent");
    assert!(
        tab.factory_consent.as_ref().unwrap().error.is_some(),
        "wrong confirm text keeps the dialog open"
    );

    click_text(&mut tab, TALL, "Esc cancel");
    assert!(tab.factory_consent.is_none());
}

#[test]
fn test_revoke_modal_yes_no_buttons() {
    let ws = TempDir::new();
    let mut tab = factory_tab(&ws);
    tab.factory_revoke = Some(FactoryRevokeState {
        provider: "claude".to_string(),
        confirm_yes: false,
    });
    click_text(&mut tab, TALL, "[ No ]");
    assert!(tab.factory_revoke.is_none());

    tab.factory_revoke = Some(FactoryRevokeState {
        provider: "claude".to_string(),
        confirm_yes: false,
    });
    click_text(&mut tab, TALL, "[ Yes ]");
    assert!(tab.factory_revoke.is_none());
}

#[test]
fn test_consent_state_built_directly_renders_clickable_hint() {
    let ws = TempDir::new();
    let mut tab = factory_tab(&ws);
    tab.factory_consent = Some(FactoryConsentState {
        provider: "claude".to_string(),
        auth_idx: 0,
        input: String::new(),
        error: None,
    });
    click_text(&mut tab, TALL, "Left/Right auth");
    assert_eq!(tab.factory_consent.as_ref().unwrap().auth_idx, 1);
}

#[test]
fn test_scroll_wheel_moves_focus_and_clamps_at_both_ends() {
    let ws = TempDir::new();
    let mut tab = factory_tab(&ws);
    let small = Rect::new(0, 0, 90, 12);
    tab.focused_row = QUIT_ROW + 1;

    for _ in 0..3 {
        tab.handle_mouse(&mouse(MouseEventKind::ScrollDown, 5, 5), small);
    }
    assert_eq!(tab.focused_row, QUIT_ROW + 4);

    for _ in 0..200 {
        tab.handle_mouse(&mouse(MouseEventKind::ScrollDown, 5, 5), small);
    }
    let last = tab.focused_row;
    tab.handle_mouse(&mouse(MouseEventKind::ScrollDown, 5, 5), small);
    assert_eq!(tab.focused_row, last, "no wrap-around at the bottom");

    let buf = screen(&tab, small);
    let _ = buf;

    for _ in 0..500 {
        tab.handle_mouse(&mouse(MouseEventKind::ScrollUp, 5, 5), small);
    }
    assert_eq!(tab.focused_row, 0);
    tab.handle_mouse(&mouse(MouseEventKind::ScrollUp, 5, 5), small);
    assert_eq!(tab.focused_row, 0, "no wrap-around at the top");
}

#[test]
fn test_click_on_scrolled_viewport_hits_visible_row() {
    let ws = TempDir::new();
    let mut tab = factory_tab(&ws);
    let small = Rect::new(0, 0, 100, 12);
    tab.focused_row = tab.focused_row.max(QUIT_ROW + 8);
    let provider_row = tab.focused_row;

    let buf = screen(&tab, small);
    let (x, y) = find(&buf, small, "▶");
    tab.focused_row = 0;
    tab.handle_mouse(&click(x + 4, y), small);
    assert_eq!(
        tab.focused_row, provider_row,
        "click maps through the scroll offset"
    );
}

#[test]
fn test_standard_rows_keep_their_click_targets() {
    let ws = TempDir::new();
    let mut tab = factory_tab(&ws);
    let before = tab.prefs.cols;
    let action = tab.handle_mouse(&click(3, 1), TALL);
    assert_eq!(action, Some(SettingsAction::Changed));
    assert_ne!(tab.prefs.cols, before);

    tab.handle_mouse(&click(3, 6), TALL);
    assert!(tab.confirm_reset);
    tab.confirm_reset = false;

    let action = tab.handle_mouse(&click(15, 0), TALL);
    assert_eq!(action, Some(SettingsAction::QuitMenu));
}

#[test]
fn test_non_factory_settings_scroll_is_a_noop_and_clicks_unchanged() {
    let mut tab = SettingsTab::default();
    let area = Rect::new(0, 0, 80, 12);
    assert_eq!(
        tab.handle_mouse(&mouse(MouseEventKind::ScrollDown, 3, 3), area),
        None
    );
    let before = tab.prefs.cols;
    tab.handle_mouse(&click(3, 1), area);
    assert_ne!(tab.prefs.cols, before);
    let _ = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
}
