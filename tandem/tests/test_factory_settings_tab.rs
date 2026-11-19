use std::fs;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tandem::factory::events;
use tandem::factory::model::ProviderGateState;
use tandem::factory::settings;
use tandem::factory::store::FactoryStore;
use tandem::status::settings_tab::SettingsTab;

fn temp_workspace(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tandem-fst-{tag}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn type_str(tab: &mut SettingsTab, s: &str) {
    for ch in s.chars() {
        tab.handle_key(&key(KeyCode::Char(ch)));
    }
}

fn render_text(tab: &SettingsTab, width: u16, height: u16) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    tab.render(area, &mut buf);
    let mut out = String::new();
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            line.push_str(buf.cell((x, y)).unwrap().symbol());
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[test]
fn normal_session_hides_factory_section() {
    let tab = SettingsTab {
        factory_enabled: false,
        ..Default::default()
    };
    let rendered = render_text(&tab, 100, 30);
    assert!(!rendered.contains("FACTORY"));
    assert!(!rendered.contains("Max parallel agents"));
    assert!(rendered.contains("Grid layout"));
    assert!(rendered.contains("[ Reset UI ]"));
}

#[test]
fn factory_session_renders_plain_settings_providers_and_checks_wide_and_narrow() {
    let ws = temp_workspace("render");
    let wf_dir = ws.join("workflows");
    fs::create_dir_all(&wf_dir).unwrap();
    fs::write(
        wf_dir.join("default-delivery.yaml"),
        "name: default-delivery\nversion: 1\nstages:\n  - name: implement\n    agent: senior-backend-dev\n",
    )
    .unwrap();

    let mut tab = SettingsTab::default();
    tab.configure_factory(true, Some(ws.clone()));

    let wide = render_text(&tab, 100, 40);
    assert!(wide.contains("FACTORY"));
    assert!(wide.contains("Max parallel agents"));
    assert!(wide.contains("(1-8)"));
    assert!(wide.contains("Default temperature"));
    assert!(wide.contains("(0.0-1.0"));
    assert!(wide.contains("Max review rounds"));
    assert!(wide.contains("(0-5)"));
    assert!(wide.contains("Default workflow"));
    assert!(wide.contains("Run budget"));
    assert!(wide.contains("Approval grace period"));
    assert!(wide.contains("(0-600 s)"));
    assert!(wide.contains("Providers"));
    assert!(wide.contains("claude"));
    assert!(wide.contains("Model tiers"));
    assert!(wide.contains("Check commands"));
    assert!(wide.contains("Deploy command"));

    let narrow = render_text(&tab, 52, 55);
    assert!(narrow.contains("FACTORY"));
    assert!(narrow.contains("Max parallel agents"));
    assert!(narrow.contains("(1-8)"));
    let lines: Vec<&str> = narrow.lines().collect();
    let label_idx = lines
        .iter()
        .position(|l| l.contains("Max parallel agents"))
        .expect("label line present");
    assert!(
        !lines[label_idx].contains("(1-8)"),
        "narrow layout puts value and range on the second line"
    );
    assert!(
        lines[label_idx + 1].contains("(1-8)"),
        "narrow layout puts value and range on line after label"
    );

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn factory_in_place_edit_validates_saves_and_records_audit_event() {
    let ws = temp_workspace("edit");
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();

    let mut tab = SettingsTab::default();
    tab.configure_factory(true, Some(ws.clone()));

    tab.focus_factory_key("max_parallel_agents");
    tab.handle_key(&key(KeyCode::Enter));
    for _ in 0..4 {
        tab.handle_key(&key(KeyCode::Backspace));
    }
    type_str(&mut tab, "6");
    tab.handle_key(&key(KeyCode::Esc));
    assert_eq!(settings::load(&store).unwrap().max_parallel_agents, 3);

    tab.handle_key(&key(KeyCode::Enter));
    for _ in 0..4 {
        tab.handle_key(&key(KeyCode::Backspace));
    }
    type_str(&mut tab, "99");
    tab.handle_key(&key(KeyCode::Enter));
    assert_eq!(settings::load(&store).unwrap().max_parallel_agents, 3);
    assert!(tab.is_editing_or_modal());

    for _ in 0..4 {
        tab.handle_key(&key(KeyCode::Backspace));
    }
    type_str(&mut tab, "5");
    tab.handle_key(&key(KeyCode::Enter));
    assert_eq!(settings::load(&store).unwrap().max_parallel_agents, 5);
    assert!(!tab.is_editing_or_modal());

    let evs = events::read_all(&store.root.join("events.jsonl"))
        .unwrap()
        .events;
    let changed = evs
        .iter()
        .find(|e| e.r#type == "settings.changed")
        .expect("settings.changed event recorded");
    assert_eq!(changed.data["key"], "max_parallel_agents");
    assert_eq!(changed.data["old_value"], 3);
    assert_eq!(changed.data["new_value"], 5);

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn factory_provider_consent_and_revoke_flow() {
    let ws = temp_workspace("provider");
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();

    let mut tab = SettingsTab::default();
    tab.configure_factory(true, Some(ws.clone()));

    tab.focus_factory_provider("claude");
    tab.handle_key(&key(KeyCode::Enter));
    assert!(tab.is_editing_or_modal());

    tab.handle_key(&key(KeyCode::Right));
    type_str(&mut tab, "claude");
    tab.handle_key(&key(KeyCode::Enter));

    let loaded = settings::load(&store).unwrap();
    let claude_gate = loaded
        .provider_gates
        .iter()
        .find(|g| g.provider == "claude")
        .expect("claude gate present");
    assert_eq!(claude_gate.state, ProviderGateState::Open);

    tab.focus_factory_provider("claude");
    tab.handle_key(&key(KeyCode::Enter));
    assert!(tab.is_editing_or_modal());
    tab.handle_key(&key(KeyCode::Char('y')));

    let loaded_after = settings::load(&store).unwrap();
    let claude_after = loaded_after
        .provider_gates
        .iter()
        .find(|g| g.provider == "claude")
        .expect("claude gate present");
    assert_eq!(claude_after.state, ProviderGateState::Closed);

    let _ = fs::remove_dir_all(ws);
}
