mod common;

use common::TempDir;
use common::parity::load_fixture_value;
use tandem::conversations::{from_agy_log, valid_id};

#[test]
fn test_conversations_parity_fixture() {
    let fixture = load_fixture_value("conversations");

    let valid_id_map = fixture["valid_id"].as_object().unwrap();
    for (input, expected) in valid_id_map {
        let actual = valid_id(input);
        if expected.is_null() {
            assert!(
                actual.is_none(),
                "Expected None for input '{}', got {:?}",
                input,
                actual
            );
        } else {
            assert_eq!(
                actual.as_deref(),
                expected.as_str(),
                "Mismatch for input '{}'",
                input
            );
        }
    }

    let temp_dir = TempDir::new();
    let sample_file = temp_dir.path().join("sample.log");
    std::fs::write(
        &sample_file,
        "2026-11-01 info: starting agent\n\
         2026-11-01 info: Created conversation c0a80101-0000-0000-0000-000000000002\n\
         2026-11-01 info: running message\n",
    )
    .unwrap();
    let found = from_agy_log(&sample_file);
    assert_eq!(found.as_deref(), fixture["from_agy_log_sample"].as_str());

    let empty_file = temp_dir.path().join("empty.log");
    std::fs::write(&empty_file, "").unwrap();
    let found_empty = from_agy_log(&empty_file);
    assert!(found_empty.is_none());
}
