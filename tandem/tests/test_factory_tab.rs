use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tandem::config::Config;
use tandem::coordination::store::CoordStore;
use tandem::factory::model::{IdeaState, Risk};
use tandem::snapshot::{FactorySummary, IdeaCard, StatusSnapshot};
use tandem::status::factory_tab::FactoryTab;
use tandem::status::{StatusApp, StatusTab};
use tandem::tmux::keys::{
    FACTORY_NEW_IDEA_KEY, FACTORY_TAB_KEY, default_keymap, help_lines, help_lines_with_factory,
    parse_shortcut,
};

fn temp_workspace(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tandem-test-factory-tab-{name}-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(path.join(".agent-workspace/coordination/queries")).unwrap();
    fs::create_dir_all(path.join("specs")).unwrap();
    path
}

fn buffer_text(buf: &Buffer, area: Rect) -> String {
    let mut lines = Vec::new();
    for y in area.y..area.y + area.height {
        let mut line = String::new();
        for x in area.x..area.x + area.width {
            line.push_str(buf[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

fn sample_snapshot(now: f64, ideas: Vec<IdeaCard>) -> StatusSnapshot {
    let mut by_state = HashMap::new();
    for idea in &ideas {
        *by_state
            .entry(idea.state.label().to_lowercase().replace(' ', "_"))
            .or_insert(0) += 1;
    }
    StatusSnapshot {
        now,
        factory: Some(FactorySummary {
            enabled: true,
            ideas,
            needs_you: 0,
            by_state,
            error: None,
        }),
        ..StatusSnapshot::default()
    }
}

fn sample_ideas(now: f64) -> Vec<IdeaCard> {
    vec![
        IdeaCard {
            id: "i-7k2m9q4d".to_string(),
            title: "Export team activity as CSV".to_string(),
            state: IdeaState::Inbox,
            owner: "you".to_string(),
            risk: None,
            blockers: 0,
            updated: now - 120.0,
            run: None,
        },
        IdeaCard {
            id: "i-b81xw0pe".to_string(),
            title: "Dark mode for reports".to_string(),
            state: IdeaState::Ideation,
            owner: "alice".to_string(),
            risk: Some(Risk::Medium),
            blockers: 0,
            updated: now - 3600.0,
            run: None,
        },
        IdeaCard {
            id: "i-c92yz1qf".to_string(),
            title: "Audit log\x1b[31m export\x1b[0m with very long title that should truncate cleanly at narrow widths".to_string(),
            state: IdeaState::InDevelopment,
            owner: "bob".to_string(),
            risk: Some(Risk::High),
            blockers: 0,
            updated: now - 30.0,
            run: None,
        },
    ]
}

#[test]
fn test_status_tab_visible_normal_vs_factory() {
    assert_eq!(StatusTab::ALL.len(), 7);
    assert_eq!(StatusTab::visible(false), &StatusTab::ALL);
    let factory_tabs = StatusTab::visible(true);
    assert_eq!(factory_tabs.len(), 8);
    assert_eq!(factory_tabs[7], StatusTab::Factory);
    assert_eq!(StatusTab::Settings.next_for(false), StatusTab::Status);
    assert_eq!(StatusTab::Settings.next_for(true), StatusTab::Factory);
    assert_eq!(StatusTab::Factory.next_for(true), StatusTab::Status);
    assert_eq!(StatusTab::Status.prev_for(false), StatusTab::Settings);
    assert_eq!(StatusTab::Status.prev_for(true), StatusTab::Factory);
}

#[test]
fn test_status_app_tabs_normal_vs_factory() {
    let ws = temp_workspace("tabs");
    let store = CoordStore::new(ws.join(".agent-workspace/coordination"));

    let mut normal_app = StatusApp::new(ws.clone(), store.clone(), None, Some(Config::default()));
    assert_eq!(normal_app.tab_bar.titles.len(), 7);
    normal_app.handle_key(&KeyEvent::new(KeyCode::Char('8'), KeyModifiers::empty()));
    assert_eq!(normal_app.tab, StatusTab::Status);

    let mut factory_app = StatusApp::new(
        ws.clone(),
        store,
        None,
        Some(Config {
            factory: true,
            ..Config::default()
        }),
    );
    assert_eq!(factory_app.tab_bar.titles.len(), 8);
    assert_eq!(factory_app.tab_bar.titles[7], "Factory");
    factory_app.handle_key(&KeyEvent::new(KeyCode::Char('8'), KeyModifiers::empty()));
    assert_eq!(factory_app.tab, StatusTab::Factory);

    let _ = fs::remove_dir_all(ws);
}

#[test]
fn test_factory_tab_render_wide_medium_narrow_short() {
    let now = 1_700_000_000.0;
    let snap = sample_snapshot(now, sample_ideas(now));
    let mut tab = FactoryTab::default();
    tab.sync(snap.factory.as_ref());

    // Wide (>= 120)
    let wide = Rect::new(0, 0, 125, 10);
    let mut buf = Buffer::empty(wide);
    tab.render(&snap, wide, &mut buf);
    let out_wide = buffer_text(&buf, wide);
    assert!(out_wide.contains("FACTORY   ideas 3   needs you 0"));
    assert!(out_wide.contains("OWNER"));
    assert!(out_wide.contains("▶i-7k2m9q4d"));
    assert!(out_wide.contains("2m ago"));
    assert!(out_wide.contains("↑/↓ move"));
    assert!(!out_wide.contains("\x1b[31m"));

    // Medium (80-119): OWNER dropped
    let med = Rect::new(0, 0, 95, 10);
    let mut buf = Buffer::empty(med);
    tab.render(&snap, med, &mut buf);
    let out_med = buffer_text(&buf, med);
    assert!(out_med.contains("FACTORY   ideas 3   needs you 0"));
    assert!(!out_med.contains("OWNER"));
    assert!(out_med.contains("RISK"));
    assert!(out_med.contains("▶i-7k2m9q4d"));

    // Narrow (< 80): two lines per idea
    let narrow = Rect::new(0, 0, 60, 10);
    let mut buf = Buffer::empty(narrow);
    tab.render(&snap, narrow, &mut buf);
    let out_narrow = buffer_text(&buf, narrow);
    assert!(out_narrow.contains("▶i-7k2m9q4d"));
    assert!(out_narrow.contains("0 blockers"));
    assert!(out_narrow.contains("Export team activity as CSV · 2m ago"));

    // Short (3 rows): hint line dropped first
    let short = Rect::new(0, 0, 125, 3);
    let mut buf = Buffer::empty(short);
    tab.render(&snap, short, &mut buf);
    let out_short = buffer_text(&buf, short);
    assert!(out_short.contains("FACTORY   ideas 3   needs you 0"));
    assert!(out_short.contains("▶i-7k2m9q4d"));
    assert!(!out_short.contains("↑/↓ move"));
}

#[test]
fn test_factory_tab_render_empty_and_error() {
    let now = 1_700_000_000.0;
    let empty_snap = sample_snapshot(now, Vec::new());
    let mut tab = FactoryTab::default();
    tab.sync(empty_snap.factory.as_ref());

    let area = Rect::new(0, 0, 90, 6);
    let mut buf = Buffer::empty(area);
    tab.render(&empty_snap, area, &mut buf);
    let out_empty = buffer_text(&buf, area);
    assert!(out_empty.contains("FACTORY   ideas 0"));
    assert!(out_empty.contains("No ideas yet."));
    assert!(out_empty.contains("tandem factory idea new"));

    let err_snap = StatusSnapshot {
        now,
        factory: Some(FactorySummary {
            enabled: true,
            ideas: Vec::new(),
            needs_you: 0,
            by_state: HashMap::new(),
            error: Some("permission denied".to_string()),
        }),
        ..StatusSnapshot::default()
    };
    let mut buf = Buffer::empty(area);
    tab.render(&err_snap, area, &mut buf);
    let out_err = buffer_text(&buf, area);
    assert!(out_err.contains("could not read .agent-workspace/factory (permission denied)"));
}

#[test]
fn test_factory_tab_selection_follows_idea_id_across_refresh() {
    let now = 1_700_000_000.0;
    let ideas = sample_ideas(now);
    let mut tab = FactoryTab::default();
    tab.sync(sample_snapshot(now, ideas.clone()).factory.as_ref());
    assert_eq!(tab.selected, Some(0));
    assert_eq!(tab.selected_id.as_deref(), Some("i-7k2m9q4d"));

    tab.select_next(&ideas);
    assert_eq!(tab.selected, Some(1));
    assert_eq!(tab.selected_id.as_deref(), Some("i-b81xw0pe"));

    let mut reordered = vec![IdeaCard {
        id: "i-new00001".to_string(),
        title: "Newly added idea".to_string(),
        state: IdeaState::Inbox,
        owner: "you".to_string(),
        risk: None,
        blockers: 0,
        updated: now,
        run: None,
    }];
    reordered.extend(ideas);

    let refreshed = sample_snapshot(now + 10.0, reordered);
    tab.sync(refreshed.factory.as_ref());
    assert_eq!(tab.selected, Some(2));
    assert_eq!(tab.selected_id.as_deref(), Some("i-b81xw0pe"));
}

#[test]
fn test_factory_shortcut_has_modifier_and_only_in_factory_help() {
    let (mods, _base) = parse_shortcut(FACTORY_TAB_KEY).expect("FACTORY_TAB_KEY must be valid");
    assert!(
        !mods.is_empty(),
        "Factory tab shortcut must require at least one modifier key"
    );

    let km = default_keymap();
    let normal_help = help_lines("linux", &km).join("\n");
    assert!(!normal_help.contains("Factory"));
    assert!(!normal_help.contains("1-8 direct"));

    let factory_help = help_lines_with_factory("linux", &km, true).join("\n");
    assert!(factory_help.contains("Factory"));
    assert!(factory_help.contains("Alt+f"));
    assert!(factory_help.contains("1-8 direct"));
}

#[test]
fn test_factory_tab_filter_cycles_and_renders_blockers_marker() {
    use tandem::coordination::schema::Query;
    use tandem::status::factory_tab::{FactoryTab, IdeaFilter};

    let now = 1_700_000_000.0;
    let mut ideas = sample_ideas(now);
    ideas[0].state = IdeaState::Planning;
    ideas[0].blockers = 1;

    let mut snap = sample_snapshot(now, ideas);
    snap.factory.as_mut().unwrap().needs_you = 1;
    snap.queries.push(Query {
        id: "q-001".to_string(),
        asked_by: "orchestrator".to_string(),
        task: "idea:i-7k2m9q4d".to_string(),
        question: "Which CSV columns?".to_string(),
        blocking_scope: "planning".to_string(),
        status: "open".to_string(),
        created: now - 60.0,
        options: Vec::new(),
        answer: String::new(),
        answered_via: String::new(),
    });

    let mut tab = FactoryTab::default();
    tab.sync_snapshot(&snap, false);

    let area = Rect::new(0, 0, 125, 10);
    let mut buf = Buffer::empty(area);
    tab.render(&snap, area, &mut buf);
    let out = buffer_text(&buf, area);
    assert!(out.contains("filter: All"));
    assert!(out.contains("needs you 1"));
    assert!(out.contains("1 ?"));

    // Cycle filter with 'f' -> Needs you
    tab.handle_key(
        &KeyEvent::new(KeyCode::Char('f'), KeyModifiers::empty()),
        &snap,
    );
    assert_eq!(tab.filter, IdeaFilter::NeedsYou);
    let mut buf = Buffer::empty(area);
    tab.render(&snap, area, &mut buf);
    let out = buffer_text(&buf, area);
    assert!(out.contains("filter: Needs you"));
    assert!(out.contains("i-7k2m9q4d"));
    assert!(!out.contains("i-b81xw0pe"));

    // Cycle to Inbox (0 matching -> filtered-to-empty message)
    tab.handle_key(
        &KeyEvent::new(KeyCode::Char('f'), KeyModifiers::empty()),
        &snap,
    );
    assert_eq!(tab.filter, IdeaFilter::State(IdeaState::Inbox));
    let mut buf = Buffer::empty(area);
    tab.render(&snap, area, &mut buf);
    let out = buffer_text(&buf, area);
    assert!(out.contains("filter: Inbox"));
    assert!(out.contains("No ideas match filter"));
}

#[test]
fn test_factory_tab_new_idea_picker_cli_then_model_returns_start_action() {
    use std::sync::Arc;
    use tandem::adapters::base::Model;
    use tandem::status::factory_tab::{FactoryAction, FactoryTab, FactoryView, NewIdeaStep};

    let now = 1_700_000_000.0;
    let snap = sample_snapshot(now, sample_ideas(now));

    let mut installed = HashMap::new();
    installed.insert("claude".to_string(), true);
    installed.insert("agy".to_string(), false);

    let loader = Arc::new(|cli: &str| {
        assert_eq!(cli, "claude");
        Ok(vec![
            Model {
                id: "opus".to_string(),
                label: "opus (latest)".to_string(),
            },
            Model {
                id: "sonnet".to_string(),
                label: "sonnet".to_string(),
            },
        ])
    });

    let mut tab = FactoryTab::new(None, installed, loader);
    tab.sync_snapshot(&snap, false);

    // Open new idea modal with Ctrl+Alt+n
    let (mods, _) = parse_shortcut(FACTORY_NEW_IDEA_KEY).unwrap();
    assert!(!mods.is_empty());
    tab.handle_key(
        &KeyEvent::new(
            KeyCode::Char('n'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        ),
        &snap,
    );
    assert_eq!(tab.view, FactoryView::NewIdea);

    let area = Rect::new(0, 0, 80, 14);
    let mut buf = Buffer::empty(area);
    tab.render(&snap, area, &mut buf);
    let out = buffer_text(&buf, area);
    assert!(out.contains("New idea"));
    assert!(out.contains("claude"));
    assert!(out.contains("agy  (not installed)"));

    // Select claude -> moves to Model step
    tab.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()), &snap);
    assert_eq!(
        tab.new_idea.as_ref().map(|m| m.step),
        Some(NewIdeaStep::Model)
    );

    // Wait briefly for background model loader
    for _ in 0..50 {
        if tab.poll_background() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let mut buf = Buffer::empty(area);
    tab.render(&snap, area, &mut buf);
    let out = buffer_text(&buf, area);
    assert!(out.contains("opus (latest)"));

    // Confirm model -> emits StartIdea action
    let action = tab.handle_key(&KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()), &snap);
    assert_eq!(
        action,
        Some(FactoryAction::StartIdea {
            cli: "claude".to_string(),
            model: "opus".to_string(),
        })
    );
    assert_eq!(tab.view, FactoryView::Board);
}
