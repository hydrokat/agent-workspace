mod common;

use common::parity::load_fixture_value;
use tandem::state::{describe, parse, to_json};

#[test]
fn test_state_parity_fixture() {
    let fixture = load_fixture_value("state");

    let parse_cases = fixture["parse"].as_object().unwrap();
    let good_state = parse(&parse_cases["good"]).expect("parse good state");
    assert_eq!(good_state.agents.len(), 2);
    assert_eq!(good_state.agents[0].cli, "claude");
    assert_eq!(good_state.agents[1].cli, "agy");
    assert_eq!(good_state.active.get("planner"), Some(&1));
    assert_eq!(good_state.active.get("implementer"), Some(&1));
    assert_eq!(good_state.columns_percent, Some(50));
    assert_eq!(good_state.status_rows, Some(10));

    let json_val = to_json(&good_state);
    assert_eq!(json_val, fixture["to_json"]);

    let partial_state = parse(&parse_cases["partial"]).expect("parse partial state");
    assert_eq!(partial_state.agents.len(), 1);
    assert_eq!(partial_state.agents[0].model, "opus");

    assert!(parse(&parse_cases["bad_version"]).is_none());
    assert!(parse(&parse_cases["bad_no_agents"]).is_none());

    let describe_cases = fixture["describe"].as_object().unwrap();

    let clean_desc = describe(&good_state, Some(1700000000.0 + 1000.0));
    assert_eq!(clean_desc, describe_cases["clean_16min"].as_str().unwrap());

    let mut unclean_state = good_state.clone();
    unclean_state.clean = false;
    let unclean_desc = describe(&unclean_state, Some(1700000000.0 + 7200.0));
    assert_eq!(
        unclean_desc,
        describe_cases["unclean_120min"].as_str().unwrap()
    );

    let mut zero_state = good_state.clone();
    zero_state.saved_at = 0.0;
    let zero_desc = describe(&zero_state, Some(1700000000.0));
    assert_eq!(zero_desc, describe_cases["zero_saved_at"].as_str().unwrap());
}
