mod common;

use std::fs;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tandem::config::Config;
use tandem::coordination::store::CoordStore;
use tandem::factory::broker::job::job_dir;
use tandem::factory::routing::ModelTier;
use tandem::factory::team::{AgentSummary, AssignmentSource, GlobalAgentSummary};
use tandem::snapshot::{FactorySummary, StatusSnapshot};
use tandem::status::factory_agents::{AgentFilter, AgentsKeyOutcome, FactoryAgents};
use tandem::status::factory_tab::{FactoryAction, FactoryTab, FactoryView};
use tandem::status::{StatusApp, StatusTab};

use common::TempDir;
use common::run_fixture::setup_run;

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

fn agent(
    idea: &str,
    title: &str,
    run: &str,
    stage: &str,
    state: &str,
    alive: bool,
) -> GlobalAgentSummary {
    GlobalAgentSummary {
        idea_id: idea.to_string(),
        idea_title: title.to_string(),
        tier: Some(ModelTier::Standard),
        agent: AgentSummary {
            job_id: format!("{run}.{stage}.1"),
            run_id: run.to_string(),
            stage: stage.to_string(),
            attempt: 1,
            cli: "claude".to_string(),
            model: "sonnet-5-5".to_string(),
            role: "senior-backend-dev".to_string(),
            source: AssignmentSource::Default,
            selection: None,
            job_state: state.to_string(),
            pane_id: Some("%21".to_string()),
            pane_alive: alive,
            activity: None,
            has_result: false,
            result_status: None,
        },
    }
}

fn agents() -> Vec<GlobalAgentSummary> {
    vec![
        agent(
            "i-7k2m9q4d",
            "Export CSV",
            "r-4h8k2m1x",
            "develop",
            "running",
            true,
        ),
        agent(
            "i-7k2m9q4d",
            "Export CSV",
            "r-4h8k2m1x",
            "plan",
            "finished",
            false,
        ),
        agent(
            "i-b81xw0pe",
            "Dark mode",
            "r-9z8y7x6w",
            "review",
            "running",
            false,
        ),
    ]
}

fn view(list: Vec<GlobalAgentSummary>) -> FactoryAgents {
    FactoryAgents::with_agents(PathBuf::from("/tmp/tandem-agents-view"), list)
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

fn render(v: &mut FactoryAgents, width: u16, height: u16) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    v.render(NOW, area, &mut buf);
    text(&buf, area)
}

#[test]
fn rows_show_idea_stage_cli_model_tier_state_and_age() {
    let mut v = view(agents());
    let out = render(&mut v, 130, 10);
    assert!(out.contains("Agents  2 running"), "{out}");
    for header in ["IDEA", "STAGE", "CLI/MODEL", "TIER", "STATE", "AGE"] {
        assert!(out.contains(header), "{header} in {out}");
    }
    assert!(out.contains("Export CSV"), "{out}");
    assert!(out.contains("develop"), "{out}");
    assert!(out.contains("claude/sonnet-5-5"), "{out}");
    assert!(out.contains("standard"), "{out}");
    assert!(out.contains("running"), "{out}");
    assert!(
        !out.contains("plan "),
        "finished jobs are hidden under Running: {out}"
    );
}

#[test]
fn f_toggles_between_running_and_all() {
    let mut v = view(agents());
    assert_eq!(v.filter, AgentFilter::Running);
    assert_eq!(v.visible().len(), 2);
    v.handle_key(&ch('f'));
    assert_eq!(v.filter, AgentFilter::All);
    assert_eq!(v.visible().len(), 3);
    let out = render(&mut v, 130, 10);
    assert!(out.contains("filter: All"), "{out}");
    assert!(out.contains("plan"), "{out}");
    v.handle_key(&ch('f'));
    assert_eq!(v.filter, AgentFilter::Running);
}

#[test]
fn a_running_job_whose_pane_is_gone_says_so() {
    let mut v = view(agents());
    let out = render(&mut v, 130, 10);
    assert!(out.contains("pane gone"), "{out}");
}

#[test]
fn the_empty_and_error_states_have_their_own_messages() {
    let mut empty = view(Vec::new());
    let out = render(&mut empty, 100, 8);
    assert!(
        out.contains("No agents running. Agents appear when a run starts a stage."),
        "{out}"
    );

    let mut failed = view(Vec::new());
    failed.error = Some("permission denied".to_string());
    let out = render(&mut failed, 100, 8);
    assert!(
        out.contains("could not list agents (permission denied)"),
        "{out}"
    );
}

#[test]
fn narrow_terminals_drop_age_then_tier() {
    let mut v = view(agents());
    let wide = render(&mut v, 130, 8);
    assert!(wide.contains("TIER") && wide.contains(" AGE"), "{wide}");
    let medium = render(&mut v, 90, 8);
    assert!(
        medium.contains("TIER") && !medium.contains(" AGE"),
        "{medium}"
    );
    let narrow = render(&mut v, 70, 8);
    assert!(
        !narrow.contains("TIER") && !narrow.contains(" AGE"),
        "{narrow}"
    );
    assert!(narrow.contains("STATE"), "{narrow}");
}

#[test]
fn selection_keys_and_enter_open_the_runs_monitor() {
    let mut v = view(agents());
    assert_eq!(v.selected, 0);
    v.handle_key(&ch('j'));
    assert_eq!(v.selected, 1);
    v.handle_key(&key(KeyCode::Down));
    assert_eq!(v.selected, 1, "clamped to the visible rows");
    v.handle_key(&ch('k'));
    assert_eq!(v.selected, 0);

    assert_eq!(
        v.handle_key(&key(KeyCode::Enter)),
        AgentsKeyOutcome::OpenRun {
            idea_id: "i-7k2m9q4d".to_string(),
            run_id: "r-4h8k2m1x".to_string(),
        }
    );
    assert_eq!(v.handle_key(&key(KeyCode::Esc)), AgentsKeyOutcome::Back);
}

#[test]
fn v_views_a_live_pane_and_refuses_a_closed_one() {
    let mut v = view(agents());
    assert_eq!(
        v.handle_key(&ch('v')),
        AgentsKeyOutcome::OpenWorkerPane {
            pane_id: "%21".to_string()
        }
    );
    v.handle_key(&ch('j'));
    assert_eq!(v.handle_key(&ch('v')), AgentsKeyOutcome::Consumed);
    assert!(v.notice.contains("pane is closed"), "{}", v.notice);
}

#[test]
fn message_and_stop_prompts_capture_input_and_validate() {
    let mut v = view(agents());
    assert!(!v.captures_input());
    v.handle_key(&ch('m'));
    assert!(v.captures_input());
    v.handle_key(&key(KeyCode::Enter));
    let out = render(&mut v, 120, 10);
    assert!(out.contains("Message must not be empty"), "{out}");
    v.handle_key(&key(KeyCode::Esc));
    assert!(!v.captures_input());

    v.handle_key(&ch('s'));
    assert!(v.captures_input());
    for c in "wrong".chars() {
        v.handle_key(&ch(c));
    }
    v.handle_key(&key(KeyCode::Enter));
    let out = render(&mut v, 120, 10);
    assert!(
        out.contains("Type r-4h8k2m1x.develop.1 to confirm stop"),
        "{out}"
    );
    v.handle_key(&key(KeyCode::Esc));
    assert!(!v.captures_input());
}

#[test]
fn mouse_selects_a_row_and_a_second_click_opens_the_run() {
    let mut v = view(agents());
    let out = render(&mut v, 130, 10);
    let (row, col) = out
        .lines()
        .enumerate()
        .find_map(|(y, line)| line.find("Dark mode").map(|x| (y as u16, x as u16)))
        .expect("row visible");
    assert_eq!(v.mouse_key(&click(col, row)), None);
    assert_eq!(v.selected, 1);
    render(&mut v, 130, 10);
    assert_eq!(v.mouse_key(&click(col, row)), Some(key(KeyCode::Enter)));
}

#[test]
fn rows_are_sorted_running_first() {
    let mut list = agents();
    list.reverse();
    let mut v = view(list);
    v.handle_key(&ch('f'));
    let states: Vec<String> = v
        .visible()
        .iter()
        .map(|a| a.agent.job_state.clone())
        .collect();
    assert_eq!(states, vec!["running", "running", "finished"]);
}

fn snapshot() -> StatusSnapshot {
    StatusSnapshot {
        now: NOW,
        factory: Some(FactorySummary {
            enabled: true,
            ..FactorySummary::default()
        }),
        ..StatusSnapshot::default()
    }
}

#[test]
fn g_opens_the_agents_view_from_the_board_and_esc_returns() {
    let ws = TempDir::new();
    let snap = snapshot();
    let mut tab = FactoryTab {
        workspace: Some(ws.path().to_path_buf()),
        ..FactoryTab::default()
    };
    tab.handle_key(&ch('g'), &snap);
    assert_eq!(tab.view, FactoryView::Agents);
    assert!(tab.agents.is_some());

    assert_eq!(tab.handle_key(&key(KeyCode::Esc), &snap), None);
    assert_eq!(tab.view, FactoryView::Board);
    assert!(tab.agents.is_none());
}

#[test]
fn g_without_a_workspace_explains_itself() {
    let snap = snapshot();
    let mut tab = FactoryTab::default();
    tab.handle_key(&ch('g'), &snap);
    assert_eq!(tab.view, FactoryView::Board);
    assert!(
        tab.notice.contains("Workspace path unavailable"),
        "{}",
        tab.notice
    );
}

#[test]
fn the_tab_routes_the_agent_keys_to_its_actions() {
    let ws = TempDir::new();
    let snap = snapshot();
    let mut tab = FactoryTab {
        workspace: Some(ws.path().to_path_buf()),
        ..FactoryTab::default()
    };
    tab.handle_key(&ch('g'), &snap);
    tab.agents = Some(FactoryAgents::with_agents(
        ws.path().to_path_buf(),
        agents(),
    ));

    assert_eq!(
        tab.handle_key(&ch('v'), &snap),
        Some(FactoryAction::OpenWorkerPane {
            pane_id: "%21".to_string()
        })
    );
    assert!(!tab.captures_input());
    tab.handle_key(&ch('m'), &snap);
    assert!(
        tab.captures_input(),
        "the message prompt must capture typing"
    );
    tab.handle_key(&key(KeyCode::Esc), &snap);
    assert_eq!(tab.view, FactoryView::Agents, "Esc closes the prompt first");

    tab.handle_key(&key(KeyCode::Enter), &snap);
    assert_eq!(tab.view, FactoryView::RunMonitor);
    tab.handle_key(&key(KeyCode::Esc), &snap);
    assert_eq!(
        tab.view,
        FactoryView::Agents,
        "back from the run returns to the list"
    );
}

#[test]
fn the_factory_tab_title_shows_how_many_agents_run() {
    let ws = TempDir::new();
    let (store, _idea, run_id) = setup_run(ws.path());
    let dir = job_dir(&store, &run_id, "develop", 1);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("state"), "running\n").unwrap();

    let store = CoordStore::new(ws.path().join(".agent-workspace/coordination"));
    let mut app = StatusApp::new(
        ws.path().to_path_buf(),
        store,
        None,
        Some(Config {
            factory: true,
            ..Config::default()
        }),
    );
    app.action_refresh_now();
    let titles = &app.tab_bar.titles;
    let factory = StatusTab::visible(true)
        .iter()
        .position(|t| *t == StatusTab::Factory)
        .unwrap();
    assert_eq!(titles[factory], "Factory (1)", "{titles:?}");
}
