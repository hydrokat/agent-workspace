mod common;

use common::TempDir;
use std::fs;
use std::path::Path;
use tandem::registry::{
    WorkspaceEntry, WorkspaceRegistry, default_registry_path, list_active, load_registry,
    register_or_heartbeat, remove, save_registry,
};
use tandem::tmux::client::socket_for;

#[test]
fn test_default_registry_path() {
    let base = Path::new("/custom/state/dir");
    let path = default_registry_path(base);
    assert_eq!(path, Path::new("/custom/state/dir/workspaces.json"));
}

#[test]
fn test_load_nonexistent_returns_empty_registry() {
    let temp = TempDir::new();
    let reg_path = temp.path().join("workspaces.json");
    let reg = load_registry(&reg_path).expect("should succeed on missing file");
    assert!(reg.workspaces.is_empty());
}

#[test]
fn test_save_and_load_roundtrip() {
    let temp = TempDir::new();
    let reg_path = temp.path().join("workspaces.json");

    let mut reg = WorkspaceRegistry::default();
    reg.workspaces.push(WorkspaceEntry {
        id: "tandem-1234567890".to_string(),
        name: "my-project".to_string(),
        path: "/path/to/my-project".to_string(),
        socket: "tandem-1234567890".to_string(),
        updated_at: 1794120000,
        pid: Some(12345),
    });

    save_registry(&reg_path, &reg).expect("save should succeed");
    assert!(reg_path.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::metadata(&reg_path).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }

    let loaded = load_registry(&reg_path).expect("load should succeed");
    assert_eq!(loaded.workspaces.len(), 1);
    assert_eq!(loaded.workspaces[0], reg.workspaces[0]);
}

#[test]
fn test_register_or_heartbeat_creates_and_updates() {
    let temp = TempDir::new();
    let state_dir = temp.path();
    let ws_dir = TempDir::new();
    let ws_path = ws_dir.path();

    let expected_socket = socket_for(ws_path);
    let expected_name = ws_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "workspace".to_string());

    let entry1 = register_or_heartbeat(state_dir, ws_path, Some(5555))
        .expect("initial registration should succeed");
    assert_eq!(entry1.id, expected_socket);
    assert_eq!(entry1.socket, expected_socket);
    assert_eq!(entry1.name, expected_name);
    assert_eq!(entry1.pid, Some(5555));
    assert!(entry1.updated_at > 0);

    let reg_path = default_registry_path(state_dir);
    let loaded1 = load_registry(&reg_path).expect("load should succeed");
    assert_eq!(loaded1.workspaces.len(), 1);
    assert_eq!(loaded1.workspaces[0].id, expected_socket);

    std::thread::sleep(std::time::Duration::from_millis(10));

    let entry2 =
        register_or_heartbeat(state_dir, ws_path, Some(5556)).expect("heartbeat should succeed");
    assert_eq!(entry2.id, expected_socket);
    assert_eq!(entry2.pid, Some(5556));
    assert!(entry2.updated_at >= entry1.updated_at);

    let loaded2 = load_registry(&reg_path).expect("load should succeed");
    assert_eq!(loaded2.workspaces.len(), 1);
    assert_eq!(loaded2.workspaces[0].pid, Some(5556));
}

#[test]
fn test_remove_workspace() {
    let temp = TempDir::new();
    let state_dir = temp.path();
    let ws_dir = TempDir::new();
    let ws_path = ws_dir.path();

    register_or_heartbeat(state_dir, ws_path, Some(1111)).expect("register");
    let reg_path = default_registry_path(state_dir);
    let loaded = load_registry(&reg_path).expect("load");
    assert_eq!(loaded.workspaces.len(), 1);

    remove(state_dir, ws_path).expect("remove should succeed");
    let loaded_after = load_registry(&reg_path).expect("load after remove");
    assert!(loaded_after.workspaces.is_empty());

    remove(state_dir, ws_path).expect("idempotent remove should not fail");
}

#[test]
fn test_list_active_filters_dead_sessions() {
    let temp = TempDir::new();
    let state_dir = temp.path();

    let mut reg = WorkspaceRegistry::default();
    reg.workspaces.push(WorkspaceEntry {
        id: "tandem-active1".to_string(),
        name: "active-proj".to_string(),
        path: "/path/active".to_string(),
        socket: "tandem-active1".to_string(),
        updated_at: 100,
        pid: Some(10),
    });
    reg.workspaces.push(WorkspaceEntry {
        id: "tandem-dead2".to_string(),
        name: "dead-proj".to_string(),
        path: "/path/dead".to_string(),
        socket: "tandem-dead2".to_string(),
        updated_at: 200,
        pid: Some(20),
    });

    let reg_path = default_registry_path(state_dir);
    save_registry(&reg_path, &reg).expect("save");

    let active = list_active(state_dir, |socket| socket == "tandem-active1")
        .expect("list_active should succeed");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, "tandem-active1");
}
