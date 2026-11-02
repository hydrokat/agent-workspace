mod common;

use std::fs;

use common::TempDir;
use tandem::notice::{Notice, mark_seen, notice_path, read_notice, whats_new, write_notice};
use tandem::update_check::{
    CheckState, DEFAULT_INTERVAL, due, installed_version_from_manifest, latest_version, read_state,
    record, should_offer, write_state,
};

#[test]
fn test_read_notice_valid_and_seen() {
    let tmp = TempDir::new();
    let path = notice_path(tmp.path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();

    fs::write(
        &path,
        "TANDEM_UPDATE_FROM=1.2.0\n\
         TANDEM_UPDATE_TO=1.3.0\n\
         TANDEM_UPDATE_AT=1700000000\n",
    )
    .unwrap();

    let n = read_notice(tmp.path()).unwrap();
    assert_eq!(
        n,
        Notice {
            from_version: "1.2.0".to_string(),
            to_version: "1.3.0".to_string(),
            at: 1700000000,
            seen: false,
        }
    );

    mark_seen(tmp.path()).unwrap();
    let n_seen = read_notice(tmp.path()).unwrap();
    assert!(n_seen.seen);
}

#[test]
fn test_read_notice_invalid_lines_and_injections_rejected() {
    let tmp = TempDir::new();
    let path = notice_path(tmp.path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();

    assert!(read_notice(tmp.path()).is_none());

    let bad_cases = [
        "BAD LINE\n",
        "UNKNOWN_KEY=value\n",
        "TANDEM_UPDATE_TO=$(evil)\n",
        "TANDEM_UPDATE_TO=1.3.0; rm -rf /\n",
    ];

    for bad in bad_cases {
        let content = format!(
            "TANDEM_UPDATE_FROM=1.2.0\n\
             {}\
             TANDEM_UPDATE_AT=1700000000\n",
            bad
        );
        fs::write(&path, content).unwrap();
        assert!(
            read_notice(tmp.path()).is_none(),
            "Expected None for: {}",
            bad
        );
    }
}

#[test]
fn test_write_notice_atomic() {
    let tmp = TempDir::new();
    write_notice(tmp.path(), "1.2.0", "1.3.0", Some(1700000000)).unwrap();
    let n = read_notice(tmp.path()).unwrap();
    assert_eq!(n.from_version, "1.2.0");
    assert_eq!(n.to_version, "1.3.0");
    assert_eq!(n.at, 1700000000);
}

const CHANGELOG: &str = "\
# Changelog

## [Unreleased]
- Not yet in a release

## [1.3.0] - 2024-01-01
### Added
- **Clickable buttons**: click the status bar
- Click an agent tab to switch

## [1.2.0] - 2023-12-01
- Old feature. Details here.
";

#[test]
fn test_whats_new_extracts_headlines_and_fallbacks() {
    let list = whats_new(CHANGELOG, "1.2.0", "1.3.0", None);
    assert_eq!(
        list,
        vec![
            "Clickable buttons".to_string(),
            "Click an agent tab to switch".to_string()
        ]
    );

    let unreleased = whats_new(CHANGELOG, "2.0.0", "2.1.0", None);
    assert_eq!(unreleased, vec!["Not yet in a release".to_string()]);
}

#[test]
fn test_check_state_read_write_and_corrupt() {
    let tmp = TempDir::new();
    let state = CheckState {
        last: 123456.0,
        latest: "1.5.0".to_string(),
        skip: "1.4.9".to_string(),
    };
    write_state(tmp.path(), &state).unwrap();
    assert_eq!(read_state(tmp.path()), state);

    let path = tmp.path().join(".agent-workspace/update-check.env");
    fs::write(&path, "TANDEM_CHECK_LATEST=$(evil)\n").unwrap();
    assert_eq!(read_state(tmp.path()), CheckState::default());
}

#[test]
fn test_due_and_should_offer() {
    let state = CheckState {
        last: 1_000_000.0,
        latest: String::new(),
        skip: String::new(),
    };
    assert!(due(
        &state,
        1_000_000.0 + DEFAULT_INTERVAL as f64,
        DEFAULT_INTERVAL
    ));
    assert!(!due(
        &state,
        1_000_000.0 + DEFAULT_INTERVAL as f64 - 1.0,
        DEFAULT_INTERVAL
    ));

    assert!(should_offer(Some("1.3.0"), Some("1.4.0"), ""));
    assert!(!should_offer(Some("1.4.0"), Some("1.4.0"), ""));
    assert!(!should_offer(Some("1.4.0"), Some("1.3.0"), ""));
    assert!(!should_offer(Some("1.3.0"), Some("1.4.0"), "1.4.0"));
    assert!(should_offer(Some("1.3.0"), Some("1.5.0"), "1.4.0"));
    assert!(!should_offer(None, Some("1.4.0"), ""));
}

#[test]
fn test_record_persists_new_state() {
    let tmp = TempDir::new();
    let state = CheckState::default();
    let updated = record(tmp.path(), &state, 5000.0, Some("1.5.0")).unwrap();
    assert_eq!(updated.last, 5000.0);
    assert_eq!(updated.latest, "1.5.0");
    assert_eq!(read_state(tmp.path()), updated);
}

#[test]
fn test_latest_version_with_mock_runner() {
    let tmp = TempDir::new();
    let ver = latest_version("agent-workspace", tmp.path(), |_argv, _ws, _timeout| {
        Ok((0, "v1.5.0\n".to_string()))
    });
    assert_eq!(ver, Some("1.5.0".to_string()));

    let fail = latest_version("agent-workspace", tmp.path(), |_argv, _ws, _timeout| {
        Ok((1, "error".to_string()))
    });
    assert!(fail.is_none());
}

#[test]
fn test_installed_version_from_manifest() {
    let tmp = TempDir::new();
    assert!(installed_version_from_manifest(tmp.path()).is_none());

    let manifest_dir = tmp.path().join(".agent-workspace");
    fs::create_dir_all(&manifest_dir).unwrap();
    fs::write(
        manifest_dir.join("manifest.json"),
        r#"{"version": "1.5.0"}"#,
    )
    .unwrap();

    assert_eq!(
        installed_version_from_manifest(tmp.path()),
        Some("1.5.0".to_string())
    );
}
