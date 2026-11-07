#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub fn baseline_fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("baseline")
}

pub fn baseline_fixture_path(relative_path: &str) -> PathBuf {
    baseline_fixtures_dir().join(relative_path)
}

pub fn assert_golden(relative_path: &str, actual: &str) {
    let path = baseline_fixture_path(relative_path);
    let should_update = std::env::var("UPDATE_GOLDEN")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    if should_update || !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .unwrap_or_else(|err| panic!("failed to create dir {:?}: {}", parent, err));
        }
        std::fs::write(&path, actual)
            .unwrap_or_else(|err| panic!("failed to write golden fixture {:?}: {}", path, err));
        return;
    }

    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read golden fixture {:?}: {}", path, err));

    if expected != actual {
        panic!(
            "Golden snapshot mismatch for {:?}!\n\n--- Expected ---\n{}\n--- Actual ---\n{}\n\nSet UPDATE_GOLDEN=1 to update if this change is intended and ledgered.",
            path, expected, actual
        );
    }
}
