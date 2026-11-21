mod common;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use common::TempDir;
use common::run_fixture::setup_run;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tandem::config::Config;
use tandem::coordination::store::CoordStore;
use tandem::factory::model::IdeaState;
use tandem::snapshot::{FactorySummary, IdeaCard, StatusSnapshot};
use tandem::status::factory_audit::{FactoryAudit, KindFilter};
use tandem::status::factory_detail::FactoryDetail;
use tandem::status::factory_run::{FactoryRun, RunSubView};
use tandem::status::factory_tab::{FactoryTab, FactoryView, IdeaFilter, NewIdeaStep};
use tandem::status::{StatusApp, StatusTab};

fn click(x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }
}

fn scroll(down: bool) -> MouseEvent {
    MouseEvent {
        kind: if down {
            MouseEventKind::ScrollDown
        } else {
            MouseEventKind::ScrollUp
        },
        column: 5,
        row: 3,
        modifiers: KeyModifiers::NONE,
    }
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
            let char_idx = line[..byte].chars().count();
            return (xs[char_idx], y);
        }
    }
    panic!(
        "{needle:?} not on screen:\n{}",
        (area.y..area.y + area.height)
            .map(|y| (area.x..area.x + area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn click_text(tab: &mut FactoryTab, snap: &StatusSnapshot, area: Rect, needle: &str) {
    let mut buf = Buffer::empty(area);
    tab.render(snap, area, &mut buf);
    let (x, y) = find(&buf, area, needle);
    tab.handle_mouse(&click(x, y), snap);
}

fn card(id: &str, title: &str, state: IdeaState) -> IdeaCard {
    IdeaCard {
        id: id.to_string(),
        title: title.to_string(),
        state,
        owner: "you".to_string(),
        risk: None,
        blockers: 0,
        updated: 0.0,
        run: None,
        team_count: 0,
        needs_sign_in: false,
        moves: Vec::new(),
    }
}

fn snapshot(ideas: Vec<IdeaCard>) -> StatusSnapshot {
    StatusSnapshot {
        factory: Some(FactorySummary {
            enabled: true,
            ideas,
            needs_you: 0,
            by_state: Default::default(),
            columns: Vec::new(),
            agents_active: 0,
            error: None,
        }),
        now: 1_000.0,
        ..StatusSnapshot::default()
    }
}

fn installed(claude: bool, agy: bool) -> HashMap<String, bool> {
    HashMap::from([
        ("claude".to_string(), claude),
        ("agy".to_string(), agy),
        ("opencode".to_string(), false),
    ])
}

fn tab_for(ws: &std::path::Path) -> FactoryTab {
    let mut tab = FactoryTab::new(
        Some(ws.to_path_buf()),
        installed(true, false),
        Arc::new(|_| Ok(Vec::new())),
    );
    tab.layout = tandem::status::factory_board::BoardLayout::List;
    tab
}

fn workspace() -> TempDir {
    let tmp = TempDir::new();
    std::fs::create_dir_all(tmp.path().join(".agent-workspace/coordination/queries")).unwrap();
    std::fs::create_dir_all(tmp.path().join("specs")).unwrap();
    tmp
}

const AREA: Rect = Rect {
    x: 0,
    y: 0,
    width: 120,
    height: 12,
};

fn three_ideas() -> StatusSnapshot {
    snapshot(vec![
        card("i-aaaaaaaa", "First idea", IdeaState::Inbox),
        card("i-bbbbbbbb", "Second idea", IdeaState::Ideation),
        card("i-cccccccc", "Third idea", IdeaState::Planning),
    ])
}

#[test]
fn test_board_row_click_selects_then_opens_detail() {
    let ws = workspace();
    let snap = three_ideas();
    let mut tab = tab_for(ws.path());
    tab.sync_snapshot(&snap, false);

    click_text(&mut tab, &snap, AREA, "i-bbbbbbbb");
    assert_eq!(tab.selected, Some(1));
    assert_eq!(tab.selected_id.as_deref(), Some("i-bbbbbbbb"));
    assert_eq!(tab.view, FactoryView::Board);

    click_text(&mut tab, &snap, AREA, "i-bbbbbbbb");
    assert_eq!(tab.view, FactoryView::Detail);
    assert_eq!(tab.detail.as_ref().unwrap().idea_id, "i-bbbbbbbb");
}

#[test]
fn test_board_scroll_wheel_moves_selection() {
    let ws = workspace();
    let snap = three_ideas();
    let mut tab = tab_for(ws.path());
    tab.sync_snapshot(&snap, false);
    let mut buf = Buffer::empty(AREA);
    tab.render(&snap, AREA, &mut buf);

    tab.handle_mouse(&scroll(true), &snap);
    tab.handle_mouse(&scroll(true), &snap);
    assert_eq!(tab.selected, Some(2));
    tab.handle_mouse(&scroll(false), &snap);
    assert_eq!(tab.selected, Some(1));
}

#[test]
fn test_board_filter_and_new_idea_clicks() {
    let ws = workspace();
    let snap = three_ideas();
    let mut tab = tab_for(ws.path());
    tab.sync_snapshot(&snap, false);

    click_text(&mut tab, &snap, AREA, "filter: All");
    assert_ne!(tab.filter, IdeaFilter::All);

    tab.filter = IdeaFilter::All;
    click_text(&mut tab, &snap, AREA, "new idea");
    assert_eq!(tab.view, FactoryView::NewIdea);
    assert!(tab.new_idea.is_some());
}

#[test]
fn test_empty_board_new_idea_click() {
    let ws = workspace();
    let snap = snapshot(Vec::new());
    let mut tab = tab_for(ws.path());
    click_text(&mut tab, &snap, AREA, "new idea");
    assert_eq!(tab.view, FactoryView::NewIdea);
}

#[test]
fn test_clicks_on_blank_space_are_safe_noops() {
    let ws = workspace();
    let snap = three_ideas();
    let mut tab = tab_for(ws.path());
    tab.sync_snapshot(&snap, false);
    let mut buf = Buffer::empty(AREA);
    tab.render(&snap, AREA, &mut buf);
    assert!(tab.handle_mouse(&click(119, 11), &snap).is_none());
    assert!(tab.handle_mouse(&click(0, 8), &snap).is_none());
    assert_eq!(tab.view, FactoryView::Board);
}

#[test]
fn test_new_idea_modal_cli_rows_back_and_cancel() {
    let ws = workspace();
    let snap = three_ideas();
    let mut tab = tab_for(ws.path());
    tab.open_new_idea();
    assert_eq!(tab.new_idea.as_ref().unwrap().step, NewIdeaStep::Title);
    for c in "Idea".chars() {
        tab.handle_key(&KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), &snap);
    }
    click_text(&mut tab, &snap, AREA, "Enter next");
    assert_eq!(
        tab.new_idea.as_ref().unwrap().step,
        NewIdeaStep::Description
    );
    click_text(&mut tab, &snap, AREA, "Ctrl+Enter done");
    assert_eq!(tab.new_idea.as_ref().unwrap().step, NewIdeaStep::Cli);

    click_text(&mut tab, &snap, AREA, "agy");
    assert_eq!(tab.new_idea.as_ref().unwrap().step, NewIdeaStep::Cli);
    assert!(
        tab.new_idea
            .as_ref()
            .unwrap()
            .message
            .contains("agy is not installed")
    );

    click_text(&mut tab, &snap, AREA, "claude");
    assert_eq!(tab.new_idea.as_ref().unwrap().step, NewIdeaStep::Model);

    click_text(&mut tab, &snap, AREA, "Esc back");
    assert_eq!(tab.new_idea.as_ref().unwrap().step, NewIdeaStep::Cli);

    click_text(&mut tab, &snap, AREA, "Esc back");
    assert_eq!(
        tab.new_idea.as_ref().unwrap().step,
        NewIdeaStep::Description
    );
    click_text(&mut tab, &snap, AREA, "Esc back");
    assert_eq!(tab.new_idea.as_ref().unwrap().step, NewIdeaStep::Title);

    click_text(&mut tab, &snap, AREA, "Esc cancel");
    assert_eq!(tab.view, FactoryView::Board);
    assert!(tab.new_idea.is_none());
}

#[test]
fn test_audit_filter_and_back_clicks() {
    let ws = workspace();
    let snap = three_ideas();
    let mut tab = tab_for(ws.path());
    tab.audit = Some(FactoryAudit::load(ws.path(), "i-aaaaaaaa"));
    tab.view = FactoryView::Audit;

    click_text(&mut tab, &snap, AREA, "f filter kind");
    assert_eq!(
        tab.audit.as_ref().unwrap().kind_filter,
        KindFilter::JobStart
    );

    click_text(&mut tab, &snap, AREA, "v verify");
    assert!(!tab.audit.as_ref().unwrap().notice.is_empty());

    let mut tab2 = tab_for(ws.path());
    tab2.audit = Some(FactoryAudit::load(ws.path(), "i-aaaaaaaa"));
    tab2.view = FactoryView::Audit;
    click_text(&mut tab2, &snap, AREA, "Esc back");
    assert_eq!(tab2.view, FactoryView::Board);
    assert!(tab2.audit.is_none());
}

#[test]
fn test_detail_back_click_returns_to_board() {
    let ws = workspace();
    let snap = three_ideas();
    let mut tab = tab_for(ws.path());
    tab.detail = Some(FactoryDetail::load(ws.path(), "i-missing00", &[], false));
    tab.view = FactoryView::Detail;
    click_text(&mut tab, &snap, AREA, "◀ Ideas");
    assert_eq!(tab.view, FactoryView::Board);
}

#[test]
fn test_detail_to_run_monitor_to_audit_via_clicks() {
    let ws = workspace();
    let (_store, idea_id, _run_id) = setup_run(ws.path());
    let snap = three_ideas();
    let mut tab = tab_for(ws.path());
    tab.detail = Some(FactoryDetail::load(ws.path(), &idea_id, &[], false));
    tab.view = FactoryView::Detail;
    let area = Rect::new(0, 0, 140, 16);

    click_text(&mut tab, &snap, area, "u audit");
    assert_eq!(tab.view, FactoryView::Audit);
    click_text(&mut tab, &snap, area, "Esc back");
    assert_eq!(tab.view, FactoryView::Detail);

    click_text(&mut tab, &snap, area, "Enter run");
    assert_eq!(tab.view, FactoryView::RunMonitor);

    click_text(&mut tab, &snap, area, "◀ Idea");
    assert_eq!(tab.view, FactoryView::Detail);
}

#[test]
fn test_run_monitor_stage_click_dialogs_and_log_scroll() {
    let ws = workspace();
    let (_store, idea_id, run_id) = setup_run(ws.path());
    let snap = three_ideas();
    let mut tab = tab_for(ws.path());
    tab.run_monitor = Some(FactoryRun::load(ws.path(), &idea_id, &run_id, "linux"));
    tab.view = FactoryView::RunMonitor;
    let area = Rect::new(0, 0, 140, 16);

    let stage_count = tab
        .run_monitor
        .as_ref()
        .unwrap()
        .run
        .as_ref()
        .unwrap()
        .stages
        .len();
    assert!(stage_count >= 2);
    let second = tab
        .run_monitor
        .as_ref()
        .unwrap()
        .run
        .as_ref()
        .unwrap()
        .stages[1]
        .name
        .clone();

    click_text(&mut tab, &snap, area, &second);
    assert_eq!(tab.run_monitor.as_ref().unwrap().selected_stage, 1);
    assert_eq!(tab.run_monitor.as_ref().unwrap().sub_view, RunSubView::Main);
    click_text(&mut tab, &snap, area, &second);
    assert!(matches!(
        tab.run_monitor.as_ref().unwrap().sub_view,
        RunSubView::StageDetail { stage_idx: 1 }
    ));
    click_text(&mut tab, &snap, area, "Esc back");
    assert_eq!(tab.run_monitor.as_ref().unwrap().sub_view, RunSubView::Main);

    click_text(&mut tab, &snap, area, "e evidence");
    assert!(matches!(
        tab.run_monitor.as_ref().unwrap().sub_view,
        RunSubView::EvidenceList { .. }
    ));
    click_text(&mut tab, &snap, area, "Esc back");
    assert_eq!(tab.run_monitor.as_ref().unwrap().sub_view, RunSubView::Main);

    click_text(&mut tab, &snap, area, "cancel");
    assert!(matches!(
        tab.run_monitor.as_ref().unwrap().sub_view,
        RunSubView::ConfirmCancel { .. }
    ));
    click_text(&mut tab, &snap, area, "Esc back");
    assert_eq!(tab.run_monitor.as_ref().unwrap().sub_view, RunSubView::Main);
}

#[test]
fn test_run_monitor_picker_row_click_selects_and_confirms() {
    let ws = workspace();
    let (_store, idea_id, run_id) = setup_run(ws.path());
    let snap = three_ideas();
    let mut tab = tab_for(ws.path());
    let mut monitor = FactoryRun::load(ws.path(), &idea_id, &run_id, "linux");
    monitor.sub_view = RunSubView::TargetPicker {
        choices: vec!["repo-a".to_string(), "repo-b".to_string()],
        selected: 0,
        error: None,
    };
    tab.run_monitor = Some(monitor);
    tab.view = FactoryView::RunMonitor;
    let area = Rect::new(0, 0, 100, 10);

    let mut buf = Buffer::empty(area);
    tab.render(&snap, area, &mut buf);
    let (x, y) = find(&buf, area, "repo-b");
    tab.handle_mouse(&click(x, y), &snap);
    let monitor = tab.run_monitor.as_ref().unwrap();
    assert!(
        !matches!(
            monitor.sub_view,
            RunSubView::TargetPicker { selected: 0, .. }
        ),
        "click must change selection or confirm"
    );
}

#[test]
fn test_status_app_routes_factory_mouse_events() {
    let ws = workspace();
    let (_store, _idea_id, _run_id) = setup_run(ws.path());
    let store = CoordStore::new(ws.path().join(".agent-workspace/coordination"));
    let mut app = StatusApp::new(
        PathBuf::from(ws.path()),
        store,
        None,
        Some(Config {
            factory: true,
            ..Config::default()
        }),
    );
    assert_eq!(app.tab, StatusTab::Factory);
    app.action_refresh_now();

    let area = Rect::new(0, 0, 140, 20);
    let mut buf = Buffer::empty(area);
    app.render(area, &mut buf);
    let (x, y) = find(&buf, area, "Export team");
    app.handle_mouse(&click(x, y));
    app.handle_mouse(&click(x, y));
    assert_eq!(app.factory_tab.view, FactoryView::Detail);

    let mut buf = Buffer::empty(area);
    app.render(area, &mut buf);
    let (bx, by) = find(&buf, area, "◀ Ideas");
    app.handle_mouse(&click(bx, by));
    assert_eq!(app.factory_tab.view, FactoryView::Board);
}

#[test]
fn test_narrow_widths_render_without_panic_and_keep_hits_in_bounds() {
    let ws = workspace();
    let snap = three_ideas();
    for width in [1u16, 5, 20, 40, 79, 80] {
        let mut tab = tab_for(ws.path());
        tab.sync_snapshot(&snap, false);
        let area = Rect::new(0, 0, width, 6);
        let mut buf = Buffer::empty(area);
        tab.render(&snap, area, &mut buf);
        for y in 0..6 {
            for x in 0..width {
                let _ = tab.handle_mouse(&click(x, y), &snap);
                tab.view = FactoryView::Board;
                tab.detail = None;
                tab.new_idea = None;
                tab.render(&snap, area, &mut buf);
            }
        }
    }
}
