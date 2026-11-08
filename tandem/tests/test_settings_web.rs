use std::fs;
use tandem::web_manager::{WebConfig, load_web_config, save_web_config};

#[test]
fn test_web_config_defaults_and_roundtrip() {
    let tmp_dir = std::env::temp_dir().join(format!("tandem-web-cfg-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp_dir).unwrap();

    let default_cfg = WebConfig::default();
    assert!(!default_cfg.enabled, "web must be off by default");
    assert!(!default_cfg.first_time_acknowledged);
    assert_eq!(default_cfg.port, 3940);
    assert!(!default_cfg.auth_token.is_empty());

    let cfg_path = tmp_dir.join("web-config.json");
    save_web_config(&cfg_path, &default_cfg).expect("save must succeed");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = fs::metadata(&cfg_path).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    }

    let loaded = load_web_config(&cfg_path).expect("load must succeed");
    assert_eq!(loaded, default_cfg);

    let _ = fs::remove_dir_all(tmp_dir);
}

#[test]
fn test_system_daemon_runner_and_lifecycle() {
    use tandem::web_manager::{
        DaemonRunner, SystemDaemonRunner, WebStatus, is_pid_alive, web_config_path, web_pid_path,
    };

    let tmp_dir = std::env::temp_dir().join(format!("tandem-daemon-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp_dir).unwrap();

    let cfg_path = web_config_path(&tmp_dir);
    assert!(cfg_path.ends_with("web-config.json"));

    let pid_path = web_pid_path(&tmp_dir);
    assert!(pid_path.ends_with("tandem-web.pid"));

    // Empty or non-existent file returns default config
    let loaded_missing = load_web_config(&cfg_path).unwrap();
    assert!(!loaded_missing.enabled);
    assert_eq!(loaded_missing.port, 3940);
    assert!(!loaded_missing.auth_token.is_empty());

    fs::write(&cfg_path, b"").unwrap();
    let loaded_empty = load_web_config(&cfg_path).unwrap();
    assert!(!loaded_empty.enabled);
    assert_eq!(loaded_empty.port, 3940);
    assert!(!loaded_empty.auth_token.is_empty());

    // Invalid JSON returns Err
    fs::write(&cfg_path, b"{ invalid json").unwrap();
    assert!(load_web_config(&cfg_path).is_err());

    let runner = SystemDaemonRunner;

    // Disabled status
    let config_off = WebConfig {
        enabled: false,
        ..Default::default()
    };
    assert_eq!(runner.status(&config_off, &tmp_dir), WebStatus::Off);

    // Enabled with no running daemon
    let config = WebConfig {
        enabled: true,
        ..Default::default()
    };
    assert_eq!(runner.status(&config, &tmp_dir), WebStatus::Off);

    // Write a dummy stale pid
    fs::write(&pid_path, "9999999\n").unwrap();
    assert!(!is_pid_alive(9999999));
    // Status clears dead pid
    assert_eq!(runner.status(&config, &tmp_dir), WebStatus::Off);
    assert!(!pid_path.exists());

    // Spawn a dummy process to test live PID and stop_daemon
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .expect("sleep should spawn");
    let child_pid = child.id();
    assert!(is_pid_alive(child_pid));
    fs::write(&pid_path, format!("{child_pid}\n")).unwrap();

    let running_status = runner.status(&config, &tmp_dir);
    match running_status {
        WebStatus::Running { url, pid } => {
            assert!(url.contains(&config.bind_ip));
            assert_eq!(pid, Some(child_pid));
        }
        other => panic!("expected Running status, got {:?}", other),
    }

    // Existing pid returns immediately from start_daemon
    let existing = runner
        .start_daemon(&config, &tmp_dir, &tmp_dir)
        .expect("should return existing pid");
    assert_eq!(existing, child_pid);

    // Stop daemon terminates the child process and removes pid file
    runner.stop_daemon(&tmp_dir).expect("stop should succeed");
    assert!(!pid_path.exists());
    let _ = child.kill();
    let _ = child.wait();

    // Stop daemon when no pid file exists succeeds gracefully
    runner
        .stop_daemon(&tmp_dir)
        .expect("stop on clean state should succeed");

    assert_ne!(WebStatus::Off, WebStatus::Starting);
    assert_ne!(WebStatus::Starting, WebStatus::Error("error".into()));

    // start_daemon fails when tandem-web directory is missing
    let err = runner
        .start_daemon(&config, &tmp_dir, &tmp_dir)
        .expect_err("should fail when tandem-web is missing");
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);

    let _ = fs::remove_dir_all(tmp_dir);
}
