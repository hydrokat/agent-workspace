mod common;

use common::parity::{fixture_path, load_fixture, load_fixture_value};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct VersionsFixture {
    parse_version: std::collections::BTreeMap<String, Option<ParsedVersion>>,
    is_at_least: std::collections::BTreeMap<String, bool>,
}

#[derive(Debug, Deserialize)]
struct ParsedVersion {
    parts: Vec<u32>,
    suffix: String,
}

#[test]
fn test_all_13_fixtures_exist_and_load_as_json() {
    let fixture_names = [
        "keys",
        "tmux",
        "adapters",
        "state",
        "quota",
        "render",
        "sources",
        "versions",
        "update_check",
        "notice",
        "conversations",
        "coordination",
        "updater",
    ];

    for name in fixture_names {
        let path = fixture_path(name);
        assert!(path.exists(), "Fixture file {:?} must exist", path);
        let val = load_fixture_value(name);
        assert!(
            val.is_object(),
            "Fixture {} must deserialize to a JSON object",
            name
        );
    }
}

#[test]
fn test_typed_fixture_loader() {
    let versions: VersionsFixture = load_fixture("versions");
    assert!(versions.is_at_least.contains_key("1.5.0>=1.4.0"));
    assert_eq!(versions.is_at_least["1.5.0>=1.4.0"], true);
    assert_eq!(versions.is_at_least["1.4.9>=1.5.0"], false);

    let v150 = versions.parse_version["1.5.0"]
        .as_ref()
        .expect("parsed 1.5.0");
    assert_eq!(v150.parts, vec![1, 5, 0]);
    assert_eq!(v150.suffix, "");
}

#[test]
fn test_readme_exists_in_parity_dir() {
    let readme_path = common::parity::fixtures_dir().join("README.md");
    assert!(
        readme_path.exists(),
        "README.md must exist in parity directory"
    );
    let content = std::fs::read_to_string(readme_path).expect("read parity README.md");
    assert!(content.contains("DO NOT edit these fixture files directly"));
}
