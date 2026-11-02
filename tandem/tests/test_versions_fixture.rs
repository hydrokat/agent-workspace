mod common;

use common::parity::load_fixture_value;
use tandem::versions::{is_at_least, parse_min_versions_str, parse_version};

#[test]
fn test_versions_parity_fixture() {
    let fixture = load_fixture_value("versions");

    let parse_version_cases = fixture["parse_version"]
        .as_object()
        .expect("parse_version map");
    for (input, expected) in parse_version_cases {
        let actual = parse_version(input);
        if expected.is_null() {
            assert_eq!(actual, None, "Expected None for input '{}'", input);
        } else {
            let actual = actual.unwrap_or_else(|| panic!("Expected Some for input '{}'", input));
            let expected_parts: Vec<u32> = expected["parts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect();
            let expected_suffix = expected["suffix"].as_str().unwrap();

            assert_eq!(
                actual.parts, expected_parts,
                "Parts mismatch for '{}'",
                input
            );
            assert_eq!(
                actual.suffix, expected_suffix,
                "Suffix mismatch for '{}'",
                input
            );
        }
    }

    let is_at_least_cases = fixture["is_at_least"].as_object().expect("is_at_least map");
    for (pair_str, expected) in is_at_least_cases {
        let (a_str, b_str) = pair_str.split_once(">=").expect("split pair");
        let va = parse_version(a_str).unwrap();
        let vb = parse_version(b_str).unwrap();
        let expected_bool = expected.as_bool().unwrap();
        assert_eq!(
            is_at_least(&va, &vb),
            expected_bool,
            "is_at_least failed for '{}'",
            pair_str
        );
    }

    let min_versions_map = fixture["parse_min_versions"]
        .as_object()
        .expect("parse_min_versions map");
    let sample_env = "# Minimum versions required\nTANDEM_MIN_NODE=18.0.0\nTANDEM_MIN_PYTHON=3.10.0\nTANDEM_MIN_TMUX=3.2\n";
    let actual_min = parse_min_versions_str(sample_env, "min-versions.env").unwrap();
    for (k, v) in min_versions_map {
        assert_eq!(
            actual_min.get(k).map(|s| s.as_str()),
            Some(v.as_str().unwrap()),
            "Key '{}' mismatch in parse_min_versions",
            k
        );
    }
}
