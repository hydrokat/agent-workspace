mod common;

use common::parity::load_fixture_value;
use tandem::tmux::layout::conf_text;
use tandem::tmux::tabs::{PaneInfo, render_title, tab_text};

#[test]
fn test_tmux_parity_fixture() {
    let fixture = load_fixture_value("tmux");

    let conf_cases = fixture["conf_text"].as_object().unwrap();
    assert_eq!(conf_text(50000), conf_cases["default"].as_str().unwrap());
    assert_eq!(
        conf_text(10000),
        conf_cases["history_10000"].as_str().unwrap()
    );

    let sample_panes = vec![
        PaneInfo {
            id: "%1".to_string(),
            window: "tandem".to_string(),
            kind: "agent".to_string(),
            role: "planner".to_string(),
            index: 1,
            label: "claude·opus".to_string(),
            dead: false,
            dead_status: None,
            session: "s-1".to_string(),
            spec: "spec1".to_string(),
            cli_version: "1.0".to_string(),
            name: String::new(),
        },
        PaneInfo {
            id: "%2".to_string(),
            window: "tandem".to_string(),
            kind: "agent".to_string(),
            role: "planner".to_string(),
            index: 2,
            label: "agy·sonnet".to_string(),
            dead: true,
            dead_status: Some(0),
            session: "s-2".to_string(),
            spec: "spec2".to_string(),
            cli_version: "1.0".to_string(),
            name: String::new(),
        },
        PaneInfo {
            id: "%3".to_string(),
            window: "tandem".to_string(),
            kind: "agent".to_string(),
            role: "implementer".to_string(),
            index: 1,
            label: "claude·sonnet".to_string(),
            dead: false,
            dead_status: None,
            session: "s-3".to_string(),
            spec: "spec3".to_string(),
            cli_version: "1.0".to_string(),
            name: String::new(),
        },
    ];

    let tabs_obj = fixture["tabs"].as_object().unwrap();
    let expected_tab_texts: Vec<String> =
        serde_json::from_value(tabs_obj["tab_text"].clone()).unwrap();
    let actual_tab_texts: Vec<String> = sample_panes.iter().map(tab_text).collect();
    assert_eq!(actual_tab_texts, expected_tab_texts);

    assert_eq!(
        render_title(&sample_panes, "planner"),
        tabs_obj["render_title_planner"].as_str().unwrap()
    );
    assert_eq!(
        render_title(&sample_panes, "implementer"),
        tabs_obj["render_title_implementer"].as_str().unwrap()
    );
}
