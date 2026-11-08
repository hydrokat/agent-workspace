use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use std::fs;
use tandem::net_interfaces::{IpCandidate, NetworkSource};
use tandem::status::settings_tab::{SettingsAction, SettingsTab};
use tandem::tmux::grid::{GridPrefs, SidebarMode};
use tandem::web_manager::{WebConfig, load_web_config, save_web_config};

fn make_key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

fn sample_ip_candidates() -> Vec<IpCandidate> {
    vec![
        IpCandidate {
            ip: "100.101.102.103".to_string(),
            interface: "tailscale0".to_string(),
            source: NetworkSource::Tailscale,
            preferred: true,
        },
        IpCandidate {
            ip: "192.168.1.50".to_string(),
            interface: "en0".to_string(),
            source: NetworkSource::Lan,
            preferred: false,
        },
        IpCandidate {
            ip: "127.0.0.1".to_string(),
            interface: "lo".to_string(),
            source: NetworkSource::Localhost,
            preferred: false,
        },
    ]
}

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
fn test_settings_tab_navigation_and_rows() {
    let mut tab = SettingsTab::new(GridPrefs::default(), SidebarMode::default());
    tab.set_ip_candidates(sample_ip_candidates());

    assert_eq!(tab.focused_row, 0);

    // Navigate down through all 7 rows: 0..=6
    for i in 1..=6 {
        tab.handle_key(&make_key(KeyCode::Down));
        assert_eq!(tab.focused_row, i);
    }

    // Down from row 6 wraps to 0
    tab.handle_key(&make_key(KeyCode::Down));
    assert_eq!(tab.focused_row, 0);

    // Up from row 0 wraps to 6
    tab.handle_key(&make_key(KeyCode::Up));
    assert_eq!(tab.focused_row, 6);
}

#[test]
fn test_settings_tab_first_time_consent_modal() {
    let mut tab = SettingsTab::new(GridPrefs::default(), SidebarMode::default());
    tab.set_ip_candidates(sample_ip_candidates());

    // Navigate to row 4: Enable web
    tab.focused_row = 4;
    assert!(!tab.web_config.enabled);
    assert!(!tab.web_config.first_time_acknowledged);

    // Attempting to enable web triggers first-time consent modal
    tab.handle_key(&make_key(KeyCode::Right));
    assert!(
        tab.confirm_web_modal,
        "modal must open on first-time enable"
    );
    assert_eq!(tab.web_modal_focus, 0); // Defaults to [ Install & Enable ]

    // Cancel via Esc
    tab.handle_key(&make_key(KeyCode::Esc));
    assert!(!tab.confirm_web_modal);
    assert!(!tab.web_config.enabled, "web must remain off after cancel");

    // Re-open modal and confirm
    tab.handle_key(&make_key(KeyCode::Right));
    assert!(tab.confirm_web_modal);
    let action = tab.handle_key(&make_key(KeyCode::Enter));
    assert_eq!(action, Some(SettingsAction::Changed));
    assert!(!tab.confirm_web_modal);
    assert!(tab.web_config.first_time_acknowledged);
    assert!(tab.web_config.enabled);

    // Subsequent toggle directly toggles without modal
    let action = tab.handle_key(&make_key(KeyCode::Left));
    assert_eq!(action, Some(SettingsAction::Changed));
    assert!(!tab.web_config.enabled);
    assert!(!tab.confirm_web_modal);

    tab.handle_key(&make_key(KeyCode::Right));
    assert!(tab.web_config.enabled);
    assert!(!tab.confirm_web_modal);
}

#[test]
fn test_settings_tab_ip_source_cycling() {
    let mut tab = SettingsTab::new(GridPrefs::default(), SidebarMode::default());
    let candidates = sample_ip_candidates();
    tab.set_ip_candidates(candidates);

    // Navigate to row 5: Bind IP
    tab.focused_row = 5;
    assert_eq!(tab.web_config.bind_ip, "100.101.102.103");
    assert_eq!(tab.web_config.bind_source, NetworkSource::Tailscale);

    // Cycle right
    tab.handle_key(&make_key(KeyCode::Right));
    assert_eq!(tab.web_config.bind_ip, "192.168.1.50");
    assert_eq!(tab.web_config.bind_source, NetworkSource::Lan);

    // Cycle right again
    tab.handle_key(&make_key(KeyCode::Right));
    assert_eq!(tab.web_config.bind_ip, "127.0.0.1");
    assert_eq!(tab.web_config.bind_source, NetworkSource::Localhost);

    // Cycle right wraps to 0
    tab.handle_key(&make_key(KeyCode::Right));
    assert_eq!(tab.web_config.bind_ip, "100.101.102.103");
    assert_eq!(tab.web_config.bind_source, NetworkSource::Tailscale);
}

#[test]
fn test_settings_tab_render_contains_web_section() {
    let mut tab = SettingsTab::new(GridPrefs::default(), SidebarMode::default());
    tab.set_ip_candidates(sample_ip_candidates());

    let area = Rect::new(0, 0, 100, 24);
    let mut buf = Buffer::empty(area);
    tab.render(area, &mut buf);

    let content: String = (0..area.height)
        .map(|y| {
            let line: String = (0..area.width)
                .map(|x| buf.cell((x, y)).unwrap().symbol().to_string())
                .collect();
            format!("{line}\n")
        })
        .collect();

    assert!(content.contains("Remote web (tandem-web)"));
    assert!(content.contains("Enable web"));
    assert!(content.contains("Bind IP"));
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
