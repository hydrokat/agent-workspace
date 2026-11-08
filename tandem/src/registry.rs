use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::tmux::client::{resolve_path, socket_for};

pub const REGISTRY_FILE_NAME: &str = "workspaces.json";

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn resolve_state_dir() -> PathBuf {
    match std::env::var_os("XDG_STATE_HOME") {
        Some(raw) if !raw.is_empty() => {
            let p = PathBuf::from(raw);
            if p.is_absolute() {
                p.join("tandem")
            } else {
                home_dir().join(".local").join("state").join("tandem")
            }
        }
        _ => home_dir().join(".local").join("state").join("tandem"),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub socket: String,
    pub updated_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct WorkspaceRegistry {
    #[serde(default)]
    pub workspaces: Vec<WorkspaceEntry>,
}

pub fn default_registry_path(state_dir: &Path) -> PathBuf {
    state_dir.join(REGISTRY_FILE_NAME)
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn load_registry(path: &Path) -> Result<WorkspaceRegistry, RegistryError> {
    if !path.exists() {
        return Ok(WorkspaceRegistry::default());
    }

    let bytes = fs::read(path)?;
    if bytes.is_empty() {
        return Ok(WorkspaceRegistry::default());
    }

    let reg = serde_json::from_slice(&bytes)?;
    Ok(reg)
}

pub fn save_registry(path: &Path, registry: &WorkspaceRegistry) -> Result<(), RegistryError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".workspaces-{}.tmp", uuid::Uuid::new_v4()));

    let mut file = File::create(&tmp_path)?;
    serde_json::to_writer_pretty(&mut file, registry)?;
    file.write_all(b"\n")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));
    }

    fs::rename(&tmp_path, path)?;
    Ok(())
}

pub fn register_or_heartbeat(
    state_dir: &Path,
    workspace: &Path,
    pid: Option<u32>,
) -> Result<WorkspaceEntry, RegistryError> {
    let reg_path = default_registry_path(state_dir);
    let mut reg = load_registry(&reg_path).unwrap_or_default();

    let resolved = resolve_path(workspace);
    let socket = socket_for(&resolved);
    let id = socket.clone();
    let name = resolved
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "workspace".to_string());
    let path_str = resolved.to_string_lossy().to_string();
    let now = current_timestamp();

    let entry = WorkspaceEntry {
        id: id.clone(),
        name,
        path: path_str,
        socket,
        updated_at: now,
        pid,
    };

    if let Some(existing) = reg.workspaces.iter_mut().find(|w| w.id == id) {
        *existing = entry.clone();
    } else {
        reg.workspaces.push(entry.clone());
    }

    save_registry(&reg_path, &reg)?;
    Ok(entry)
}

pub fn remove(state_dir: &Path, workspace: &Path) -> Result<(), RegistryError> {
    let reg_path = default_registry_path(state_dir);
    if !reg_path.exists() {
        return Ok(());
    }

    let mut reg = match load_registry(&reg_path) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };

    let resolved = resolve_path(workspace);
    let target_id = socket_for(&resolved);
    let initial_len = reg.workspaces.len();
    reg.workspaces
        .retain(|w| w.id != target_id && w.socket != target_id);

    if reg.workspaces.len() != initial_len {
        save_registry(&reg_path, &reg)?;
    }

    Ok(())
}

pub fn is_tmux_socket_alive(socket: &str) -> bool {
    std::process::Command::new("tmux")
        .args([
            "-L",
            socket,
            "has-session",
            "-t",
            crate::tmux::layout::SESSION,
        ])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub fn list_active<F>(state_dir: &Path, is_active: F) -> Result<Vec<WorkspaceEntry>, RegistryError>
where
    F: Fn(&str) -> bool,
{
    let reg_path = default_registry_path(state_dir);
    let mut reg = load_registry(&reg_path)?;

    let initial_len = reg.workspaces.len();
    reg.workspaces.retain(|entry| is_active(&entry.socket));

    if reg.workspaces.len() != initial_len {
        let _ = save_registry(&reg_path, &reg);
    }

    Ok(reg.workspaces)
}
