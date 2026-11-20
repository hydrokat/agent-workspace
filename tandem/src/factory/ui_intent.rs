use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const INTENT_FILE: &str = "ui-intent.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum FactoryIntent {
    NewIdea,
    OpenIdea { idea_id: String },
}

fn intent_path(workspace: &Path) -> PathBuf {
    workspace
        .join(".agent-workspace")
        .join("factory")
        .join(INTENT_FILE)
}

pub fn write_intent(workspace: &Path, intent: &FactoryIntent) -> std::io::Result<()> {
    let path = intent_path(workspace);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_vec(intent).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, body)?;
    fs::rename(tmp, path)
}

pub fn take_intent(workspace: &Path) -> Option<FactoryIntent> {
    let path = intent_path(workspace);
    let meta = fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let text = fs::read_to_string(&path).ok();
    let _ = fs::remove_file(&path);
    serde_json::from_str(&text?).ok()
}
