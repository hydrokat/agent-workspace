use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use tandem::adapters::base::Model;
use tandem::tmux::keys::{WIZARD_ADD, WIZARD_FILTER, display_shortcut};
use tandem::wizard::app::{ModelLoader, WizardApp};
use tandem::wizard::flow::Step;

fn test_loader() -> ModelLoader {
    Arc::new(|cli: &str| {
        if cli == "agy" {
            Ok(vec![
                Model {
                    id: "gemini-3.8-flash-high".to_string(),
                    label: "Flash High".to_string(),
                },
                Model {
                    id: "gemini-3.8-flash-low".to_string(),
                    label: "Flash Low".to_string(),
                },
            ])
        } else {
            Ok(vec![
                Model {
                    id: "opus".to_string(),
                    label: "opus (latest)".to_string(),
                },
                Model {
                    id: "sonnet".to_string(),
                    label: "sonnet (latest)".to_string(),
                },
            ])
        }
    })
}

fn failing_loader() -> ModelLoader {
    Arc::new(|_| Err("timeout".to_string()))
}

fn make_app(single: bool, loader: ModelLoader) -> (WizardApp, Terminal<TestBackend>) {
    let mut installed = HashMap::new();
    installed.insert("claude".to_string(), true);
    installed.insert("agy".to_string(), true);
    let mut app = WizardApp::new(installed, loader, single);
    let backend = TestBackend::new(100, 30);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| app.render(f.area(), f.buffer_mut())).unwrap();
    (app, term)
}

fn press_key(
    app: &mut WizardApp,
    term: &mut Terminal<TestBackend>,
    code: KeyCode,
    mods: KeyModifiers,
) {
    app.handle_key(&KeyEvent::new(code, mods));
    term.draw(|f| app.render(f.area(), f.buffer_mut())).unwrap();
}

fn type_str(app: &mut WizardApp, term: &mut Terminal<TestBackend>, s: &str) {
    for c in s.chars() {
        press_key(app, term, KeyCode::Char(c), KeyModifiers::NONE);
    }
}

fn wait_for_models(app: &mut WizardApp, term: &mut Terminal<TestBackend>) {
    for _ in 0..100 {
        app.poll_background();
        term.draw(|f| app.render(f.area(), f.buffer_mut())).unwrap();
        if !app.loading_models {
            break;
        }
        sleep(Duration::from_millis(10));
    }
}

#[test]
fn test_keyboard_flow_launches_claude_planner() {
    let (mut app, mut term) = make_app(false, test_loader());

    // Enter: claude
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    // Enter: planner
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    wait_for_models(&mut app, &mut term);

    // Enter: opus (first model)
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    // Shift+L: launch
    press_key(&mut app, &mut term, KeyCode::Char('L'), KeyModifiers::SHIFT);

    let res = app.exit_value.unwrap().unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(
        (
            res[0].cli.as_str(),
            res[0].role.as_str(),
            res[0].model.as_str()
        ),
        ("claude", "planner", "opus")
    );
}

#[test]
fn test_mouse_click_selects_option() {
    let (mut app, mut term) = make_app(false, test_loader());

    // Click agy (row 8)
    app.handle_mouse(&MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 4,
        row: 8,
        modifiers: KeyModifiers::NONE,
    });
    term.draw(|f| app.render(f.area(), f.buffer_mut())).unwrap();

    // Down: implementer
    press_key(&mut app, &mut term, KeyCode::Down, KeyModifiers::NONE);
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    wait_for_models(&mut app, &mut term);

    // Enter: first model
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    // Enter: perms No (default)
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    // Enter: cwd blank
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    // Shift+L: launch
    press_key(&mut app, &mut term, KeyCode::Char('L'), KeyModifiers::SHIFT);

    let res = app.exit_value.unwrap().unwrap();
    assert_eq!(
        (res[0].cli.as_str(), res[0].role.as_str()),
        ("agy", "implementer")
    );
    assert!(!res[0].skip_permissions);
}

#[test]
fn test_model_load_failure_allows_typing_an_id() {
    let (mut app, mut term) = make_app(false, failing_loader());

    // Down, Enter: agy
    press_key(&mut app, &mut term, KeyCode::Down, KeyModifiers::NONE);
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    // Enter: planner
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    wait_for_models(&mut app, &mut term);

    // Type model id and enter
    type_str(&mut app, &mut term, "my-model");
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    // Enter: perms No
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    // Shift+L
    press_key(&mut app, &mut term, KeyCode::Char('L'), KeyModifiers::SHIFT);

    let res = app.exit_value.unwrap().unwrap();
    assert_eq!(res[0].model, "my-model");
}

#[test]
fn test_invalid_typed_model_is_rejected_with_message() {
    let (mut app, mut term) = make_app(false, failing_loader());

    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    wait_for_models(&mut app, &mut term);

    type_str(&mut app, &mut term, "a;b");
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.message.contains("Warning"));

    press_key(&mut app, &mut term, KeyCode::Esc, KeyModifiers::NONE);
    press_key(&mut app, &mut term, KeyCode::Esc, KeyModifiers::NONE);
    press_key(&mut app, &mut term, KeyCode::Esc, KeyModifiers::NONE);

    assert_eq!(app.exit_value, Some(None));
}

#[test]
fn test_single_mode_returns_after_one_agent() {
    let (mut app, mut term) = make_app(true, test_loader());

    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    wait_for_models(&mut app, &mut term);
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);

    let res = app.exit_value.unwrap().unwrap();
    assert_eq!(res.len(), 1);
}

#[test]
fn test_launch_disabled_without_agents() {
    let (mut app, mut term) = make_app(false, test_loader());

    press_key(&mut app, &mut term, KeyCode::Char('L'), KeyModifiers::SHIFT);
    assert!(app.exit_value.is_none());
    assert!(app.message.contains("Warning"));

    press_key(&mut app, &mut term, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.exit_value, Some(None));
}

#[test]
fn test_footer_replaces_hint_bar() {
    let (app, _) = make_app(false, test_loader());
    assert!(app.check_action("go_back"));
    assert!(app.check_action("help"));
}

#[test]
fn test_model_step_alt_f_focuses_filter() {
    let (mut app, mut term) = make_app(false, test_loader());

    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    wait_for_models(&mut app, &mut term);

    assert_eq!(app.flow.step(), Step::Model);
    assert!(app.check_action("focus_input"));
    assert!(!app.check_action("add"));

    press_key(&mut app, &mut term, KeyCode::Char('f'), KeyModifiers::ALT);
    assert!(app.input_focused);
}

#[test]
fn test_summary_step_actions_use_modifier_keys() {
    let (mut app, mut term) = make_app(false, test_loader());

    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    wait_for_models(&mut app, &mut term);
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(app.flow.step(), Step::Summary);
    assert!(app.check_action("add"));
    assert!(app.check_action("delete"));
    assert!(app.check_action("launch"));
    assert!(!app.check_action("focus_input"));

    press_key(&mut app, &mut term, KeyCode::Char('a'), KeyModifiers::ALT);
    assert_eq!(app.flow.step(), Step::Cli);
}

#[test]
fn test_wizard_help_key_shows_notification() {
    let (mut app, mut term) = make_app(false, test_loader());

    press_key(&mut app, &mut term, KeyCode::Char('?'), KeyModifiers::NONE);
    assert!(app.show_help);

    press_key(&mut app, &mut term, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.show_help);
}

#[test]
fn test_check_action_filter_only_at_model_step() {
    let (mut app, mut term) = make_app(false, test_loader());

    assert!(!app.check_action("focus_input"));
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    wait_for_models(&mut app, &mut term);

    assert_eq!(app.flow.step(), Step::Model);
    assert!(app.check_action("focus_input"));
    assert!(!app.check_action("add"));
}

#[test]
fn test_check_action_add_delete_launch_only_at_summary() {
    let (mut app, mut term) = make_app(false, test_loader());

    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    wait_for_models(&mut app, &mut term);
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);

    assert_eq!(app.flow.step(), Step::Summary);
    assert!(app.check_action("add"));
    assert!(app.check_action("delete"));
    assert!(app.check_action("launch"));
    assert!(!app.check_action("focus_input"));
}

#[test]
fn test_wizard_key_display_is_os_aware() {
    let darwin_add = display_shortcut(WIZARD_ADD, "darwin").unwrap();
    let darwin_filter = display_shortcut(WIZARD_FILTER, "darwin").unwrap();
    let linux_add = display_shortcut(WIZARD_ADD, "linux").unwrap();
    let linux_filter = display_shortcut(WIZARD_FILTER, "linux").unwrap();

    assert_eq!(darwin_add, "Opt+a");
    assert_eq!(darwin_filter, "Opt+f");
    assert_eq!(linux_add, "Alt+a");
    assert_eq!(linux_filter, "Alt+f");
}

#[test]
fn test_model_load_never_blocks_input() {
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let s_clone = started.clone();
    let r_clone = release.clone();

    let slow_loader: ModelLoader = Arc::new(move |_| {
        s_clone.store(true, Ordering::SeqCst);
        while !r_clone.load(Ordering::SeqCst) {
            sleep(Duration::from_millis(5));
        }
        Ok(vec![Model {
            id: "slow-model".to_string(),
            label: "Slow Model".to_string(),
        }])
    });

    let (mut app, mut term) = make_app(false, slow_loader);

    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);
    press_key(&mut app, &mut term, KeyCode::Enter, KeyModifiers::NONE);

    // Wait until background thread starts loading
    while !started.load(Ordering::SeqCst) {
        sleep(Duration::from_millis(5));
    }
    assert!(app.loading_models);

    // While models are loading in background, user input is handled immediately
    press_key(&mut app, &mut term, KeyCode::Char('f'), KeyModifiers::ALT);
    assert!(app.input_focused);
    type_str(&mut app, &mut term, "typed-while-loading");
    assert_eq!(app.input.value, "typed-while-loading");

    // Release the background loader
    release.store(true, Ordering::SeqCst);
    wait_for_models(&mut app, &mut term);
    assert!(!app.loading_models);
    assert_eq!(app.models.len(), 1);
    assert_eq!(app.models[0].id, "slow-model");
}
