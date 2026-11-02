use std::path::Path;

use serde_json::Value;

use tandem::updater::{StaleAgent, describe_stale, plan};

#[test]
fn test_updater_parity_fixture() {
    let fixture_str = include_str!("fixtures/parity/updater.json");
    let fixture: Value = serde_json::from_str(fixture_str).unwrap();

    let dummy_ws = Path::new("/dummy/ws");

    let actual_frozen = plan(dummy_ws, "1.5.0", Some(true));
    let expected_frozen: Vec<String> = fixture["plan_frozen"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(actual_frozen, expected_frozen);

    let actual_not_frozen = plan(dummy_ws, "1.5.0", Some(false));
    let expected_not_frozen: Vec<String> = fixture["plan_not_frozen"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(actual_not_frozen, expected_not_frozen);

    let sample_agent = StaleAgent {
        pane_id: "%1".to_string(),
        role: "planner".to_string(),
        index: 1,
        cli: "claude".to_string(),
        model: "opus".to_string(),
        launched: "1.4.0".to_string(),
        installed: "1.5.0".to_string(),
        resumable: true,
        busy: false,
        claim: String::new(),
    };
    let actual_describe = describe_stale(&sample_agent);
    let expected_describe = fixture["describe_stale"].as_str().unwrap();
    assert_eq!(actual_describe, expected_describe);
}
