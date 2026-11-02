mod common;

use common::parity::load_fixture_value;
use tandem::update_check::{CheckState, due, should_offer};

#[test]
fn test_update_check_parity_fixture() {
    let fixture = load_fixture_value("update_check");

    let due_cases = fixture["due"].as_array().unwrap();
    for case in due_cases {
        let last = case["last"].as_f64().unwrap();
        let now = case["now"].as_f64().unwrap();
        let interval = case["interval"].as_i64().unwrap();
        let expected = case["expected"].as_bool().unwrap();

        let state = CheckState {
            last,
            latest: String::new(),
            skip: String::new(),
        };
        assert_eq!(
            due(&state, now, interval),
            expected,
            "Failed for case: {:?}",
            case
        );
    }

    let offer_cases = fixture["should_offer"].as_array().unwrap();
    for case in offer_cases {
        let installed = case["installed"].as_str();
        let latest = case["latest"].as_str();
        let skip = case["skip"].as_str().unwrap();
        let expected = case["expected"].as_bool().unwrap();

        assert_eq!(
            should_offer(installed, latest, skip),
            expected,
            "Failed for case: {:?}",
            case
        );
    }
}
