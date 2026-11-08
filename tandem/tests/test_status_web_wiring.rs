mod common;

use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use common::TempDir;
use tandem::coordination::store::CoordStore;
use tandem::net_interfaces::NetworkSource;
use tandem::status::app::{StatusApp, StatusTab};
use tandem::web_manager::{DaemonRunner, WebConfig, WebStatus, load_web_config, web_config_path};

#[derive(Default)]
struct FakeDaemon {
    calls: Mutex<Vec<String>>,
    fail_start: Mutex<Option<String>>,
}

impl FakeDaemon {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl DaemonRunner for FakeDaemon {
    fn start_daemon(&self, _: &WebConfig, _: &Path, _: &Path) -> Result<u32, io::Error> {
        self.calls.lock().unwrap().push("start".into());
        match self.fail_start.lock().unwrap().clone() {
            Some(msg) => Err(io::Error::new(io::ErrorKind::NotFound, msg)),
            None => Ok(4242),
        }
    }
    fn stop_daemon(&self, _: &Path) -> Result<(), io::Error> {
        self.calls.lock().unwrap().push("stop".into());
        Ok(())
    }
    fn status(&self, config: &WebConfig, _: &Path) -> WebStatus {
        if config.enabled && self.fail_start.lock().unwrap().is_none() {
            WebStatus::Running {
                url: format!("http://{}:{}", config.bind_ip, config.port),
                pid: Some(4242),
            }
        } else {
            WebStatus::Off
        }
    }
}

struct Fixture {
    _tmp: TempDir,
    state: std::path::PathBuf,
    daemon: Arc<FakeDaemon>,
    app: StatusApp,
}

fn fixture() -> Fixture {
    let tmp = TempDir::new();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let state = tmp.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    let daemon = Arc::new(FakeDaemon::default());
    let app = StatusApp::new(ws, CoordStore::new(tmp.path().join("c")), None, None)
        .with_web_daemon(daemon.clone(), state.clone())
        .with_deps_runtime(Arc::new(common::ready_deps::ReadyDeps));
    Fixture {
        _tmp: tmp,
        state,
        daemon,
        app,
    }
}

fn press(app: &mut StatusApp, code: KeyCode) {
    app.handle_key(&KeyEvent::new(code, KeyModifiers::NONE));
}

fn acknowledged_cfg() -> WebConfig {
    WebConfig {
        first_time_acknowledged: true,
        bind_ip: "100.101.102.103".into(),
        bind_source: NetworkSource::Tailscale,
        ..WebConfig::default()
    }
}

fn render(app: &mut StatusApp, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| app.render(f.area(), f.buffer_mut())).unwrap();
    let buf = term.backend().buffer().clone();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn tab_order_puts_web_before_settings() {
    assert_eq!(StatusTab::ALL.len(), 7);
    assert_eq!(StatusTab::Web.index(), 5);
    assert_eq!(StatusTab::Settings.index(), 6);
    assert_eq!(StatusTab::from_index(5), Some(StatusTab::Web));
    assert_eq!(StatusTab::from_index(6), Some(StatusTab::Settings));
    assert_eq!(StatusTab::from_index(7), None);
    assert_eq!(StatusTab::Queries.next(), StatusTab::Web);
    assert_eq!(StatusTab::Web.next(), StatusTab::Settings);
    assert_eq!(StatusTab::Settings.next(), StatusTab::Status);
    assert_eq!(StatusTab::Status.prev(), StatusTab::Settings);
    assert_eq!(StatusTab::Settings.prev(), StatusTab::Web);
    assert_eq!(StatusTab::Web.prev(), StatusTab::Queries);
    assert_eq!(StatusTab::Web.title(0), "Web");
    assert_eq!(StatusTab::Web.number_title(0), "6");
    assert_eq!(StatusTab::Settings.number_title(0), "7");
    for (i, t) in StatusTab::ALL.iter().enumerate() {
        assert_eq!(t.index(), i);
    }
}

#[test]
fn number_keys_six_and_seven_select_web_and_settings() {
    let mut f = fixture();
    press(&mut f.app, KeyCode::Char('6'));
    assert_eq!(f.app.tab, StatusTab::Web);
    press(&mut f.app, KeyCode::Char('7'));
    assert_eq!(f.app.tab, StatusTab::Settings);
    press(&mut f.app, KeyCode::Tab);
    assert_eq!(f.app.tab, StatusTab::Status);
}

#[test]
fn tab_key_moves_on_from_web_unless_input_is_captured() {
    let mut f = fixture();
    press(&mut f.app, KeyCode::Char('6'));
    press(&mut f.app, KeyCode::Tab);
    assert_eq!(f.app.tab, StatusTab::Settings);

    press(&mut f.app, KeyCode::Char('6'));
    f.app.web_tab.focused_row = tandem::status::web_tab::ROW_PORT;
    press(&mut f.app, KeyCode::Enter);
    press(&mut f.app, KeyCode::Tab);
    assert_eq!(
        f.app.tab,
        StatusTab::Web,
        "Tab must not leave while editing the port"
    );
}

#[test]
fn digits_typed_into_port_do_not_switch_tabs() {
    let mut f = fixture();
    press(&mut f.app, KeyCode::Char('6'));
    f.app.web_tab.focused_row = tandem::status::web_tab::ROW_PORT;
    press(&mut f.app, KeyCode::Enter);
    for _ in 0..4 {
        press(&mut f.app, KeyCode::Backspace);
    }
    for c in "5170".chars() {
        press(&mut f.app, KeyCode::Char(c));
    }
    assert_eq!(f.app.tab, StatusTab::Web);
    press(&mut f.app, KeyCode::Enter);
    assert_eq!(f.app.web_tab.web_config.port, 5170);
}

#[test]
fn enabling_persists_config_and_restarts_daemon() {
    let mut f = fixture();
    f.app.web_tab.set_web_config(acknowledged_cfg());
    press(&mut f.app, KeyCode::Char('6'));
    press(&mut f.app, KeyCode::Enter);

    assert_eq!(f.daemon.calls(), ["stop", "start"]);
    assert!(matches!(
        f.app.web_tab.web_status,
        WebStatus::Running { .. }
    ));
    let saved = load_web_config(&web_config_path(&f.state)).unwrap();
    assert!(saved.enabled);
}

#[test]
fn disabling_only_stops_the_daemon() {
    let mut f = fixture();
    let mut cfg = acknowledged_cfg();
    cfg.enabled = true;
    f.app.web_tab.set_web_config(cfg);
    press(&mut f.app, KeyCode::Char('6'));
    press(&mut f.app, KeyCode::Enter);
    assert_eq!(f.daemon.calls(), ["stop"]);
    assert_eq!(f.app.web_tab.web_status, WebStatus::Off);
    assert!(!load_web_config(&web_config_path(&f.state)).unwrap().enabled);
}

#[test]
fn changing_bind_ip_while_running_restarts_so_it_takes_effect() {
    let mut f = fixture();
    let mut cfg = acknowledged_cfg();
    cfg.enabled = true;
    f.app.web_tab.set_web_config(cfg);
    f.app.web_tab.set_ip_candidates(vec![
        tandem::net_interfaces::IpCandidate {
            ip: "100.101.102.103".into(),
            interface: "ts".into(),
            source: NetworkSource::Tailscale,
            preferred: true,
        },
        tandem::net_interfaces::IpCandidate {
            ip: "192.168.1.50".into(),
            interface: "en0".into(),
            source: NetworkSource::Lan,
            preferred: false,
        },
    ]);
    press(&mut f.app, KeyCode::Char('6'));
    f.app.web_tab.focused_row = tandem::status::web_tab::ROW_BIND;
    press(&mut f.app, KeyCode::Right);
    assert_eq!(f.daemon.calls(), ["stop", "start"]);
    assert_eq!(
        load_web_config(&web_config_path(&f.state)).unwrap().bind_ip,
        "192.168.1.50"
    );
}

#[test]
fn start_failure_is_shown_in_the_pane_not_swallowed() {
    let mut f = fixture();
    *f.daemon.fail_start.lock().unwrap() = Some("tandem-web is not built".into());
    f.app.web_tab.set_web_config(acknowledged_cfg());
    press(&mut f.app, KeyCode::Char('6'));
    press(&mut f.app, KeyCode::Enter);

    match &f.app.web_tab.web_status {
        WebStatus::Error(msg) => assert!(msg.contains("not built")),
        other => panic!("expected Error, got {other:?}"),
    }
    let text = render(&mut f.app, 120, 14);
    assert!(text.contains("not built"), "{text}");
}

#[test]
fn regenerating_token_persists_new_token_and_restarts() {
    let mut f = fixture();
    let mut cfg = acknowledged_cfg();
    cfg.enabled = true;
    let old = cfg.auth_token.clone();
    f.app.web_tab.set_web_config(cfg);
    press(&mut f.app, KeyCode::Char('6'));
    f.app.web_tab.focused_row = tandem::status::web_tab::ROW_REGENERATE;
    press(&mut f.app, KeyCode::Enter);
    press(&mut f.app, KeyCode::Left);
    press(&mut f.app, KeyCode::Enter);

    assert_eq!(f.daemon.calls(), ["stop", "start"]);
    let saved = load_web_config(&web_config_path(&f.state)).unwrap();
    assert_ne!(saved.auth_token, old);
    assert_eq!(saved.auth_token, f.app.web_tab.web_config.auth_token);
}

#[test]
fn settings_changes_never_touch_the_web_daemon() {
    let mut f = fixture();
    let mut cfg = acknowledged_cfg();
    cfg.enabled = true;
    f.app.web_tab.set_web_config(cfg);
    press(&mut f.app, KeyCode::Char('7'));
    press(&mut f.app, KeyCode::Right);
    assert!(f.daemon.calls().is_empty(), "{:?}", f.daemon.calls());
}

#[test]
fn rendered_tab_bar_lists_web_and_pane_shows_rows() {
    let mut f = fixture();
    f.app.web_tab.set_web_config(acknowledged_cfg());
    press(&mut f.app, KeyCode::Char('6'));
    let text = render(&mut f.app, 120, 14);
    assert!(text.contains("Web"), "{text}");
    assert!(
        text.contains("Enable web") && text.contains("Regenerate"),
        "{text}"
    );
}

#[test]
fn settings_tab_has_no_web_rows_and_reset_ui_is_visible_at_default_height() {
    let mut f = fixture();
    press(&mut f.app, KeyCode::Char('7'));
    let text = render(&mut f.app, 120, 10);
    assert!(!text.contains("Remote web"), "{text}");
    assert!(!text.contains("Enable web"), "{text}");
    assert!(
        text.contains("Reset UI"),
        "Reset UI must fit in 7 body rows:\n{text}"
    );
}
