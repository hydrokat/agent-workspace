use std::fs;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tandem::factory::checks::{set_checks, set_deploy_command};
use tandem::factory::events;
use tandem::factory::model::ProviderGateState;
use tandem::factory::routing::ProviderModelTiers;
use tandem::factory::settings;
use tandem::factory::store::FactoryStore;
use tandem::status::settings_tab::{
    QUIT_ROW, RESET_ROW, SettingsAction, SettingsTab, WORKSPACE_NAME_ROW,
};

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

#[test]
fn standard_settings_navigation_workspace_rename_reset_modal_and_mouse() {
    let mut tab = SettingsTab::new(Default::default(), Default::default());
    assert!(!tab.is_editing_or_modal());

    // Wrap up from 0 -> QUIT_ROW, then Enter -> QuitMenu
    tab.handle_key(&key(KeyCode::Up));
    assert_eq!(tab.focused_row, QUIT_ROW);
    assert_eq!(
        tab.handle_key(&key(KeyCode::Enter)),
        Some(SettingsAction::QuitMenu)
    );

    // Wrap down from QUIT_ROW -> 0, cycle cols/rows/fill/sidebar
    tab.handle_key(&key(KeyCode::Down));
    assert_eq!(tab.focused_row, 0);
    for _ in 0..4 {
        assert_eq!(
            tab.handle_key(&key(KeyCode::Right)),
            Some(SettingsAction::Changed)
        );
        assert_eq!(
            tab.handle_key(&key(KeyCode::Left)),
            Some(SettingsAction::Changed)
        );
        tab.handle_key(&key(KeyCode::Down));
    }
    assert_eq!(tab.focused_row, WORKSPACE_NAME_ROW);

    // Workspace name edit: Esc cancels, Enter saves
    tab.handle_key(&key(KeyCode::Enter));
    assert!(tab.is_editing_or_modal());
    type_str(&mut tab, "Alpha");
    tab.handle_key(&key(KeyCode::Backspace));
    tab.handle_key(&key(KeyCode::Esc));
    assert!(!tab.is_editing_or_modal());
    assert_eq!(tab.workspace_name, "");

    tab.handle_key(&key(KeyCode::Enter));
    type_str(&mut tab, "My Team");
    assert_eq!(
        tab.handle_key(&key(KeyCode::Enter)),
        Some(SettingsAction::RenameWorkspace("My Team".to_string()))
    );
    assert_eq!(tab.workspace_name, "My Team");

    // Reset modal: Tab toggles focus, Enter confirms ResetUi
    tab.handle_key(&key(KeyCode::Down));
    assert_eq!(tab.focused_row, RESET_ROW);
    tab.handle_key(&key(KeyCode::Enter));
    assert!(tab.confirm_reset);
    let modal_render = render_text(&tab, 80, 24);
    assert!(modal_render.contains("Reset the UI"));
    tab.handle_key(&key(KeyCode::Tab));
    assert_eq!(
        tab.handle_key(&key(KeyCode::Enter)),
        Some(SettingsAction::ResetUi)
    );
    assert!(!tab.confirm_reset);

    // Mouse clicks on standard rows, quit button, and reset modal
    let area = Rect::new(0, 0, 80, 24);
    let click = |col, row| MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: col,
        row,
        modifiers: KeyModifiers::NONE,
    };
    assert_eq!(
        tab.handle_mouse(&click(15, 0), area),
        Some(SettingsAction::QuitMenu)
    );
    assert_eq!(
        tab.handle_mouse(&click(5, 1), area),
        Some(SettingsAction::Changed)
    );
    assert_eq!(tab.handle_mouse(&click(5, 6), area), None);
    assert!(tab.confirm_reset);
    assert_eq!(
        tab.handle_mouse(&click(32, 12), area),
        Some(SettingsAction::ResetUi)
    );
}

#[test]
fn factory_all_rows_editing_tab_guard_modals_and_narrow_scroll() {
    let ws = temp_workspace("all-rows");
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();

    let wf_dir = ws.join("workflows");
    fs::create_dir_all(&wf_dir).unwrap();
    fs::write(
        wf_dir.join("default-delivery.yaml"),
        "name: default-delivery\nversion: 1\ntrigger: ready-for-development\nstages:\n  - name: implement\n    agent: implementer\n    tools: [repo_read, repo_write, test_runner, diff_review]\n",
    )
    .unwrap();
    fs::write(
        wf_dir.join("fast-fix.yaml"),
        "name: fast-fix\nversion: 1\ntrigger: ready-for-development\nstages:\n  - name: implement\n    agent: implementer\n    tools: [repo_read, repo_write, test_runner, diff_review]\n",
    )
    .unwrap();

    let no_env = |_: &str| None;
    let no_pane = |_: &str| None;
    let caller = tandem::factory::approval::Caller::cli(true, &no_env, &no_pane);
    set_checks(
        &store,
        "workspace",
        vec![vec!["true".to_string()]],
        vec![],
        None,
        vec![],
        "workspace",
        "alice",
        &caller,
    )
    .unwrap();
    set_deploy_command(
        &store,
        "workspace",
        vec!["true".to_string()],
        "workspace",
        "alice",
        &caller,
    )
    .unwrap();

    let mut s = settings::load(&store).unwrap();
    s.model_tiers.insert(
        "claude".to_string(),
        ProviderModelTiers {
            economy: "claude-haiku".to_string(),
            standard: "claude-sonnet".to_string(),
            premium: "claude-opus".to_string(),
        },
    );
    settings::save(&store, &s).unwrap();

    let mut tab = SettingsTab::default();
    tab.configure_factory(true, Some(ws.clone()));

    // Edit default_temperature + Tab guard while editing
    tab.focus_factory_key("default_temperature");
    tab.handle_key(&key(KeyCode::Enter));
    assert!(tab.is_editing_or_modal());
    tab.handle_key(&key(KeyCode::Tab));
    assert!(tab.is_editing_or_modal(), "Tab must not exit in-place edit");
    for _ in 0..5 {
        tab.handle_key(&key(KeyCode::Backspace));
    }
    type_str(&mut tab, "0.8");
    tab.handle_key(&key(KeyCode::Enter));
    assert!(!tab.is_editing_or_modal());
    assert_eq!(settings::load(&store).unwrap().default_temperature, 0.8);

    // Edit max_review_rounds
    tab.focus_factory_key("max_review_rounds");
    tab.handle_key(&key(KeyCode::Enter));
    tab.handle_key(&key(KeyCode::Backspace));
    type_str(&mut tab, "4");
    tab.handle_key(&key(KeyCode::Enter));
    assert_eq!(settings::load(&store).unwrap().max_review_rounds, 4);

    // Cycle default_workflow
    tab.focus_factory_key("default_workflow");
    tab.handle_key(&key(KeyCode::Right));
    assert_eq!(settings::load(&store).unwrap().default_workflow, "fast-fix");
    tab.handle_key(&key(KeyCode::Left));
    assert_eq!(
        settings::load(&store).unwrap().default_workflow,
        "default-delivery"
    );

    // Edit RunBudget sub-fields (cost, duration, retries)
    tab.focus_factory_key("default_budget");
    tab.handle_key(&key(KeyCode::Enter));
    for _ in 0..6 {
        tab.handle_key(&key(KeyCode::Backspace));
    }
    type_str(&mut tab, "12.5");
    tab.handle_key(&key(KeyCode::Enter));
    assert_eq!(
        settings::load(&store).unwrap().default_budget.max_cost_usd,
        Some(12.5)
    );

    tab.handle_key(&key(KeyCode::Right)); // duration
    tab.handle_key(&key(KeyCode::Enter));
    for _ in 0..6 {
        tab.handle_key(&key(KeyCode::Backspace));
    }
    type_str(&mut tab, "600");
    tab.handle_key(&key(KeyCode::Enter));
    assert_eq!(
        settings::load(&store)
            .unwrap()
            .default_budget
            .max_duration_secs,
        Some(600)
    );

    tab.handle_key(&key(KeyCode::Right)); // retries
    tab.handle_key(&key(KeyCode::Enter));
    for _ in 0..6 {
        tab.handle_key(&key(KeyCode::Backspace));
    }
    type_str(&mut tab, "3");
    tab.handle_key(&key(KeyCode::Enter));
    assert_eq!(
        settings::load(&store).unwrap().default_budget.max_retries,
        Some(3)
    );

    // Edit approval_grace_secs
    tab.focus_factory_key("approval_grace_secs");
    tab.handle_key(&key(KeyCode::Enter));
    for _ in 0..4 {
        tab.handle_key(&key(KeyCode::Backspace));
    }
    type_str(&mut tab, "90");
    tab.handle_key(&key(KeyCode::Enter));
    assert_eq!(settings::load(&store).unwrap().approval_grace_secs, 90);

    // Consent modal: wrong input error, Left/Right auth cycle, Backspace, Esc cancel
    tab.focus_factory_provider("claude");
    tab.handle_key(&key(KeyCode::Enter));
    assert!(tab.is_editing_or_modal());
    let consent_render = render_text(&tab, 80, 30);
    assert!(consent_render.contains("Provider Consent: claude"));
    type_str(&mut tab, "wrong");
    tab.handle_key(&key(KeyCode::Enter));
    let err_render = render_text(&tab, 80, 30);
    assert!(err_render.contains("Type claude to confirm"));
    for _ in 0..5 {
        tab.handle_key(&key(KeyCode::Backspace));
    }
    tab.handle_key(&key(KeyCode::Left));
    tab.handle_key(&key(KeyCode::Right));
    tab.handle_key(&key(KeyCode::Esc));
    assert!(!tab.is_editing_or_modal());

    // Consent then Revoke via Tab + Enter on Yes, and also test No cancel
    tab.handle_key(&key(KeyCode::Enter));
    tab.handle_key(&key(KeyCode::Right));
    type_str(&mut tab, "claude");
    tab.handle_key(&key(KeyCode::Enter));
    assert!(!tab.is_editing_or_modal());

    tab.focus_factory_provider("claude");
    tab.handle_key(&key(KeyCode::Enter));
    let revoke_render = render_text(&tab, 80, 30);
    assert!(revoke_render.contains("Revoke Provider: claude"));
    tab.handle_key(&key(KeyCode::Enter)); // confirm_yes is false -> cancels
    assert!(!tab.is_editing_or_modal());

    tab.handle_key(&key(KeyCode::Enter));
    tab.handle_key(&key(KeyCode::Tab)); // confirm_yes -> true
    tab.handle_key(&key(KeyCode::Enter));
    assert!(!tab.is_editing_or_modal());

    // Navigate down through ModelTier, CheckCommands, DeployCommand and render narrow + scrolled
    while !matches!(tab.focused_row, r if r >= QUIT_ROW + 7) {
        tab.handle_key(&key(KeyCode::Down));
    }
    // Render narrow with small height to exercise scrolling & narrow layout for all sections
    let narrow_scrolled = render_text(&tab, 60, 12);
    assert!(!narrow_scrolled.is_empty());

    // Activate CheckCommands and DeployCommand to trigger read-only flashes
    tab.focused_row = QUIT_ROW + 6 + tab.factory_providers.len() + 2;
    tab.handle_key(&key(KeyCode::Enter));
    tab.handle_key(&key(KeyCode::Down));
    tab.handle_key(&key(KeyCode::Enter));

    let _ = fs::remove_dir_all(ws);
}
