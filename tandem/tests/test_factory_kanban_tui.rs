use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tandem::factory::board::{Move, MoveGate, MoveVia};
use tandem::factory::model::{IdeaState, Risk};
use tandem::snapshot::{FactorySummary, IdeaCard, StatusSnapshot};
use tandem::status::factory_board::{BoardLayout, MIN_COLUMNS_WIDTH, MoveOutcome, move_outcome};
use tandem::status::factory_tab::{FactoryAction, FactoryTab, FactoryView, NewIdeaStep};

const NOW: f64 = 1_700_000_000.0;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ch(c: char) -> KeyEvent {
    key(KeyCode::Char(c))
}

fn click(col: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: col,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn ready_move() -> Move {
    Move {
        to: IdeaState::ReadyForDevelopment,
        via: MoveVia::Ready,
        needs_confirm: true,
        gate: Some(MoveGate::UserOnly),
    }
}

fn revoke_move() -> Move {
    Move {
        to: IdeaState::Planning,
        via: MoveVia::RevokeReady,
        needs_confirm: false,
        gate: Some(MoveGate::UserOnly),
    }
}

fn cancel_move() -> Move {
    Move {
        to: IdeaState::Cancelled,
        via: MoveVia::Cancel,
        needs_confirm: true,
        gate: None,
    }
}

fn card(id: &str, title: &str, state: IdeaState, moves: Vec<Move>) -> IdeaCard {
    IdeaCard {
        id: id.to_string(),
        title: title.to_string(),
        state,
        owner: "alice".to_string(),
        risk: Some(Risk::Low),
        blockers: 0,
        updated: NOW - 60.0,
        run: None,
        team_count: 0,
        needs_sign_in: false,
        moves,
    }
}

fn ideas() -> Vec<IdeaCard> {
    let mut list = ideas_unsorted();
    list[2].updated = NOW - 10.0;
    list
}

fn ideas_unsorted() -> Vec<IdeaCard> {
    vec![
        card(
            "i-inbox001",
            "Export CSV",
            IdeaState::Inbox,
            vec![cancel_move()],
        ),
        card(
            "i-plan0001",
            "Auth rework",
            IdeaState::Planning,
            vec![ready_move(), cancel_move()],
        ),
        card(
            "i-plan0002",
            "Dark mode",
            IdeaState::Planning,
            vec![ready_move(), cancel_move()],
        ),
        card(
            "i-ready001",
            "Billing page",
            IdeaState::ReadyForDevelopment,
            vec![revoke_move(), cancel_move()],
        ),
        card(
            "i-dev00001",
            "API docs",
            IdeaState::InDevelopment,
            vec![cancel_move()],
        ),
        card("i-done0001", "Old idea", IdeaState::Delivered, Vec::new()),
    ]
}

fn snapshot(ideas: Vec<IdeaCard>) -> StatusSnapshot {
    StatusSnapshot {
        now: NOW,
        factory: Some(FactorySummary {
            enabled: true,
            ideas,
            needs_you: 0,
            by_state: HashMap::new(),
            columns: Vec::new(),
            agents_active: 0,
            error: None,
        }),
        ..StatusSnapshot::default()
    }
}

fn text(buf: &Buffer, area: Rect) -> String {
    (area.y..area.y + area.height)
        .map(|y| {
            (area.x..area.x + area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render(tab: &mut FactoryTab, snap: &StatusSnapshot, width: u16, height: u16) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    tab.render(snap, area, &mut buf);
    text(&buf, area)
}

fn tab_for(snap: &StatusSnapshot) -> FactoryTab {
    let mut tab = FactoryTab::default();
    tab.sync_snapshot(snap, false);
    tab
}

fn labels(tab: &FactoryTab) -> Vec<String> {
    tab.move_picker
        .as_ref()
        .map(|p| p.labels())
        .unwrap_or_default()
}

#[test]
fn the_board_is_in_columns_by_default_and_v_toggles_the_list() {
    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    assert_eq!(tab.layout, BoardLayout::Columns);

    let out = render(&mut tab, &snap, 140, 14);
    assert!(out.contains("Inbox (1)"), "{out}");
    assert!(out.contains("Planning (2)"), "{out}");
    assert!(
        out.contains("Ready for development (1)") || out.contains("Ready (1)"),
        "{out}"
    );
    assert!(
        out.contains("Auth rework") && out.contains("Dark mode"),
        "{out}"
    );
    assert!(
        !out.contains("BLOCKERS"),
        "columns view has no list header: {out}"
    );

    tab.handle_key(&ch('v'), &snap);
    assert_eq!(tab.layout, BoardLayout::List);
    let list = render(&mut tab, &snap, 140, 14);
    assert!(list.contains("BLOCKERS"), "{list}");

    tab.handle_key(&ch('v'), &snap);
    assert_eq!(tab.layout, BoardLayout::Columns);
}

#[test]
fn counts_include_empty_columns_and_follow_the_filter() {
    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    let out = render(&mut tab, &snap, 200, 12);
    assert!(out.contains("Ideation (0)"), "{out}");
    assert!(out.contains("Review (0)"), "{out}");
    assert!(out.contains("Delivered (1)"), "{out}");
}

#[test]
fn h_and_l_move_between_columns_and_j_and_k_between_cards() {
    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    assert_eq!(tab.column, IdeaState::Inbox);
    assert_eq!(tab.selected_id.as_deref(), Some("i-inbox001"));

    tab.handle_key(&ch('l'), &snap);
    assert_eq!(tab.column, IdeaState::Ideation);
    assert_eq!(tab.selected_id, None, "an empty column selects nothing");

    tab.handle_key(&key(KeyCode::Right), &snap);
    assert_eq!(tab.column, IdeaState::Planning);
    assert_eq!(
        tab.selected_id.as_deref(),
        Some("i-plan0002"),
        "newest card first"
    );

    tab.handle_key(&ch('j'), &snap);
    assert_eq!(tab.selected_id.as_deref(), Some("i-plan0001"));
    tab.handle_key(&key(KeyCode::Down), &snap);
    assert_eq!(
        tab.selected_id.as_deref(),
        Some("i-plan0001"),
        "clamped at the last card"
    );
    tab.handle_key(&ch('k'), &snap);
    assert_eq!(tab.selected_id.as_deref(), Some("i-plan0002"));

    tab.handle_key(&ch('h'), &snap);
    tab.handle_key(&key(KeyCode::Left), &snap);
    assert_eq!(tab.column, IdeaState::Inbox);
    tab.handle_key(&ch('h'), &snap);
    assert_eq!(tab.column, IdeaState::Inbox, "clamped at the first column");
}

#[test]
fn focusing_an_empty_column_survives_a_refresh() {
    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    tab.handle_key(&ch('l'), &snap);
    assert_eq!(tab.selected_id, None);
    tab.sync_snapshot(&snap, false);
    assert_eq!(tab.column, IdeaState::Ideation);
    assert_eq!(tab.selected_id, None);
    let out = render(&mut tab, &snap, 140, 12);
    assert!(out.contains("Ideation (0)"), "{out}");
}

#[test]
fn the_move_picker_lists_only_the_moves_the_snapshot_offered() {
    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    tab.handle_key(&ch('l'), &snap);
    tab.handle_key(&ch('l'), &snap);
    tab.handle_key(&ch('m'), &snap);
    assert_eq!(
        labels(&tab),
        vec![
            "Ready for development (Gate 2)".to_string(),
            "Cancel idea".to_string()
        ]
    );
    let out = render(&mut tab, &snap, 140, 14);
    assert!(out.contains("Move Dark mode to"), "{out}");
    assert!(out.contains("Ready for development (Gate 2)"), "{out}");

    tab.handle_key(&key(KeyCode::Esc), &snap);
    assert!(tab.move_picker.is_none());
}

#[test]
fn read_only_cards_refuse_the_picker_with_a_reason() {
    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    tab.focus_idea("i-done0001");
    tab.sync_snapshot(&snap, false);
    tab.handle_key(&ch('m'), &snap);
    assert!(tab.move_picker.is_none());
    assert!(tab.notice.contains("no moves"), "{}", tab.notice);
}

#[test]
fn picking_ready_hands_over_to_the_gate_two_flow_and_changes_nothing_itself() {
    assert_eq!(move_outcome(&ready_move()), MoveOutcome::OpenReadyFlow);
    assert_eq!(move_outcome(&revoke_move()), MoveOutcome::OpenRevokeFlow);
    assert_eq!(move_outcome(&cancel_move()), MoveOutcome::ConfirmCancel);

    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    tab.workspace =
        Some(std::env::temp_dir().join(format!("tandem-kanban-{}", uuid::Uuid::new_v4())));
    tab.focus_idea("i-plan0001");
    tab.sync_snapshot(&snap, false);
    tab.handle_key(&ch('m'), &snap);
    let action = tab.handle_key(&key(KeyCode::Enter), &snap);
    assert_eq!(
        action, None,
        "Gate 2 is confirmed in the detail view, not here"
    );
    assert_eq!(tab.view, FactoryView::Detail);
    assert!(tab.move_picker.is_none());
}

#[test]
fn cancel_asks_for_confirmation_and_emits_the_action_only_on_yes() {
    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    tab.focus_idea("i-plan0001");
    tab.sync_snapshot(&snap, false);
    tab.handle_key(&ch('m'), &snap);
    tab.handle_key(&key(KeyCode::Down), &snap);
    assert_eq!(tab.handle_key(&key(KeyCode::Enter), &snap), None);
    let out = render(&mut tab, &snap, 140, 14);
    assert!(out.contains("Cancel idea i-plan0001"), "{out}");

    assert_eq!(tab.handle_key(&ch('n'), &snap), None, "n declines");
    assert!(tab.cancel_confirm.is_none());

    tab.handle_key(&ch('m'), &snap);
    tab.handle_key(&key(KeyCode::Down), &snap);
    tab.handle_key(&key(KeyCode::Enter), &snap);
    assert_eq!(
        tab.handle_key(&ch('y'), &snap),
        Some(FactoryAction::CancelIdea {
            idea_id: "i-plan0001".to_string()
        })
    );
    assert!(tab.cancel_confirm.is_none());
}

#[test]
fn narrow_terminals_fall_back_to_one_column_with_an_indicator() {
    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    let width = MIN_COLUMNS_WIDTH - 20;
    let out = render(&mut tab, &snap, width, 12);
    assert!(out.contains("Inbox (1)"), "{out}");
    assert!(out.contains("Export CSV"), "{out}");
    assert!(
        !out.contains("Auth rework"),
        "only the focused column is listed: {out}"
    );

    tab.handle_key(&ch('l'), &snap);
    tab.handle_key(&ch('l'), &snap);
    let planning = render(&mut tab, &snap, width, 12);
    assert!(planning.contains("Planning (2)"), "{planning}");
    assert!(
        planning.contains("Auth rework") && planning.contains("Dark mode"),
        "{planning}"
    );
    assert!(!planning.contains("Export CSV"), "{planning}");
}

#[test]
fn n_still_opens_the_new_idea_modal_and_f_still_cycles_the_filter() {
    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    let before = tab.filter;
    tab.handle_key(&ch('f'), &snap);
    assert_ne!(tab.filter, before);

    tab.handle_key(&ch('n'), &snap);
    assert_eq!(tab.view, FactoryView::NewIdea);
    assert_eq!(tab.new_idea.as_ref().unwrap().step, NewIdeaStep::Title);
}

#[test]
fn mouse_selects_a_card_and_a_second_click_opens_it() {
    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    tab.workspace =
        Some(std::env::temp_dir().join(format!("tandem-kanban-{}", uuid::Uuid::new_v4())));
    let out = render(&mut tab, &snap, 160, 14);
    let (row, col) = out
        .lines()
        .enumerate()
        .find_map(|(y, line)| line.find("Dark mode").map(|x| (y as u16, x as u16)))
        .expect("card visible");

    tab.handle_mouse(&click(col, row), &snap);
    assert_eq!(tab.selected_id.as_deref(), Some("i-plan0002"));
    assert_eq!(tab.column, IdeaState::Planning);
    assert_eq!(tab.view, FactoryView::Board);

    render(&mut tab, &snap, 160, 14);
    tab.handle_mouse(&click(col, row), &snap);
    assert_eq!(tab.view, FactoryView::Detail);
}

#[test]
fn clicking_a_column_header_focuses_that_column() {
    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    let out = render(&mut tab, &snap, 160, 14);
    let (row, col) = out
        .lines()
        .enumerate()
        .find_map(|(y, line)| line.find("Planning (2)").map(|x| (y as u16, x as u16)))
        .expect("header visible");
    tab.handle_mouse(&click(col, row), &snap);
    assert_eq!(tab.column, IdeaState::Planning);
}

#[test]
fn hints_mention_the_new_keys() {
    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    let out = render(&mut tab, &snap, 140, 14);
    assert!(out.contains("m move"), "{out}");
    assert!(out.contains("v list"), "{out}");
    assert!(out.contains("h/l column"), "{out}");
}

#[test]
fn the_title_step_requires_a_title_and_the_description_accepts_several_lines() {
    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    tab.open_new_idea();
    assert_eq!(tab.new_idea.as_ref().unwrap().step, NewIdeaStep::Title);

    tab.handle_key(&key(KeyCode::Enter), &snap);
    assert_eq!(tab.new_idea.as_ref().unwrap().step, NewIdeaStep::Title);
    assert!(
        tab.new_idea
            .as_ref()
            .unwrap()
            .message
            .contains("Title is required")
    );

    for c in "Export".chars() {
        tab.handle_key(&ch(c), &snap);
    }
    tab.handle_key(&key(KeyCode::Enter), &snap);
    assert_eq!(
        tab.new_idea.as_ref().unwrap().step,
        NewIdeaStep::Description
    );

    for c in "Weekly CSV".chars() {
        tab.handle_key(&ch(c), &snap);
    }
    tab.handle_key(&key(KeyCode::Enter), &snap);
    for c in "with totals".chars() {
        tab.handle_key(&ch(c), &snap);
    }
    assert_eq!(
        tab.new_idea.as_ref().unwrap().step,
        NewIdeaStep::Description
    );
    tab.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL), &snap);
    assert_eq!(tab.new_idea.as_ref().unwrap().step, NewIdeaStep::Cli);

    tab.handle_key(&key(KeyCode::Esc), &snap);
    assert_eq!(
        tab.new_idea.as_ref().unwrap().step,
        NewIdeaStep::Description,
        "Esc goes back one step"
    );
}

#[test]
fn the_description_is_validated_before_leaving_its_step() {
    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    tab.open_new_idea();
    for c in "T".chars() {
        tab.handle_key(&ch(c), &snap);
    }
    tab.handle_key(&key(KeyCode::Enter), &snap);
    for _ in 0..8001 {
        tab.handle_key(&ch('x'), &snap);
    }
    tab.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL), &snap);
    let modal = tab.new_idea.as_ref().unwrap();
    assert_eq!(modal.step, NewIdeaStep::Description);
    assert!(modal.message.contains("8000"), "{}", modal.message);
}

#[test]
fn the_start_action_carries_title_and_description() {
    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    tab.installed.insert("claude".to_string(), true);
    tab.open_new_idea();
    for c in "Export".chars() {
        tab.handle_key(&ch(c), &snap);
    }
    tab.handle_key(&key(KeyCode::Enter), &snap);
    for c in "Body".chars() {
        tab.handle_key(&ch(c), &snap);
    }
    tab.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL), &snap);
    assert_eq!(tab.new_idea.as_ref().unwrap().step, NewIdeaStep::Cli);
    tab.handle_key(&key(KeyCode::Enter), &snap);
    assert_eq!(tab.new_idea.as_ref().unwrap().step, NewIdeaStep::Model);
    for c in "opus".chars() {
        tab.handle_key(&ch(c), &snap);
    }
    let action = tab.handle_key(&key(KeyCode::Enter), &snap);
    assert_eq!(
        action,
        Some(FactoryAction::StartIdea {
            cli: "claude".to_string(),
            model: "opus".to_string(),
            title: Some("Export".to_string()),
            text: "Body".to_string(),
        })
    );
}

#[test]
fn clicking_a_column_header_while_the_picker_is_open_does_not_choose_a_move() {
    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    tab.focus_idea("i-plan0001");
    tab.sync_snapshot(&snap, false);
    tab.handle_key(&ch('m'), &snap);
    let out = render(&mut tab, &snap, 160, 14);
    let (row, col) = out
        .lines()
        .enumerate()
        .find_map(|(y, line)| line.find("Planning (2)").map(|x| (y as u16, x as u16)))
        .expect("header visible");

    assert_eq!(tab.handle_mouse(&click(col, row), &snap), None);
    assert!(tab.move_picker.is_some(), "the picker stays open");
    assert!(tab.cancel_confirm.is_none());
    assert_eq!(tab.view, FactoryView::Board);
}

#[test]
fn clicking_a_picker_option_chooses_that_move() {
    let snap = snapshot(ideas());
    let mut tab = tab_for(&snap);
    tab.focus_idea("i-plan0001");
    tab.sync_snapshot(&snap, false);
    tab.handle_key(&ch('m'), &snap);
    let out = render(&mut tab, &snap, 160, 14);
    let (row, col) = out
        .lines()
        .enumerate()
        .find_map(|(y, line)| line.find("Cancel idea").map(|x| (y as u16, x as u16)))
        .expect("option visible");

    let enter = tab.handle_mouse(&click(col, row), &snap);
    assert_eq!(enter, None, "the click is turned into Enter internally");
    assert!(
        tab.cancel_confirm.is_some(),
        "choosing Cancel asks for confirmation"
    );
}
