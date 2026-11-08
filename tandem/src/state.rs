use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::adapters::base::{AgentSpec, validate_model_id};
use crate::conversations::valid_id;

pub const STATE_VERSION: u32 = 3;
pub const READABLE_VERSIONS: [u32; 3] = [1, 2, 3];
pub const CLIS: [&str; 2] = ["claude", "agy"];
pub const ROLES: [&str; 2] = ["planner", "implementer"];
pub const MAX_AGENTS: usize = 40;
pub const DEFAULT_SIDEBAR_COLS: usize = 26;
pub const DEFAULT_STATUS_ROWS: usize = 10;

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

fn default_clean() -> bool {
    true
}

fn default_sidebar_cols() -> usize {
    DEFAULT_SIDEBAR_COLS
}

fn default_status_rows() -> usize {
    DEFAULT_STATUS_ROWS
}

fn default_version() -> u32 {
    STATE_VERSION
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiState {
    #[serde(default = "default_sidebar_cols")]
    pub sidebar_cols: usize,
    #[serde(default = "default_status_rows")]
    pub status_rows: usize,
    #[serde(default)]
    pub selected: Vec<String>,
    #[serde(default)]
    pub grid_prefs: crate::tmux::grid::GridPrefs,
    #[serde(default)]
    pub sidebar_mode: crate::tmux::grid::SidebarMode,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            sidebar_cols: DEFAULT_SIDEBAR_COLS,
            status_rows: DEFAULT_STATUS_ROWS,
            selected: Vec::new(),
            grid_prefs: crate::tmux::grid::GridPrefs::default(),
            sidebar_mode: crate::tmux::grid::SidebarMode::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedState {
    #[serde(default = "default_version")]
    pub version: u32,
    pub agents: Vec<AgentSpec>,
    #[serde(default)]
    pub active: BTreeMap<String, usize>,
    #[serde(default)]
    pub columns_percent: Option<u32>,
    #[serde(default)]
    pub status_rows: Option<u32>,
    #[serde(default)]
    pub saved_at: f64,
    #[serde(default = "default_clean")]
    pub clean: bool,
    #[serde(default)]
    pub ui: UiState,
}

impl SavedState {
    pub fn new(
        agents: Vec<AgentSpec>,
        active: BTreeMap<String, usize>,
        columns_percent: Option<u32>,
        status_rows: Option<u32>,
        saved_at: f64,
    ) -> Self {
        let status_r = status_rows.unwrap_or(DEFAULT_STATUS_ROWS as u32) as usize;
        let mut selected = Vec::new();
        for role in ["planner", "implementer"] {
            if let Some(&idx) = active.get(role)
                && let Some(agent) = agents.iter().find(|a| a.role == role && a.index == idx)
            {
                if let Some(ref conv) = agent.conversation {
                    selected.push(conv.clone());
                } else {
                    selected.push(format!("{role}-{idx}"));
                }
            }
        }
        Self {
            version: STATE_VERSION,
            agents,
            active,
            columns_percent,
            status_rows,
            saved_at,
            clean: true,
            ui: UiState {
                sidebar_cols: DEFAULT_SIDEBAR_COLS,
                status_rows: status_r,
                selected,
                ..Default::default()
            },
        }
    }
}

pub fn state_path(workspace: &Path) -> PathBuf {
    workspace
        .join(".agent-workspace")
        .join("tandem-session.json")
}

pub fn to_json(state: &SavedState) -> serde_json::Value {
    let agents_json: Vec<serde_json::Value> = state
        .agents
        .iter()
        .map(|a| {
            serde_json::json!({
                "cli": a.cli,
                "role": a.role,
                "model": a.model,
                "index": a.index,
                "skip_permissions": a.skip_permissions,
                "cwd": a.cwd,
                "conversation": a.conversation,
            })
        })
        .collect();

    if state.version <= 2 {
        serde_json::json!({
            "version": 2,
            "saved_at": state.saved_at,
            "clean": state.clean,
            "agents": agents_json,
            "active": state.active,
            "layout": {
                "columns_percent": state.columns_percent,
                "status_rows": state.status_rows,
            },
        })
    } else {
        serde_json::json!({
            "version": STATE_VERSION,
            "saved_at": state.saved_at,
            "clean": state.clean,
            "agents": agents_json,
            "active": state.active,
            "layout": {
                "columns_percent": state.columns_percent,
                "status_rows": state.status_rows,
            },
            "ui": {
                "sidebar_cols": state.ui.sidebar_cols,
                "status_rows": state.ui.status_rows,
                "selected": state.ui.selected,
            },
        })
    }
}

pub fn discard(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

pub fn save(path: &Path, state: &SavedState) -> Result<(), StateError> {
    if state.saved_at.is_nan() || state.saved_at.is_infinite() {
        return Err(StateError::Invalid(
            "saved_at cannot be NaN or infinite".to_string(),
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".tandem-session-{}.tmp", uuid::Uuid::new_v4()));

    let json_val = to_json(state);
    let mut file = File::create(&tmp_path)?;
    serde_json::to_writer_pretty(&mut file, &json_val)?;
    file.write_all(b"\n")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));
    }

    fs::rename(&tmp_path, path)?;
    Ok(())
}

fn parse_agent(raw: &serde_json::Value) -> Option<AgentSpec> {
    let obj = raw.as_object()?;
    let cli = obj.get("cli")?.as_str()?;
    let role = obj.get("role")?.as_str()?;
    let model = obj.get("model")?.as_str()?;
    let index = obj.get("index")?.as_u64()?;

    if !CLIS.contains(&cli) || !ROLES.contains(&role) || !(1..=99).contains(&index) {
        return None;
    }

    let skip_permissions = obj
        .get("skip_permissions")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if cli == "claude" && skip_permissions {
        return None;
    }

    let cwd = match obj.get("cwd") {
        Some(v) if v.is_null() => None,
        Some(v) => {
            let s = v.as_str()?;
            if !s.starts_with('/') {
                return None;
            }
            Some(s.to_string())
        }
        None => None,
    };

    validate_model_id(model).ok()?;

    let conversation = obj
        .get("conversation")
        .and_then(|v| v.as_str())
        .and_then(valid_id);

    Some(AgentSpec {
        cli: cli.to_string(),
        role: role.to_string(),
        model: model.to_string(),
        index: index as usize,
        skip_permissions,
        cwd,
        conversation,
    })
}

fn parse_percent(raw: Option<&serde_json::Value>, low: u32, high: u32) -> Option<u32> {
    let val = raw?.as_u64()? as u32;
    if val >= low && val <= high {
        Some(val)
    } else {
        None
    }
}

pub fn parse(data: &serde_json::Value) -> Option<SavedState> {
    let obj = data.as_object()?;
    let version = obj.get("version")?.as_u64()? as u32;
    if !READABLE_VERSIONS.contains(&version) {
        return None;
    }

    let raw_agents = obj.get("agents")?.as_array()?;
    let mut seen = BTreeSet::new();
    let mut agents = Vec::new();

    for raw in raw_agents.iter().take(MAX_AGENTS) {
        if let Some(agent) = parse_agent(raw) {
            let key = (agent.role.clone(), agent.index);
            if !seen.contains(&key) {
                seen.insert(key);
                agents.push(agent);
            }
        }
    }

    if agents.is_empty() {
        return None;
    }

    let mut active = BTreeMap::new();
    if let Some(raw_active) = obj.get("active").and_then(|v| v.as_object()) {
        for (r, i_val) in raw_active {
            if ROLES.contains(&r.as_str())
                && let Some(idx) = i_val.as_u64()
            {
                let key = (r.clone(), idx as usize);
                if seen.contains(&key) {
                    active.insert(r.clone(), idx as usize);
                }
            }
        }
    }

    let layout = obj.get("layout").and_then(|v| v.as_object());
    let columns_percent = layout.and_then(|l| parse_percent(l.get("columns_percent"), 10, 90));
    let status_rows = layout.and_then(|l| parse_percent(l.get("status_rows"), 3, 60));

    let raw_ui = obj.get("ui").and_then(|v| v.as_object());
    let ui = if let Some(u) = raw_ui {
        let sidebar_cols = u
            .get("sidebar_cols")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_SIDEBAR_COLS as u64) as usize;
        let status_rows = u
            .get("status_rows")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_STATUS_ROWS as u64) as usize;
        let selected = u
            .get("selected")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let grid_prefs = u
            .get("grid_prefs")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let sidebar_mode = u
            .get("sidebar_mode")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        UiState {
            sidebar_cols,
            status_rows,
            selected,
            grid_prefs,
            sidebar_mode,
        }
    } else {
        let status_r = status_rows.unwrap_or(DEFAULT_STATUS_ROWS as u32) as usize;
        let mut selected = Vec::new();
        for role in ["planner", "implementer"] {
            if let Some(&idx) = active.get(role)
                && let Some(agent) = agents.iter().find(|a| a.role == role && a.index == idx)
            {
                if let Some(ref conv) = agent.conversation {
                    selected.push(conv.clone());
                } else {
                    selected.push(format!("{role}-{idx}"));
                }
            }
        }
        UiState {
            sidebar_cols: DEFAULT_SIDEBAR_COLS,
            status_rows: status_r,
            selected,
            ..Default::default()
        }
    };

    let saved_at = obj.get("saved_at").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let clean = obj.get("clean").and_then(|v| v.as_bool()).unwrap_or(true);
    let effective_version = if version <= 2 { 2 } else { STATE_VERSION };

    Some(SavedState {
        version: effective_version,
        agents,
        active,
        columns_percent,
        status_rows,
        saved_at,
        clean,
        ui,
    })
}

pub fn load(path: &Path) -> Option<SavedState> {
    let text = fs::read_to_string(path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&text).ok()?;
    parse(&val)
}

pub fn describe(state: &SavedState, now: Option<f64>) -> String {
    let mut lines = Vec::new();
    let title = if state.clean {
        "Saved session"
    } else {
        "tandem did not exit cleanly (crash, reboot or closed terminal). Last autosave"
    };

    let when = state.saved_at;
    if when > 0.0 {
        let current_time = now.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64()
        });
        let minutes = ((current_time - when).max(0.0) / 60.0).floor() as u64;
        if minutes < 120 {
            lines.push(format!("{title} ({minutes} min ago)"));
        } else {
            lines.push(format!("{title} ({} h ago)", minutes / 60));
        }
    } else {
        lines.push(title.to_string());
    }

    for role in ROLES {
        let mut mine: Vec<_> = state.agents.iter().filter(|a| a.role == role).collect();
        if !mine.is_empty() {
            mine.sort_by_key(|a| a.index);
            let parts: Vec<String> = mine
                .iter()
                .map(|a| {
                    if a.conversation.is_some() {
                        format!("{}·{} (conversation)", a.cli, a.model)
                    } else {
                        format!("{}·{}", a.cli, a.model)
                    }
                })
                .collect();
            lines.push(format!("  {role}s: {}", parts.join(", ")));
        }
    }

    if state.agents.iter().any(|a| a.conversation.is_some()) {
        lines.push("  (conversation) = resuming reopens that agent's chat".to_string());
    }

    lines.join("\n")
}
