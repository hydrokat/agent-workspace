mod common;

use common::parity::load_fixture_value;
use tandem::quota::{ClaudeQuota, Window, format_line_with_tz, tmux_safe};

#[test]
fn test_quota_parity_fixture() {
    let fixture = load_fixture_value("quota");

    let tmux_safe_cases = fixture["tmux_safe"].as_object().unwrap();
    for (input, expected) in tmux_safe_cases {
        assert_eq!(
            tmux_safe(input),
            expected.as_str().unwrap(),
            "tmux_safe failed for input: {:?}",
            input
        );
    }

    let now = 1700000000.0;
    let parse_cases = fixture["parse_statusline"].as_object().unwrap();

    let q_full: ClaudeQuota =
        serde_json::from_value(parse_cases["full"].clone()).expect("parse full");
    assert_eq!(
        q_full.five_hour,
        Some(Window {
            used_percentage: 35.0,
            resets_at: (now + 3600.0) as i64,
        })
    );
    assert_eq!(
        q_full.seven_day,
        Some(Window {
            used_percentage: 80.0,
            resets_at: (now + 86400.0 * 2.0) as i64,
        })
    );

    let q_five: ClaudeQuota =
        serde_json::from_value(parse_cases["five_only"].clone()).expect("parse five");
    assert!(q_five.five_hour.is_some());
    assert!(q_five.seven_day.is_none());

    assert!(parse_cases["empty"].as_bool().unwrap());
    assert!(parse_cases["invalid"].as_bool().unwrap());

    let fmt_cases = fixture["format_line"].as_object().unwrap();

    let line_fresh = format_line_with_tz(&["claude"], Some(&q_full), now, &chrono::Utc);
    assert_eq!(line_fresh, fmt_cases["claude_fresh"].as_str().unwrap());

    let line_stale = format_line_with_tz(&["claude"], Some(&q_full), now + 1000.0, &chrono::Utc);
    assert_eq!(line_stale, fmt_cases["claude_stale"].as_str().unwrap());

    let line_both = format_line_with_tz(&["claude", "agy"], Some(&q_full), now, &chrono::Utc);
    assert_eq!(line_both, fmt_cases["claude_and_agy"].as_str().unwrap());

    let line_agy = format_line_with_tz(&["agy"], None, now, &chrono::Utc);
    assert_eq!(line_agy, fmt_cases["agy_only"].as_str().unwrap());

    let line_none = format_line_with_tz(&[], None, now, &chrono::Utc);
    assert_eq!(line_none, fmt_cases["none"].as_str().unwrap());
}
