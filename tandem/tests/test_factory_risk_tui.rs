mod common;

use std::collections::HashMap;
use std::fs;

use common::TempDir;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tandem::config::Config;
use tandem::coordination::store::CoordStore;
use tandem::factory::auth_risk::{RiskItem, read_acks};
use tandem::factory::login::LoginStatusItem;
use tandem::factory::store::FactoryStore;
use tandem::snapshot::{FactorySummary, StatusSnapshot};
use tandem::status::StatusApp;
use tandem::status::factory_tab::{FactoryAction, FactoryTab, NewIdeaStep};
use tandem::status::risk_confirm::{RiskConfirm, RiskConfirmOutcome, risk_block_message};
use tandem::status::settings_tab::{SettingsAction, SettingsTab};

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

fn risky(cli: &str, method: &str, verdict: &str) -> RiskItem {
    RiskItem {
        cli: cli.to_string(),
        signed_in: true,
        method: method.to_string(),
        verdict: Some(verdict.to_string()),
        risk: Some("risky".to_string()),
        reason: Some(format!(
            "the terms verdict for {cli} with {method} sign-in is {verdict}"
        )),
        conditions: Some("Consumer Terms Section 3.7 prohibit automated access.".to_string()),
        acknowledged_on: None,
        verified_on: Some("2026-11-16".to_string()),
        reverify_by: Some("2027-02-14".to_string()),
        acknowledge_command: Some(format!(
            "tandem factory login acknowledge-risk {cli} --method {method} --confirm {cli}"
        )),
    }
}

fn ok(cli: &str, method: &str) -> RiskItem {
    RiskItem {
        cli: cli.to_string(),
        signed_in: true,
        method: method.to_string(),
        verdict: Some("allowed-with-conditions".to_string()),
        risk: Some("ok".to_string()),
        reason: None,
        conditions: None,
        acknowledged_on: None,
        verified_on: None,
        reverify_by: None,
        acknowledge_command: None,
    }
}

fn text(buf: &Buffer, area: Rect) -> String {
    (area.y..area.y + area.height)
        .map(|y| {
            (area.x..area.x + area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
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
    panic!("{needle:?} not on screen:\n{}", text(buf, area));
}

const AREA: Rect = Rect {
    x: 0,
    y: 0,
    width: 120,
    height: 40,
};

#[test]
fn the_confirm_modal_shows_the_verdict_and_needs_the_typed_cli_name() {
    let mut confirm = RiskConfirm::new(risky("claude", "subscription", "not-allowed"));
    let mut buf = Buffer::empty(AREA);
    confirm.render(AREA, &mut buf);
    let screen = text(&buf, AREA);
    assert!(
        screen.contains("Risky sign-in: claude (subscription)"),
        "{screen}"
    );
    assert!(screen.contains("NOT-ALLOWED"), "{screen}");
    assert!(screen.contains("Consumer Terms Section 3.7"), "{screen}");
    assert!(screen.contains("API key"), "{screen}");
    assert!(screen.contains("Type claude to use it anyway"), "{screen}");

    assert_eq!(
        confirm.handle_key(&key(KeyCode::Enter)),
        RiskConfirmOutcome::Pending
    );
    assert_eq!(confirm.error.as_deref(), Some("Type claude to confirm."));
    for c in "clau".chars() {
        confirm.handle_key(&ch(c));
    }
    assert_eq!(
        confirm.handle_key(&key(KeyCode::Enter)),
        RiskConfirmOutcome::Pending
    );
    confirm.handle_key(&ch('d'));
    confirm.handle_key(&ch('e'));
    assert_eq!(
        confirm.handle_key(&key(KeyCode::Enter)),
        RiskConfirmOutcome::Confirmed
    );
}

#[test]
fn the_confirm_modal_cancels_with_escape_and_its_buttons_are_clickable() {
    let mut confirm = RiskConfirm::new(risky("agy", "unknown", "unclear"));
    assert_eq!(
        confirm.handle_key(&key(KeyCode::Esc)),
        RiskConfirmOutcome::Cancelled
    );

    let mut buf = Buffer::empty(AREA);
    confirm.render(AREA, &mut buf);
    let (x, y) = find(&buf, AREA, "[Esc] Cancel");
    let esc = confirm
        .key_for_click(&click(x, y), AREA)
        .expect("cancel click");
    assert_eq!(esc.code, KeyCode::Esc);
    let (x, y) = find(&buf, AREA, "[Enter] I understand");
    let enter = confirm
        .key_for_click(&click(x, y), AREA)
        .expect("proceed click");
    assert_eq!(enter.code, KeyCode::Enter);
    assert!(confirm.key_for_click(&click(0, 0), AREA).is_none());
}

#[test]
fn risk_block_messages_only_exist_for_risky_unacknowledged_clis() {
    let mut acked = ok("opencode", "api-key");
    acked.risk = Some("acknowledged".to_string());
    let risks = vec![
        risky("claude", "subscription", "not-allowed"),
        ok("agy", "api-key"),
        acked,
    ];
    assert!(
        risk_block_message(&risks, "claude")
            .unwrap()
            .contains("risky sign-in")
    );
    assert!(risk_block_message(&risks, "agy").is_none());
    assert!(risk_block_message(&risks, "opencode").is_none());
    assert!(risk_block_message(&risks, "unknown").is_none());
}

fn settings_with(ws: &TempDir, risks: Vec<RiskItem>) -> SettingsTab {
    let mut tab = SettingsTab::default();
    tab.configure_factory(true, Some(ws.path().to_path_buf()));
    tab.factory_logins = vec![
        LoginStatusItem {
            cli: "claude".into(),
            installed: true,
            version: "2.1".into(),
            signed_in: true,
            method: Some("subscription".into()),
            ready: true,
            has_login_command: true,
            login_hint: None,
            error: None,
        },
        LoginStatusItem {
            cli: "agy".into(),
            installed: true,
            version: "1.5".into(),
            signed_in: true,
            method: None,
            ready: true,
            has_login_command: false,
            login_hint: None,
            error: None,
        },
    ];
    tab.set_risks(risks);
    tab
}

#[test]
fn settings_rows_show_each_clis_risk_and_a_review_action() {
    let ws = TempDir::new();
    let tab = settings_with(
        &ws,
        vec![
            risky("claude", "subscription", "not-allowed"),
            ok("agy", "api-key"),
        ],
    );
    let mut buf = Buffer::empty(AREA);
    tab.render(AREA, &mut buf);
    let screen = text(&buf, AREA);
    assert!(
        screen.contains("⚠ risky (not-allowed) [Review risk]"),
        "{screen}"
    );
}

#[test]
fn settings_a_key_and_a_click_on_review_risk_request_the_confirmation() {
    let ws = TempDir::new();
    let mut tab = settings_with(&ws, vec![risky("claude", "subscription", "not-allowed")]);
    tab.focus_agent_login("claude");
    assert_eq!(
        tab.handle_key(&ch('a')),
        Some(SettingsAction::ReviewRisk("claude".to_string()))
    );

    let mut buf = Buffer::empty(AREA);
    tab.render(AREA, &mut buf);
    let (x, y) = find(&buf, AREA, "[Review risk]");
    assert_eq!(
        tab.handle_mouse(&click(x + 2, y), AREA),
        Some(SettingsAction::ReviewRisk("claude".to_string()))
    );

    tab.focus_agent_login("agy");
    assert_eq!(tab.handle_key(&ch('a')), None, "agy has no risk entry yet");
}

fn snapshot() -> StatusSnapshot {
    StatusSnapshot {
        now: 1_700_000_000.0,
        factory: Some(FactorySummary {
            enabled: true,
            ideas: Vec::new(),
            needs_you: 0,
            by_state: HashMap::new(),
            columns: Vec::new(),
            agents_active: 0,
            error: None,
        }),
        ..StatusSnapshot::default()
    }
}

fn walk_to_cli_step(tab: &mut FactoryTab, snap: &StatusSnapshot) {
    tab.open_new_idea();
    for c in "Export".chars() {
        tab.handle_key(&ch(c), snap);
    }
    tab.handle_key(&key(KeyCode::Enter), snap);
    tab.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL), snap);
    assert_eq!(tab.new_idea.as_ref().unwrap().step, NewIdeaStep::Cli);
}

#[test]
fn the_new_idea_cli_step_blocks_a_risky_cli_and_offers_review() {
    let snap = snapshot();
    let mut tab = FactoryTab::default();
    tab.sync_snapshot(&snap, false);
    tab.installed.insert("claude".to_string(), true);
    tab.installed.insert("agy".to_string(), true);
    tab.set_risks(vec![
        risky("claude", "subscription", "not-allowed"),
        ok("agy", "api-key"),
    ]);
    walk_to_cli_step(&mut tab, &snap);

    let area = Rect::new(0, 0, 100, 14);
    let mut buf = Buffer::empty(area);
    tab.render(&snap, area, &mut buf);
    let screen = text(&buf, area);
    assert!(
        screen.contains("claude  ⚠ risky: subscription (not-allowed)"),
        "{screen}"
    );
    assert!(screen.contains("r review risk"), "{screen}");

    tab.handle_key(&key(KeyCode::Enter), &snap);
    let modal = tab.new_idea.as_ref().unwrap();
    assert_eq!(modal.step, NewIdeaStep::Cli, "a risky CLI cannot be chosen");
    assert!(modal.message.contains("risky sign-in"), "{}", modal.message);
    assert!(modal.message.contains("Press r"), "{}", modal.message);

    let action = tab.handle_key(&ch('r'), &snap);
    assert!(matches!(action, Some(FactoryAction::ReviewRisk { ref cli }) if cli == "claude"));

    tab.handle_key(&key(KeyCode::Down), &snap);
    assert_eq!(
        tab.handle_key(&ch('r'), &snap),
        None,
        "agy is ok: nothing to review"
    );
    tab.handle_key(&key(KeyCode::Enter), &snap);
    assert_eq!(tab.new_idea.as_ref().unwrap().step, NewIdeaStep::Model);
}

fn app_for(ws: &std::path::Path) -> StatusApp {
    let store = CoordStore::new(ws.join(".agent-workspace/coordination"));
    let mut app = StatusApp::new(
        ws.to_path_buf(),
        store,
        None,
        Some(Config {
            factory: true,
            ..Config::default()
        }),
    );
    app.factory_tab.layout = tandem::status::factory_board::BoardLayout::List;
    app
}

fn draw(app: &mut StatusApp) -> Buffer {
    let mut term = Terminal::new(TestBackend::new(AREA.width, AREA.height)).unwrap();
    term.draw(|f| app.render(f.area(), f.buffer_mut())).unwrap();
    term.backend().buffer().clone()
}

#[test]
fn confirming_in_the_app_records_a_user_acknowledgement_and_keeps_other_clis_risky() {
    let ws = TempDir::new();
    let store = FactoryStore::new(ws.path());
    store.ensure().unwrap();
    let mut app = app_for(ws.path());
    app.set_risks(vec![
        risky("claude", "subscription", "not-allowed"),
        risky("agy", "unknown", "unclear"),
    ]);

    app.open_risk_confirm("claude");
    assert!(app.risk_confirm.is_some());
    let screen = text(&draw(&mut app), AREA);
    assert!(
        screen.contains("Risky sign-in: claude (subscription)"),
        "{screen}"
    );

    app.handle_key(&key(KeyCode::Enter));
    assert!(app.risk_confirm.is_some(), "needs the typed name");
    for c in "claude".chars() {
        app.handle_key(&ch(c));
    }
    app.handle_key(&key(KeyCode::Enter));
    assert!(app.risk_confirm.is_none());

    let state = |cli: &str| {
        app.risks
            .iter()
            .find(|r| r.cli == cli)
            .and_then(|r| r.risk.clone())
    };
    assert_eq!(state("claude").as_deref(), Some("acknowledged"));
    assert_eq!(state("agy").as_deref(), Some("risky"));
    assert!(
        app.settings_tab
            .risks
            .iter()
            .any(|r| r.cli == "claude" && r.risk.as_deref() == Some("acknowledged"))
    );

    let acks = read_acks(&store);
    assert_eq!(acks.len(), 1, "{acks:?}");
    assert_eq!(acks[0].cli, "claude");
    assert_eq!(acks[0].method, "subscription");
    assert_eq!(acks[0].verdict, "not-allowed");

    let log = fs::read_to_string(ws.path().join(".agent-workspace/factory/events.jsonl")).unwrap();
    assert!(log.contains("auth.risk_acknowledged"));
}

#[test]
fn escape_closes_the_app_modal_without_recording_anything() {
    let ws = TempDir::new();
    let store = FactoryStore::new(ws.path());
    store.ensure().unwrap();
    let mut app = app_for(ws.path());
    app.set_risks(vec![risky("claude", "subscription", "not-allowed")]);
    app.open_risk_confirm("claude");
    app.handle_key(&key(KeyCode::Esc));
    assert!(app.risk_confirm.is_none());
    assert!(read_acks(&store).is_empty());
}

#[test]
fn the_app_refuses_to_start_an_idea_on_a_risky_cli_and_opens_the_confirmation() {
    let ws = TempDir::new();
    let store = FactoryStore::new(ws.path());
    store.ensure().unwrap();
    let mut app = app_for(ws.path());
    app.set_risks(vec![risky("claude", "subscription", "not-allowed")]);
    app.handle_factory_action(Some(FactoryAction::StartIdea {
        cli: "claude".to_string(),
        model: "opus".to_string(),
        title: Some("Export".to_string()),
        text: String::new(),
    }));
    assert!(app.risk_confirm.is_some());
    assert!(
        store.list_ideas().unwrap_or_default().is_empty(),
        "no idea may be created while the sign-in risk is unconfirmed"
    );
    assert!(app.factory_tab.notice.contains("risky sign-in"));
}

#[test]
fn a_click_on_the_modal_buttons_drives_the_confirmation() {
    let ws = TempDir::new();
    let mut app = app_for(ws.path());
    app.set_risks(vec![risky("claude", "subscription", "not-allowed")]);
    app.open_risk_confirm("claude");
    let buf = draw(&mut app);
    let (x, y) = find(&buf, AREA, "[Esc] Cancel");
    app.handle_mouse(&click(x, y));
    assert!(app.risk_confirm.is_none());
}
