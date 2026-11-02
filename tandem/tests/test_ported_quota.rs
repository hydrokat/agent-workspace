mod common;

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use common::TempDir;
use tandem::conversations::recorded;
use tandem::quota::{
    ClaudeQuota, Window, chain_with_timeout, format_line, parse_statusline, read_claude, run_feed,
    tmux_safe, user_statusline, write_feed,
};

const NOW: f64 = 1_800_000_000.0;

fn make_payload() -> serde_json::Value {
    serde_json::json!({
        "rate_limits": {
            "five_hour": {
                "used_percentage": 41.6,
                "resets_at": (NOW as i64) + 3600,
            },
            "seven_day": {
                "used_percentage": 12.0,
                "resets_at": (NOW as i64) + 5 * 86400,
            }
        },
        "model": {"display_name": "x"},
        "cost": {"total_cost_usd": 3},
    })
}

#[test]
fn test_parse_extracts_only_the_two_documented_windows() {
    let payload = make_payload();
    let q = parse_statusline(&payload, NOW).unwrap();
    assert_eq!(
        q.five_hour,
        Some(Window {
            used_percentage: 41.6,
            resets_at: (NOW as i64) + 3600,
        })
    );
    assert!(q.seven_day.is_some());
}

#[test]
fn test_parse_rejects_missing_or_malformed() {
    let bad_cases = vec![
        serde_json::Value::Null,
        serde_json::json!([]),
        serde_json::json!({}),
        serde_json::json!({"rate_limits": 5}),
        serde_json::json!({"rate_limits": {}}),
        serde_json::json!({"rate_limits": {"five_hour": {"used_percentage": "9", "resets_at": 1}}}),
        serde_json::json!({"rate_limits": {"five_hour": {"used_percentage": true, "resets_at": 1}}}),
        serde_json::json!({"rate_limits": {"five_hour": {"used_percentage": -1.0, "resets_at": 5}}}),
    ];
    for bad in bad_cases {
        assert!(parse_statusline(&bad, NOW).is_none());
    }
}

#[test]
fn test_one_window_is_enough() {
    let payload = serde_json::json!({
        "rate_limits": {
            "seven_day": {
                "used_percentage": 12.0,
                "resets_at": (NOW as i64) + 5 * 86400,
            }
        }
    });
    let q = parse_statusline(&payload, NOW).unwrap();
    assert!(q.five_hour.is_none());
    assert!(q.seven_day.is_some());
}

#[test]
fn test_feed_file_holds_only_percentages_and_is_private() {
    let tmp = TempDir::new();
    let q = parse_statusline(&make_payload(), NOW).unwrap();
    write_feed(tmp.path(), "planner-1", &q).unwrap();

    let path = tmp.path().join("quota").join("claude-planner-1.json");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains("cost"));
    assert!(!text.contains("model"));

    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    let map = parsed.as_object().unwrap();
    assert!(map.contains_key("updated"));
    assert!(map.contains_key("five_hour"));
    assert!(map.contains_key("seven_day"));
    assert_eq!(map.len(), 3);

    assert_eq!(read_claude(tmp.path()), Some(q));
}

#[test]
fn test_bad_session_id_is_refused() {
    let tmp = TempDir::new();
    let q = parse_statusline(&make_payload(), NOW).unwrap();
    assert!(write_feed(tmp.path(), "../evil", &q).is_err());
    assert!(!tmp.path().join("quota").exists());
}

#[test]
fn test_read_picks_freshest_and_survives_garbage() {
    let tmp = TempDir::new();
    let old = parse_statusline(&make_payload(), NOW - 100.0).unwrap();
    let new_payload = serde_json::json!({
        "rate_limits": {
            "five_hour": {
                "used_percentage": 90.0,
                "resets_at": (NOW as i64) + 60,
            }
        }
    });
    let new = parse_statusline(&new_payload, NOW).unwrap();

    write_feed(tmp.path(), "planner-1", &old).unwrap();
    write_feed(tmp.path(), "planner-2", &new).unwrap();

    let garbage = tmp.path().join("quota").join("claude-implementer-1.json");
    fs::write(garbage, "{nope").unwrap();

    assert_eq!(read_claude(tmp.path()), Some(new));
    assert!(read_claude(&tmp.path().join("missing")).is_none());
}

#[test]
fn test_line_shows_percentages_and_reset_times() {
    let q = parse_statusline(&make_payload(), NOW);
    let line = format_line(&["claude"], q.as_ref(), NOW);
    assert!(line.starts_with("Quota  claude: 5h 42% (resets "));
    assert!(line.contains("7d 12% (resets "));
}

#[test]
fn test_line_says_quota_not_available_for_agy_always() {
    assert_eq!(
        format_line(&["agy"], None, NOW),
        "Quota  agy: quota not available"
    );
    let both = format_line(&["claude", "agy"], None, NOW);
    assert_eq!(both.matches("quota not available").count(), 2);
}

#[test]
fn test_expired_windows_are_dropped_and_empty_means_not_available() {
    let q = ClaudeQuota {
        five_hour: Some(Window {
            used_percentage: 50.0,
            resets_at: (NOW as i64) - 1,
        }),
        seven_day: None,
        updated: NOW,
    };
    let line = format_line(&["claude"], Some(&q), NOW);
    assert!(line.contains("claude: quota not available"));
}

#[test]
fn test_stale_feed_is_labelled() {
    let q = parse_statusline(&make_payload(), NOW - 3600.0);
    let line = format_line(&["claude"], q.as_ref(), NOW);
    assert!(line.contains("[as of "));
}

#[test]
fn test_no_agents() {
    let line = format_line(&[], None, NOW);
    assert!(line.contains("quota not available (no agents running)"));
}

#[test]
fn test_output_cannot_inject_tmux_formats() {
    assert_eq!(
        tmux_safe("#[fg=red]#(rm -rf)\x1b[0m"),
        "##[fg=red]##(rm -rf)[0m"
    );
}

fn write_settings(path: &Path, command: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(
        path,
        serde_json::to_string(&serde_json::json!({
            "statusLine": {
                "type": "command",
                "command": command,
            }
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn test_user_statusline_precedence_local_project_user() {
    let tmp = TempDir::new();
    let cwd = tmp.path().join("p");
    let home = tmp.path().join("h");

    write_settings(&home.join(".claude/settings.json"), "echo user");
    assert_eq!(user_statusline(&cwd, &home), Some("echo user".to_string()));

    write_settings(&cwd.join(".claude/settings.json"), "echo project");
    assert_eq!(
        user_statusline(&cwd, &home),
        Some("echo project".to_string())
    );

    write_settings(&cwd.join(".claude/settings.local.json"), "echo local");
    assert_eq!(user_statusline(&cwd, &home), Some("echo local".to_string()));
}

#[test]
fn test_user_statusline_never_chains_to_itself() {
    let tmp = TempDir::new();
    write_settings(
        &tmp.path().join(".claude/settings.json"),
        "py -m tandem quota-feed",
    );
    assert!(user_statusline(tmp.path(), tmp.path()).is_none());
}

#[test]
fn test_feed_records_quota_then_runs_the_users_statusline() {
    let tmp = TempDir::new();
    let home = tmp.path().join("h");
    write_settings(
        &home.join(".claude/settings.json"),
        "cat >/dev/null; echo MY-LINE",
    );

    let out = run_feed(
        &make_payload().to_string(),
        &tmp.path().join("coord"),
        "planner-1",
        tmp.path(),
        &home,
        NOW,
    );
    assert_eq!(out.trim(), "MY-LINE");
    assert!(read_claude(&tmp.path().join("coord")).is_some());
}

#[test]
fn test_users_statusline_receives_the_same_input() {
    let tmp = TempDir::new();
    let home = tmp.path().join("h");
    write_settings(&home.join(".claude/settings.json"), "cat");

    let out = run_feed(
        "{\"a\": 1}",
        &tmp.path().join("c"),
        "planner-1",
        tmp.path(),
        &home,
        NOW,
    );
    assert_eq!(out, "{\"a\": 1}");
}

#[test]
fn test_feed_never_raises_on_bad_input() {
    let tmp = TempDir::new();
    for bad in ["", "not json", "[1]", "[[[[[[[[[["] {
        let out = run_feed(
            bad,
            &tmp.path().join("c"),
            "planner-1",
            tmp.path(),
            tmp.path(),
            NOW,
        );
        assert_eq!(out, "");
        assert!(!tmp.path().join("c/quota").exists());
    }
}

#[test]
fn test_feed_survives_unwritable_coord_dir() {
    let tmp = TempDir::new();
    let blocker = tmp.path().join("file");
    fs::write(&blocker, "x").unwrap();

    let out = run_feed(
        &make_payload().to_string(),
        &blocker.join("sub"),
        "planner-1",
        tmp.path(),
        tmp.path(),
        NOW,
    );
    assert_eq!(out, "");
}

#[test]
fn test_hung_user_statusline_is_cut_off() {
    let start = Instant::now();
    let out = chain_with_timeout(Some("sleep 5"), "{}", Duration::from_millis(300));
    assert_eq!(out, "");
    assert!(start.elapsed() < Duration::from_secs(3));
}

#[test]
fn test_feed_records_the_panes_current_conversation() {
    let tmp = TempDir::new();
    let conv = "46f2fc12-7f1f-4ca2-b714-06cbbfd669e9";
    let coord = tmp.path().join("coord");

    run_feed(
        &serde_json::json!({"session_id": conv}).to_string(),
        &coord,
        "planner-1",
        tmp.path(),
        tmp.path(),
        NOW,
    );
    assert_eq!(recorded(&coord, "planner-1"), Some(conv.to_string()));

    run_feed(
        &serde_json::json!({"session_id": "nope"}).to_string(),
        &coord,
        "planner-1",
        tmp.path(),
        tmp.path(),
        NOW,
    );
    assert_eq!(recorded(&coord, "planner-1"), Some(conv.to_string()));
}
