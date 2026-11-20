mod common;

use common::TempDir;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tandem::config::Config;
use tandem::coordination::store::CoordStore;
use tandem::factory::model::IdeaState;
use tandem::factory::ui_intent::{FactoryIntent, take_intent, write_intent};
use tandem::sidebar::app::SidebarApp;
use tandem::sidebar::model::{SidebarRow, rows_from, rows_from_for};
use tandem::snapshot::{FactorySummary, IdeaCard, RunSummary};
use tandem::status::{StatusApp, StatusTab};
use tandem::tmux::keys::{default_keymap, help_lines_with_factory, mouse_commands_for};
use tandem::tmux::layout::{STATUS_ROWS, status_rows_for};
use tandem::tmux::tabs::PaneInfo;

fn press(app: &mut StatusApp, c: char) {
    app.handle_key(&KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
}

fn status_app(ws: &std::path::Path, factory: bool) -> StatusApp {
    let store = CoordStore::new(ws.join(".agent-workspace/coordination"));
    StatusApp::new(
        ws.to_path_buf(),
        store,
        None,
        Some(Config {
            factory,
            ..Config::default()
        }),
    )
}

fn workspace() -> TempDir {
    let tmp = TempDir::new();
    std::fs::create_dir_all(tmp.path().join(".agent-workspace/coordination/queries")).unwrap();
    std::fs::create_dir_all(tmp.path().join("specs")).unwrap();
    tmp
}

#[test]
fn test_factory_tab_set_is_dedicated_four_tabs() {
    assert_eq!(
        StatusTab::FACTORY_TABS,
        [
            StatusTab::Factory,
            StatusTab::Queries,
            StatusTab::Web,
            StatusTab::Settings
        ]
    );
    assert_eq!(StatusTab::visible(true), &StatusTab::FACTORY_TABS);
    assert_eq!(StatusTab::visible(false), &StatusTab::ALL);
    assert_eq!(StatusTab::Factory.index_for(true), 0);
    assert_eq!(StatusTab::Settings.index_for(true), 3);
    assert_eq!(StatusTab::Settings.index_for(false), 6);
}

#[test]
fn test_factory_app_opens_on_factory_with_four_numbered_tabs() {
    let ws = workspace();
    let mut app = status_app(ws.path(), true);
    assert_eq!(app.tab, StatusTab::Factory);
    assert_eq!(app.tab_bar.selected, 0);
    assert_eq!(
        app.tab_bar.titles,
        vec!["Factory", "Queries", "Web", "Settings"]
    );

    press(&mut app, '2');
    assert_eq!(app.tab, StatusTab::Queries);
    assert_eq!(app.tab_bar.selected, 1);
    press(&mut app, '3');
    assert_eq!(app.tab, StatusTab::Web);
    press(&mut app, '4');
    assert_eq!(app.tab, StatusTab::Settings);
    assert_eq!(app.tab_bar.selected, 3);
    press(&mut app, '5');
    assert_eq!(app.tab, StatusTab::Settings);
    press(&mut app, '1');
    assert_eq!(app.tab, StatusTab::Factory);
}

#[test]
fn test_factory_app_never_lands_on_tandem_only_tabs() {
    let ws = workspace();
    let mut app = status_app(ws.path(), true);
    app.set_tab(StatusTab::Sprint);
    assert_eq!(app.tab, StatusTab::Factory);
    app.action_show_status();
    assert_eq!(app.tab, StatusTab::Factory);
    app.set_tab(StatusTab::Settings);
    app.action_reset_ui();
    assert_eq!(app.tab, StatusTab::Factory);
}

#[test]
fn test_tandem_app_keeps_seven_tabs_and_status_default() {
    let ws = workspace();
    let mut app = status_app(ws.path(), false);
    assert_eq!(app.tab, StatusTab::Status);
    assert_eq!(app.tab_bar.titles.len(), 7);
    press(&mut app, '7');
    assert_eq!(app.tab, StatusTab::Settings);
    assert_eq!(app.tab_bar.selected, 6);
    press(&mut app, '1');
    assert_eq!(app.tab, StatusTab::Status);
}

#[test]
fn test_status_rows_for_factory_gives_control_plane_room() {
    assert_eq!(status_rows_for(false, 45), STATUS_ROWS);
    assert_eq!(status_rows_for(false, 200), STATUS_ROWS);

    let tall = status_rows_for(true, 45);
    assert!((22..=26).contains(&tall), "55% of 45 rows, got {tall}");
    assert!(tall > STATUS_ROWS);

    let tiny = status_rows_for(true, 20);
    assert!(
        20 - tiny >= 8,
        "orchestrator pane keeps at least 8 rows, got {tiny}"
    );
    assert!(tiny >= 6);

    let huge = status_rows_for(true, 120);
    assert!(huge <= 120 - 8);
    assert_eq!(status_rows_for(true, 0), 6);
}

fn orch_pane(id: &str, index: usize, idea: &str) -> PaneInfo {
    let mut p = PaneInfo::new(
        id,
        "main",
        "agent",
        "orchestrator",
        index,
        "claude·opus",
        false,
        None,
    );
    p.idea = idea.to_string();
    p
}

fn card(id: &str, title: &str, state: IdeaState, run: Option<RunSummary>) -> IdeaCard {
    IdeaCard {
        id: id.to_string(),
        title: title.to_string(),
        state,
        owner: "you".to_string(),
        risk: None,
        blockers: 0,
        updated: 0.0,
        run,
        team_count: 0,
        needs_sign_in: false,
    }
}

fn run(id: &str, state: &str) -> RunSummary {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "state": state,
        "stages_done": 1,
        "stages_total": 5,
    }))
    .expect("run summary")
}

fn summary() -> FactorySummary {
    FactorySummary {
        enabled: true,
        ideas: vec![
            card("i-7k2m9q4d", "Dark mode", IdeaState::Ideation, None),
            card(
                "i-3b8f1a0c",
                "Export CSV",
                IdeaState::InDevelopment,
                Some(run("r-9x3a1c2b", "running")),
            ),
            card(
                "i-done0000",
                "Old",
                IdeaState::Delivered,
                Some(run("r-old00000", "delivered")),
            ),
        ],
        needs_you: 0,
        by_state: Default::default(),
        error: None,
    }
}

fn titles(rows: &[SidebarRow]) -> Vec<String> {
    rows.iter()
        .map(|r| match r {
            SidebarRow::Header { title, count } => format!("H:{title}:{count}"),
            SidebarRow::Agent { label, .. } => format!("A:{label}"),
            SidebarRow::NewIdea => "NEW".to_string(),
            SidebarRow::Run { run_id, label, .. } => format!("R:{run_id}:{label}"),
            SidebarRow::AddAgent => "ADD".to_string(),
            SidebarRow::Empty => "EMPTY".to_string(),
        })
        .collect()
}

#[test]
fn test_factory_sidebar_groups_orchestrators_and_active_runs() {
    let panes = vec![
        orch_pane("%1", 1, "i-7k2m9q4d"),
        orch_pane("%2", 2, "i-3b8f1a0c"),
    ];
    let rows = rows_from_for(&panes, &[], &[], Some("%1"), &[], Some(&summary()));
    let t = titles(&rows);
    assert_eq!(t[0], "H:FACTORY:0");
    assert_eq!(t[1], "NEW");
    assert_eq!(t[2], "H:ORCHESTRATORS:2");
    assert_eq!(t[3], "A:i-7k2m · Ideation");
    assert_eq!(t[4], "A:i-3b8f · In development");
    assert_eq!(t[5], "H:ACTIVE RUNS:1");
    assert_eq!(t[6], "R:r-9x3a1c2b:r-9x3a · running");
    assert_eq!(t.len(), 7, "terminal runs hidden, no Tandem groups: {t:?}");
    assert!(!t.iter().any(|r| r.contains("PLANNERS") || r == "ADD"));
}

#[test]
fn test_factory_sidebar_empty_state_still_offers_new_idea() {
    let rows = rows_from_for(&[], &[], &[], None, &[], Some(&FactorySummary::default()));
    let t = titles(&rows);
    assert_eq!(t, vec!["H:FACTORY:0", "NEW", "H:ORCHESTRATORS:0", "EMPTY"]);
    assert!(rows.iter().any(|r| r.is_selectable()));
}

#[test]
fn test_tandem_sidebar_rows_unchanged_without_factory() {
    let planner = PaneInfo::new(
        "%1",
        "main",
        "agent",
        "planner",
        1,
        "claude·opus",
        false,
        None,
    );
    let implementer = PaneInfo::new(
        "%2",
        "main",
        "agent",
        "implementer",
        1,
        "agy·m",
        false,
        None,
    );
    let panes = vec![planner, implementer];
    assert_eq!(
        rows_from(&panes, &[], &[], None, &[]),
        rows_from_for(&panes, &[], &[], None, &[], None)
    );
}

#[test]
fn test_sidebar_new_idea_and_run_rows_write_intents() {
    let ws = workspace();
    let store = CoordStore::new(ws.path().join(".agent-workspace/coordination"));
    let mut app = SidebarApp::new(ws.path().to_path_buf(), store, None);

    app.rows = vec![
        SidebarRow::NewIdea,
        SidebarRow::Run {
            run_id: "r-9x3a1c2b".to_string(),
            idea_id: "i-3b8f1a0c".to_string(),
            label: "r-9x3a · running".to_string(),
        },
    ];
    app.selected = 0;
    app.select_selected(false);
    assert_eq!(take_intent(ws.path()), Some(FactoryIntent::NewIdea));
    assert_eq!(take_intent(ws.path()), None);

    app.selected = 1;
    app.select_selected(false);
    assert_eq!(
        take_intent(ws.path()),
        Some(FactoryIntent::OpenIdea {
            idea_id: "i-3b8f1a0c".to_string()
        })
    );
}

#[test]
fn test_sidebar_factory_rows_render_without_panic_at_narrow_widths() {
    let ws = workspace();
    let store = CoordStore::new(ws.path().join(".agent-workspace/coordination"));
    let mut app = SidebarApp::new(ws.path().to_path_buf(), store, None);
    app.rows = rows_from_for(
        &[orch_pane("%1", 1, "i-7k2m9q4d")],
        &[],
        &[],
        Some("%1"),
        &[],
        Some(&summary()),
    );
    for width in [5u16, 10, 26] {
        let area = Rect::new(0, 0, width, 12);
        let mut buf = Buffer::empty(area);
        app.render(area, &mut buf);
    }
    let area = Rect::new(0, 0, 26, 12);
    let mut buf = Buffer::empty(area);
    app.render(area, &mut buf);
    let text: String = (0..12)
        .flat_map(|y| (0..26).map(move |x| (x, y)))
        .map(|(x, y)| buf[(x, y)].symbol().to_string())
        .collect();
    assert!(text.contains("New Idea"));
    assert!(text.contains("ORCHESTRATORS"));
}

#[test]
fn test_status_app_consumes_intents_from_sidebar() {
    let ws = workspace();
    let mut app = status_app(ws.path(), true);
    app.set_tab(StatusTab::Settings);

    write_intent(ws.path(), &FactoryIntent::NewIdea).unwrap();
    app.poll_factory_intent();
    assert_eq!(app.tab, StatusTab::Factory);
    assert!(app.factory_tab.new_idea.is_some());
    assert_eq!(take_intent(ws.path()), None);

    app.set_tab(StatusTab::Web);
    write_intent(
        ws.path(),
        &FactoryIntent::OpenIdea {
            idea_id: "i-3b8f1a0c".to_string(),
        },
    )
    .unwrap();
    app.poll_factory_intent();
    assert_eq!(app.tab, StatusTab::Factory);
    assert_eq!(app.factory_tab.selected_id.as_deref(), Some("i-3b8f1a0c"));
}

#[test]
fn test_intent_file_ignores_garbage() {
    let ws = workspace();
    let dir = ws.path().join(".agent-workspace/factory");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ui-intent.json"), "{not json").unwrap();
    assert_eq!(take_intent(ws.path()), None);
    assert!(!dir.join("ui-intent.json").exists());
}

#[test]
fn test_factory_help_omits_planner_implementer_shortcuts() {
    let km = default_keymap();
    let normal = help_lines_with_factory("linux", &km, false).join("\n");
    assert!(normal.contains("show last planner"));
    assert!(normal.contains("1-7 direct"));

    let factory = help_lines_with_factory("linux", &km, true).join("\n");
    assert!(!factory.contains("show last planner"));
    assert!(!factory.contains("show last implementer"));
    assert!(factory.contains("1-4 direct"));
}

#[test]
fn test_factory_right_click_menu_says_orchestrator_and_factory() {
    let km = default_keymap();
    let menu = |factory: bool| {
        mouse_commands_for(None, Some("/w"), "linux", &km, factory)
            .into_iter()
            .map(|c| c.join(" "))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let normal = menu(false);
    assert!(normal.contains("New agent"));
    assert!(!normal.contains("New orchestrator"));
    let factory = menu(true);
    assert!(factory.contains("New orchestrator"));
    assert!(!factory.contains("New agent"));
    assert!(factory.contains("\\\"Factory\\\"") || factory.contains("\"Factory\""));
}
