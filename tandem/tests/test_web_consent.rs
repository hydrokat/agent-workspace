mod common;

use std::fs;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use common::TempDir;
use tandem::coordination::store::CoordStore;
use tandem::net_interfaces::NetworkSource;
use tandem::status::app::{StatusApp, StatusTab};
use tandem::status::web_tab::{ROW_ENABLE, WebAction, WebTab};
use tandem::web_manager::{
    DaemonRunner, SystemDaemonRunner, WebConfig, WebStatus, load_web_config, resolve_web_dir,
    web_config_path,
};

#[derive(Default)]
struct FakeDaemon {
    calls: Mutex<Vec<String>>,
}

impl FakeDaemon {
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl DaemonRunner for FakeDaemon {
    fn start_daemon(&self, _: &WebConfig, _: &Path, _: &Path) -> Result<u32, io::Error> {
        self.calls.lock().unwrap().push("start".into());
        Ok(1)
    }
    fn stop_daemon(&self, _: &Path) -> Result<(), io::Error> {
        self.calls.lock().unwrap().push("stop".into());
        Ok(())
    }
    fn status(&self, config: &WebConfig, _: &Path) -> WebStatus {
        if config.enabled {
            WebStatus::Running {
                url: format!("http://{}:{}", config.bind_ip, config.port),
                pid: Some(1),
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

fn fresh_config() -> WebConfig {
    WebConfig {
        bind_ip: "100.101.102.103".into(),
        bind_source: NetworkSource::Tailscale,
        ..WebConfig::default()
    }
}

fn fixture(config: WebConfig) -> Fixture {
    let tmp = TempDir::new();
    let ws = tmp.path().join("ws");
    fs::create_dir_all(&ws).unwrap();
    let state = tmp.path().join("state");
    fs::create_dir_all(&state).unwrap();
    let daemon = Arc::new(FakeDaemon::default());
    let mut app = StatusApp::new(ws, CoordStore::new(tmp.path().join("c")), None, None)
        .with_web_daemon(daemon.clone(), state.clone())
        .with_deps_runtime(Arc::new(common::ready_deps::ReadyDeps));
    app.web_tab.set_web_config(config);
    app.set_tab(StatusTab::Web);
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

fn text(buf: &Buffer) -> String {
    (buf.area.y..buf.area.y + buf.area.height)
        .map(|y| {
            (buf.area.x..buf.area.x + buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_tab(tab: &WebTab, w: u16, h: u16) -> String {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    tab.render(area, &mut buf);
    text(&buf)
}

fn click(col: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: col,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn first_time_tab() -> WebTab {
    let mut tab = WebTab::default();
    tab.set_web_config(fresh_config());
    tab.focused_row = ROW_ENABLE;
    tab.confirm_web_modal = true;
    tab
}

#[test]
fn consent_modal_tells_the_user_that_additional_dependencies_may_be_installed() {
    let out = render_tab(&first_time_tab(), 100, 12);
    assert!(out.contains("may install"), "{out}");
    assert!(out.contains("additional dependencies"), "{out}");
    for name in ["bun", "tandem-web packages", "ttyd"] {
        assert!(out.contains(name), "modal must name {name}:\n{out}");
    }
}

#[test]
fn consent_modal_shows_what_will_be_exposed_and_offers_a_choice() {
    let out = render_tab(&first_time_tab(), 100, 12);
    assert!(out.contains("100.101.102.103"), "{out}");
    assert!(out.contains("tailscale"), "{out}");
    assert!(out.contains("3940"), "{out}");
    assert!(out.contains("[ Install & Enable ]"), "{out}");
    assert!(out.contains("[ Cancel ]"), "{out}");
}

#[test]
fn consent_modal_still_fits_and_does_not_panic_at_small_sizes() {
    let tab = first_time_tab();
    for w in 0..=120u16 {
        for h in [0u16, 1, 4, 7, 9, 14] {
            let _ = render_tab(&tab, w, h);
        }
    }
}

#[test]
fn enabling_for_the_first_time_asks_before_doing_anything() {
    let mut f = fixture(fresh_config());
    press(&mut f.app, KeyCode::Enter);

    assert!(f.app.web_tab.confirm_web_modal, "consent must be requested");
    assert!(
        f.daemon.calls().is_empty(),
        "nothing may run before consent: {:?}",
        f.daemon.calls()
    );
    assert!(!f.app.web_tab.web_config.enabled);
    assert!(
        !web_config_path(&f.state).exists(),
        "nothing may be written before consent"
    );
}

#[test]
fn declining_keeps_web_off_and_asks_again_next_time() {
    let mut f = fixture(fresh_config());
    press(&mut f.app, KeyCode::Enter);
    press(&mut f.app, KeyCode::Esc);

    assert!(!f.app.web_tab.confirm_web_modal);
    assert!(f.daemon.calls().is_empty());
    assert!(!f.app.web_tab.web_config.enabled);
    assert!(!f.app.web_tab.web_config.first_time_acknowledged);

    press(&mut f.app, KeyCode::Enter);
    assert!(
        f.app.web_tab.confirm_web_modal,
        "a declined consent must be asked again"
    );
}

#[test]
fn choosing_cancel_in_the_modal_declines() {
    let mut f = fixture(fresh_config());
    press(&mut f.app, KeyCode::Enter);
    press(&mut f.app, KeyCode::Tab);
    press(&mut f.app, KeyCode::Enter);

    assert!(f.daemon.calls().is_empty());
    assert!(!f.app.web_tab.web_config.enabled);
    assert!(!f.app.web_tab.web_config.first_time_acknowledged);
}

#[test]
fn accepting_enables_starts_the_server_and_remembers_the_consent() {
    let mut f = fixture(fresh_config());
    press(&mut f.app, KeyCode::Enter);
    press(&mut f.app, KeyCode::Enter);

    assert_eq!(f.daemon.calls(), ["stop", "start"]);
    let saved = load_web_config(&web_config_path(&f.state)).unwrap();
    assert!(saved.enabled);
    assert!(saved.first_time_acknowledged, "consent must be persisted");
}

#[test]
fn consent_is_remembered_across_tandem_restarts() {
    let mut f = fixture(fresh_config());
    press(&mut f.app, KeyCode::Enter);
    press(&mut f.app, KeyCode::Enter);
    press(&mut f.app, KeyCode::Enter);
    assert!(!f.app.web_tab.web_config.enabled, "third Enter disables");

    let saved = load_web_config(&web_config_path(&f.state)).unwrap();
    let mut restarted = fixture(saved);
    press(&mut restarted.app, KeyCode::Enter);

    assert!(
        !restarted.app.web_tab.confirm_web_modal,
        "no second consent prompt after a restart"
    );
    assert!(restarted.app.web_tab.web_config.enabled);
}

#[test]
fn clicking_install_and_enable_accepts() {
    let area = Rect::new(0, 0, 100, 12);
    let (x, y) = (18, 2);
    let mut tab = first_time_tab();
    let action = tab.handle_mouse(&click(x + 14, y + 5), area);
    assert_eq!(action, Some(WebAction::Changed));
    assert!(tab.web_config.enabled && tab.web_config.first_time_acknowledged);
    assert!(!tab.confirm_web_modal);
}

#[test]
fn clicking_cancel_declines() {
    let area = Rect::new(0, 0, 100, 12);
    let (x, y) = (18, 2);
    let mut tab = first_time_tab();
    assert_eq!(tab.handle_mouse(&click(x + 40, y + 5), area), None);
    assert!(!tab.web_config.enabled && !tab.web_config.first_time_acknowledged);
    assert!(!tab.confirm_web_modal);
}

#[test]
fn clicking_outside_the_modal_dismisses_without_enabling() {
    let area = Rect::new(0, 0, 100, 12);
    let mut tab = first_time_tab();
    assert_eq!(tab.handle_mouse(&click(1, 0), area), None);
    assert!(!tab.web_config.enabled && !tab.web_config.first_time_acknowledged);
    assert!(!tab.confirm_web_modal);
}

mod web_dir_resolution {
    use super::*;

    fn dirs() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let tmp = TempDir::new();
        let ws = tmp.path().join("ws");
        let state = tmp.path().join("state");
        fs::create_dir_all(&ws).unwrap();
        fs::create_dir_all(&state).unwrap();
        (tmp, ws, state)
    }

    fn write_web(dir: &Path, stamp: Option<&str>) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("package.json"), "{}").unwrap();
        if let Some(s) = stamp {
            fs::write(dir.join(".tandem-version"), format!("{s}\n")).unwrap();
        }
    }

    #[test]
    fn prefers_a_tandem_web_inside_the_workspace() {
        let (_t, ws, state) = dirs();
        write_web(&ws.join("tandem-web"), None);
        write_web(&state.join("tandem-web"), Some(env!("CARGO_PKG_VERSION")));
        assert_eq!(resolve_web_dir(&ws, &state), Some(ws.join("tandem-web")));
    }

    #[test]
    fn uses_the_installed_copy_when_its_version_matches() {
        let (_t, ws, state) = dirs();
        write_web(&state.join("tandem-web"), Some(env!("CARGO_PKG_VERSION")));
        assert_eq!(resolve_web_dir(&ws, &state), Some(state.join("tandem-web")));
    }

    #[test]
    fn ignores_an_installed_copy_from_another_version_or_without_a_stamp() {
        let (_t, ws, state) = dirs();
        write_web(&state.join("tandem-web"), Some("0.0.1"));
        assert_eq!(resolve_web_dir(&ws, &state), None);
        write_web(&state.join("tandem-web"), None);
        assert_eq!(resolve_web_dir(&ws, &state), None);
    }

    #[test]
    fn ignores_an_empty_workspace_directory() {
        let (_t, ws, state) = dirs();
        fs::create_dir_all(ws.join("tandem-web")).unwrap();
        assert_eq!(resolve_web_dir(&ws, &state), None);
    }

    #[test]
    fn returns_none_when_nothing_is_installed() {
        let (_t, ws, state) = dirs();
        assert_eq!(resolve_web_dir(&ws, &state), None);
    }

    fn start_error(ws: &Path, state: &Path) -> String {
        let cfg = WebConfig {
            enabled: true,
            bind_ip: "100.101.102.103".into(),
            ..WebConfig::default()
        };
        SystemDaemonRunner
            .start_daemon(&cfg, ws, state)
            .expect_err("must not start")
            .to_string()
    }

    #[test]
    fn start_error_when_not_installed_tells_the_user_how_to_install_it() {
        let (_t, ws, state) = dirs();
        let msg = start_error(&ws, &state);
        assert!(msg.contains("not installed"), "{msg}");
        assert!(msg.contains("agent-workspace web"), "{msg}");
    }

    #[test]
    fn start_error_when_installed_but_not_built_tells_the_user_how_to_build_it() {
        let (_t, ws, state) = dirs();
        write_web(&state.join("tandem-web"), Some(env!("CARGO_PKG_VERSION")));
        let msg = start_error(&ws, &state);
        assert!(msg.contains("not built"), "{msg}");
        assert!(msg.contains("agent-workspace web"), "{msg}");
    }

    #[test]
    fn start_error_for_a_stale_installed_copy_asks_to_update_it() {
        let (_t, ws, state) = dirs();
        write_web(&state.join("tandem-web"), Some("0.0.1"));
        let msg = start_error(&ws, &state);
        assert!(msg.contains("agent-workspace web"), "{msg}");
    }
}
