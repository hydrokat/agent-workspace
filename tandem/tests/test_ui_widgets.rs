use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::widgets::{StatefulWidget, Widget};

use tandem::ui::buttons::{Button, ButtonRow};
use tandem::ui::collapsible::Collapsible;
use tandem::ui::footer::{Footer, FooterItem};
use tandem::ui::input::TextInput;
use tandem::ui::option_list::{OptionItem, OptionList, OptionListState};
use tandem::ui::tabs::TabBar;
use tandem::ui::textarea::TextArea;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn test_option_list_keyboard_and_mouse() {
    let items = vec![
        OptionItem::header("Section 1"),
        OptionItem::new("item1", "Item 1"),
        OptionItem::disabled("item2", "Item 2 (disabled)"),
        OptionItem::new("item3", "Item 3"),
    ];

    let mut state = OptionListState::default();
    state.select_first_selectable(&items);
    assert_eq!(state.selected_id(&items), Some("item1"));

    state.select_next(&items);
    assert_eq!(state.selected_id(&items), Some("item3"));

    state.select_prev(&items);
    assert_eq!(state.selected_id(&items), Some("item1"));

    // Mouse click on header (row 10): cannot be clicked
    assert_eq!(state.click_at(5, 10, 10, 5, &items), None);
    // Mouse click on item1 (row 11)
    assert_eq!(state.click_at(5, 11, 10, 5, &items), Some(1));
    assert_eq!(state.selected_id(&items), Some("item1"));

    // Mouse click on disabled item2 (row 12): cannot be clicked
    assert_eq!(state.click_at(5, 12, 10, 5, &items), None);

    // Mouse click outside area (row 20)
    assert_eq!(state.click_at(5, 20, 10, 5, &items), None);

    // Render check
    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let list = OptionList::new(&items);
            list.render(f.area(), f.buffer_mut(), &mut state);
        })
        .unwrap();
}

#[test]
fn test_text_input_keyboard_and_mouse() {
    let mut input = TextInput::with_placeholder("Enter text...");
    assert_eq!(input.value, "");
    assert_eq!(input.cursor, 0);

    input.handle_key(&key(KeyCode::Char('h')));
    input.handle_key(&key(KeyCode::Char('i')));
    assert_eq!(input.value, "hi");
    assert_eq!(input.cursor, 2);

    input.handle_key(&key(KeyCode::Left));
    assert_eq!(input.cursor, 1);

    input.handle_key(&key(KeyCode::Char('o')));
    assert_eq!(input.value, "hoi");
    assert_eq!(input.cursor, 2);

    input.handle_key(&key(KeyCode::Backspace));
    assert_eq!(input.value, "hi");
    assert_eq!(input.cursor, 1);

    input.handle_key(&key(KeyCode::Home));
    assert_eq!(input.cursor, 0);

    input.handle_key(&key(KeyCode::Delete));
    assert_eq!(input.value, "i");

    input.handle_key(&key(KeyCode::End));
    assert_eq!(input.cursor, 1);

    // Mouse click_at
    input.set_value("hello world");
    input.click_at(15, 10, 30);
    assert_eq!(input.cursor, 5);
    assert!(input.focused);

    // Mouse click beyond text length but within area clamps to length
    input.click_at(35, 10, 50);
    assert_eq!(input.cursor, 11);

    let backend = TestBackend::new(40, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            input.render_with_block(f.area(), f.buffer_mut(), None);
        })
        .unwrap();
}

#[test]
fn test_text_area_keyboard_and_mouse() {
    let mut textarea = TextArea::new();
    textarea.handle_key(&key(KeyCode::Char('a')));
    textarea.handle_key(&key(KeyCode::Enter));
    textarea.handle_key(&key(KeyCode::Char('b')));
    assert_eq!(textarea.lines, vec!["a", "b"]);
    assert_eq!(textarea.cursor_row, 1);
    assert_eq!(textarea.cursor_col, 1);

    textarea.handle_key(&key(KeyCode::Up));
    assert_eq!(textarea.cursor_row, 0);
    assert_eq!(textarea.cursor_col, 1);

    textarea.handle_key(&key(KeyCode::Down));
    assert_eq!(textarea.cursor_row, 1);

    // Click hit testing
    textarea.click_at(12, 10, 10, 10, 20, 5);
    assert_eq!(textarea.cursor_row, 0);
    assert_eq!(textarea.cursor_col, 1);
    assert!(textarea.focused);

    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            textarea.render_with_block(f.area(), f.buffer_mut(), None);
        })
        .unwrap();
}

#[test]
fn test_collapsible_toggle_and_click() {
    let mut c = Collapsible::new("Details", false);
    assert!(!c.expanded);

    c.toggle();
    assert!(c.expanded);

    // Click at row 5 where area_y is 5
    assert!(c.click_at(5, 5));
    assert!(!c.expanded);

    // Click elsewhere
    assert!(!c.click_at(6, 5));
    assert!(!c.expanded);

    let backend = TestBackend::new(40, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            c.render_header(Rect::new(0, 0, 40, 1), f.buffer_mut());
        })
        .unwrap();
}

#[test]
fn test_tab_bar_navigation_and_click() {
    let titles = vec![
        "Tab 1".to_string(),
        "Tab 2".to_string(),
        "Tab 3".to_string(),
    ];
    let mut tabs = TabBar::new(titles, 0);
    assert_eq!(tabs.selected, 0);

    tabs.select_next();
    assert_eq!(tabs.selected, 1);

    tabs.select_next();
    assert_eq!(tabs.selected, 2);

    tabs.select_next();
    assert_eq!(tabs.selected, 0);

    tabs.select_prev();
    assert_eq!(tabs.selected, 2);

    // Width of Tab 1 is "Tab 1".len() + 4 = 9. So 0..9 is Tab 0, 9..18 is Tab 1.
    assert_eq!(tabs.click_at(3, 0, 40), Some(0));
    assert_eq!(tabs.selected, 0);

    assert_eq!(tabs.click_at(12, 0, 40), Some(1));
    assert_eq!(tabs.selected, 1);

    let backend = TestBackend::new(40, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            (&tabs).render(f.area(), f.buffer_mut());
        })
        .unwrap();
}

#[test]
fn test_button_row_keys_and_click() {
    let buttons = vec![
        Button::new("cancel", "Cancel"),
        Button::default_btn("ok", "OK"),
    ];
    let mut row = ButtonRow::new(buttons);
    assert_eq!(row.focused_idx, 1);

    assert_eq!(row.handle_key(&key(KeyCode::Left)), None);
    assert_eq!(row.focused_idx, 0);

    assert_eq!(row.handle_key(&key(KeyCode::Enter)), Some("cancel"));

    assert_eq!(row.handle_key(&key(KeyCode::Right)), None);
    assert_eq!(row.focused_idx, 1);

    // Mouse click
    // Button "Cancel" width: 6 + 4 = 10, plus gap 2 = 12.
    assert_eq!(row.click_at(3, 0, 30), Some("cancel"));
    assert_eq!(row.focused_idx, 0);

    assert_eq!(row.click_at(14, 0, 30), Some("ok"));
    assert_eq!(row.focused_idx, 1);

    let backend = TestBackend::new(40, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            (&row).render(f.area(), f.buffer_mut());
        })
        .unwrap();
}

#[test]
fn test_footer_render() {
    let items = vec![
        FooterItem::new("Esc", "Back"),
        FooterItem::new("Enter", "Submit"),
    ];
    let footer = Footer::new(&items);

    let backend = TestBackend::new(40, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            footer.render(f.area(), f.buffer_mut());
        })
        .unwrap();
}
