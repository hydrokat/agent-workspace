mod common;

use common::parity::load_fixture_value;
use tandem::notice::{headline, whats_new};

#[test]
fn test_notice_parity_fixture() {
    let fixture = load_fixture_value("notice");

    let h_bold = headline("- **Major Rust port foundations**: initial work.");
    assert_eq!(h_bold, fixture["whats_new_headline_bold"].as_str().unwrap());

    let h_period = headline("- Some sentence. More text.");
    assert_eq!(
        h_period,
        fixture["whats_new_headline_period"].as_str().unwrap()
    );

    let sample_changelog = "\
# Changelog

## [1.5.0] - 2026-11-01
- **Major Rust port foundations**: initial work.
- Second bullet item without bold.
";
    let list = whats_new(sample_changelog, "1.4.0", "1.5.0", None);
    let expected: Vec<String> = serde_json::from_value(fixture["whats_new_text"].clone()).unwrap();
    assert_eq!(list, expected);
}
