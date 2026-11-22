//! Permanent gate for the defects that shipped in v1.20.0: Factory Mode must launch
//! without the Tandem wizard, show a dedicated Factory shell, be fully mouse-drivable,
//! and offer claude, agy, and opencode as orchestrators. Normal Tandem Mode must stay
//! byte-for-byte on its 7 tabs and PLANNERS/IMPLEMENTERS sidebar.

mod common;

use std::path::Path;

use common::TempDir;
use common::run_fixture::setup_run;
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tandem::adapters::AgentSpec;
use tandem::cli::select_launch_specs;
use tandem::config::Config;
use tandem::coordination::store::CoordStore;
use tandem::sidebar::model::{SidebarRow, rows_from, rows_from_for};
use tandem::snapshot::FactorySummary;
use tandem::status::factory_tab::{FactoryView, NewIdeaStep};
use tandem::status::{StatusApp, StatusTab};
use tandem::tmux::tabs::PaneInfo;

const AREA: Rect = Rect {
    x: 0,
    y: 0,
    width: 150,
    height: 30,
};

fn click(x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }
}

fn app_for(ws: &Path, factory: bool) -> StatusApp {
    let store = CoordStore::new(ws.join(".agent-workspace/coordination"));
    let mut app = StatusApp::new(
        ws.to_path_buf(),
        store,
        None,
        Some(Config {
            factory,
            ..Config::default()
        }),
    );
    app.factory_tab.layout = tandem::status::factory_board::BoardLayout::List;
    app.action_refresh_now();
    app
}

fn draw(app: &mut StatusApp) -> Buffer {
    let mut term = Terminal::new(TestBackend::new(AREA.width, AREA.height)).unwrap();
    term.draw(|f| app.render(f.area(), f.buffer_mut())).unwrap();
    term.backend().buffer().clone()
}

fn find(buf: &Buffer, needle: &str) -> (u16, u16) {
    for y in 0..AREA.height {
        let mut line = String::new();
        let mut xs = Vec::new();
        for x in 0..AREA.width {
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
    let screen = (0..AREA.height)
        .map(|y| {
            (0..AREA.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    panic!("{needle:?} not on screen:\n{screen}");
}

fn click_on(app: &mut StatusApp, needle: &str) {
    let buf = draw(app);
    let (x, y) = find(&buf, needle);
    app.handle_mouse(&click(x, y));
}

fn screen_text(buf: &Buffer) -> String {
    (0..AREA.height)
        .map(|y| {
            (0..AREA.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn gate_factory_launch_skips_wizard_and_filters_stale_panes() {
    let mut wizard_called = false;
    let specs = select_launch_specs(true, None, || {
        wizard_called = true;
        Ok(None)
    })
    .unwrap();
    assert!(!wizard_called);
    assert_eq!(specs, Some(Vec::<AgentSpec>::new()));

    let source = include_str!("../src/cli.rs");
    let wizard_calls = source.matches("run_launch_wizard(").count();
    assert_eq!(
        wizard_calls, 1,
        "launch must reach the Tandem wizard only through select_launch_specs"
    );
    assert!(source.contains("select_launch_specs(factory, specs"));
}

#[test]
fn gate_factory_shell_has_only_factory_tabs_and_opens_on_factory() {
    let ws = TempDir::new();
    let mut app = app_for(ws.path(), true);
    assert_eq!(app.tab, StatusTab::Factory);
    assert_eq!(
        app.tab_bar.titles,
        vec!["Factory", "Queries", "Web", "Settings"]
    );
    let text = screen_text(&draw(&mut app));
    for legacy in ["Sprint", "Spec ", "Task "] {
        assert!(
            !text.contains(legacy),
            "legacy tab {legacy:?} leaked:\n{text}"
        );
    }

    let mut tandem = app_for(ws.path(), false);
    assert_eq!(tandem.tab, StatusTab::Status);
    assert_eq!(tandem.tab_bar.titles.len(), 7);
    assert!(screen_text(&draw(&mut tandem)).contains("Sprint"));
}

#[test]
fn gate_sidebar_is_factory_native_and_tandem_sidebar_unchanged() {
    let orch = PaneInfo::new(
        "%1",
        "main",
        "agent",
        "orchestrator",
        1,
        "claude·opus",
        false,
        None,
    );
    let planner = PaneInfo::new(
        "%2",
        "main",
        "agent",
        "planner",
        1,
        "claude·opus",
        false,
        None,
    );
    let factory_rows = rows_from_for(
        std::slice::from_ref(&orch),
        &[],
        &[],
        None,
        &[],
        Some(&FactorySummary::default()),
    );
    assert!(factory_rows.contains(&SidebarRow::NewIdea));
    assert!(
        factory_rows
            .iter()
            .any(|r| matches!(r, SidebarRow::Header { title, .. } if title == "ORCHESTRATORS"))
    );
    assert!(!factory_rows.iter().any(
        |r| matches!(r, SidebarRow::Header { title, .. } if title == "PLANNERS" || title == "IMPLEMENTERS")
    ));

    let tandem_rows = rows_from(std::slice::from_ref(&planner), &[], &[], None, &[]);
    assert!(
        tandem_rows
            .iter()
            .any(|r| matches!(r, SidebarRow::Header { title, .. } if title == "PLANNERS"))
    );
    assert!(!tandem_rows.contains(&SidebarRow::NewIdea));
}

#[test]
fn gate_full_mouse_journey_board_detail_run_audit_settings() {
    let ws = TempDir::new();
    let (_store, _idea_id, _run_id) = setup_run(ws.path());
    let mut app = app_for(ws.path(), true);

    click_on(&mut app, "new idea");
    assert_eq!(app.factory_tab.view, FactoryView::NewIdea);
    let modal = app.factory_tab.new_idea.as_ref().unwrap();
    let ids: Vec<&str> = modal.cli_items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, vec!["claude", "agy", "opencode"]);
    assert_eq!(modal.step, NewIdeaStep::Title);
    click_on(&mut app, "Esc cancel");
    assert_eq!(app.factory_tab.view, FactoryView::Board);

    let buf = draw(&mut app);
    let (x, y) = find(&buf, "i-");
    app.handle_mouse(&click(x, y));
    app.handle_mouse(&click(x, y));
    assert_eq!(app.factory_tab.view, FactoryView::Detail);

    click_on(&mut app, "Enter run");
    assert_eq!(app.factory_tab.view, FactoryView::RunMonitor);
    click_on(&mut app, "◀ Idea");
    assert_eq!(app.factory_tab.view, FactoryView::Detail);

    click_on(&mut app, "u audit");
    assert_eq!(app.factory_tab.view, FactoryView::Audit);
    click_on(&mut app, "Esc back");
    assert_eq!(app.factory_tab.view, FactoryView::Detail);
    click_on(&mut app, "◀ Ideas");
    assert_eq!(app.factory_tab.view, FactoryView::Board);

    click_on(&mut app, "Settings");
    assert_eq!(app.tab, StatusTab::Settings);
    click_on(&mut app, "Max parallel");
    assert!(app.settings_tab.factory_edit.is_some());
    click_on(&mut app, "Esc cancel");
    assert!(app.settings_tab.factory_edit.is_none());
}
