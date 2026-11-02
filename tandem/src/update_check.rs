use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::versions::{is_at_least, parse_version};

pub type Runner =
    Arc<dyn Fn(&[&str], &Path, Duration) -> Result<(i32, String), std::io::Error> + Send + Sync>;

pub const CHECK_TIMEOUT: Duration = Duration::from_secs(60);
pub const DEFAULT_INTERVAL: i64 = 14400;
pub const MIN_INTERVAL: i64 = 600;

#[derive(Debug, Clone, PartialEq)]
pub struct CheckState {
    pub last: f64,
    pub latest: String,
    pub skip: String,
}

impl Default for CheckState {
    fn default() -> Self {
        Self {
            last: 0.0,
            latest: String::new(),
            skip: String::new(),
        }
    }
}

pub fn state_path(workspace: &Path) -> PathBuf {
    workspace.join(".agent-workspace").join("update-check.env")
}

fn is_valid_check_line(line: &str) -> bool {
    if let Some((key, val)) = line.split_once('=') {
        !key.is_empty()
            && key.chars().all(|c| c.is_ascii_uppercase() || c == '_')
            && val
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    } else {
        false
    }
}

pub fn read_state(workspace: &Path) -> CheckState {
    let text = match fs::read_to_string(state_path(workspace)) {
        Ok(t) => t,
        Err(_) => return CheckState::default(),
    };

    let allowed_keys: BTreeSet<&str> = [
        "TANDEM_CHECK_LAST",
        "TANDEM_CHECK_LATEST",
        "TANDEM_CHECK_SKIP",
    ]
    .into_iter()
    .collect();

    let mut last = 0.0;
    let mut latest = String::new();
    let mut skip = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !is_valid_check_line(trimmed) {
            return CheckState::default();
        }
        let Some((key, val)) = trimmed.split_once('=') else {
            return CheckState::default();
        };
        if !allowed_keys.contains(key) {
            return CheckState::default();
        }
        match key {
            "TANDEM_CHECK_LAST" => {
                if let Ok(num) = val.parse::<f64>() {
                    last = num;
                } else {
                    return CheckState::default();
                }
            }
            "TANDEM_CHECK_LATEST" => latest = val.to_string(),
            "TANDEM_CHECK_SKIP" => skip = val.to_string(),
            _ => return CheckState::default(),
        }
    }

    CheckState { last, latest, skip }
}

pub fn write_state(workspace: &Path, state: &CheckState) -> std::io::Result<()> {
    let path = state_path(workspace);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("env.tmp");
    let content = format!(
        "TANDEM_CHECK_LAST={}\nTANDEM_CHECK_LATEST={}\nTANDEM_CHECK_SKIP={}\n",
        state.last as i64, state.latest, state.skip
    );
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn due(state: &CheckState, now: f64, interval: i64) -> bool {
    now - state.last >= interval as f64
}

pub fn should_offer(installed: Option<&str>, latest: Option<&str>, skip: &str) -> bool {
    let inst = match installed {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };
    let lat = match latest {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };
    if lat == skip {
        return false;
    }
    let iv = match parse_version(inst) {
        Some(v) => v,
        None => return false,
    };
    let lv = match parse_version(lat) {
        Some(v) => v,
        None => return false,
    };
    !is_at_least(&iv, &lv)
}

pub fn record(
    workspace: &Path,
    state: &CheckState,
    now: f64,
    latest: Option<&str>,
) -> Result<CheckState, std::io::Error> {
    let new_state = CheckState {
        last: now,
        latest: latest
            .map(|s| s.to_string())
            .unwrap_or_else(|| state.latest.clone()),
        skip: state.skip.clone(),
    };
    write_state(workspace, &new_state)?;
    Ok(new_state)
}

pub fn latest_version<F>(spec: &str, workspace: &Path, runner: F) -> Option<String>
where
    F: Fn(&[&str], &Path, Duration) -> Result<(i32, String), std::io::Error>,
{
    let (code, out) = runner(
        &["npx", "--yes", spec, "--version"],
        workspace,
        CHECK_TIMEOUT,
    )
    .ok()?;
    if code != 0 {
        return None;
    }
    parse_version(&out).map(|v| v.text().to_string())
}

pub fn installed_version_from_manifest(workspace: &Path) -> Option<String> {
    let manifest = workspace.join(".agent-workspace").join("manifest.json");
    let text = fs::read_to_string(manifest).ok()?;
    let data: serde_json::Value = serde_json::from_str(&text).ok()?;
    let v = data.get("version")?.as_str()?;
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}
