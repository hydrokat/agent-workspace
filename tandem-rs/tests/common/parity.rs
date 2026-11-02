use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};

#[allow(dead_code)]
pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("parity")
}

#[allow(dead_code)]
pub fn fixture_path(name: &str) -> PathBuf {
    fixtures_dir().join(format!("{name}.json"))
}

#[allow(dead_code)]
pub fn load_fixture_value(name: &str) -> serde_json::Value {
    let path = fixture_path(name);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("Failed to read fixture {:?}: {}", path, err));
    serde_json::from_str(&content)
        .unwrap_or_else(|err| panic!("Failed to parse fixture {:?} as JSON: {}", path, err))
}

#[allow(dead_code)]
pub fn load_fixture<T: DeserializeOwned>(name: &str) -> T {
    let path = fixture_path(name);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("Failed to read fixture {:?}: {}", path, err));
    serde_json::from_str(&content)
        .unwrap_or_else(|err| panic!("Failed to deserialize fixture {:?}: {}", path, err))
}
