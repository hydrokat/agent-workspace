mod common;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use common::TempDir;
use tandem::coordination::store::CoordStore;
use tandem::net_interfaces::NetworkSource;
use tandem::status::app::{StatusApp, StatusTab};
use tandem::status::web_tab::{ROW_ENABLE, ROW_TERMINAL};
use tandem::web_deps::{
    BunState, DepsRuntime, Facts, InstallOutcome, Platform, Report, Script, SystemDepsRuntime,
};
use tandem::web_manager::{DaemonRunner, WebConfig, WebStatus, load_web_config, web_config_path};

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

fn mac_arm() -> Platform {
    Platform {
        os: "macos".into(),
        arch: "arm64".into(),
        libc: None,
    }
}

fn facts(bun: BunState) -> Facts {
    Facts {
        platform: mac_arm(),
        bun,
        ttyd: false,
        need_terminal: false,
        host_problem: None,
        tools: vec!["curl".into()],
    }
}

struct FakeDeps {
    facts: Mutex<Facts>,
    after_install: Mutex<Facts>,
    outcome: Mutex<Option<InstallOutcome>>,
    written: Mutex<Vec<(PathBuf, String)>>,
    installs: Mutex<Vec<PathBuf>>,
    checks: Mutex<Vec<bool>>,
}

impl FakeDeps {
    fn new(initial: Facts) -> Arc<Self> {
        Arc::new(Self {
            after_install: Mutex::new(facts(BunState::Ok)),
            facts: Mutex::new(initial),
            outcome: Mutex::new(Some(InstallOutcome {
                success: true,
                message: String::new(),
            })),
            written: Mutex::new(Vec::new()),
            installs: Mutex::new(Vec::new()),
            checks: Mutex::new(Vec::new()),
        })
    }
    fn fail_with(&self, message: &str) {
        *self.outcome.lock().unwrap() = Some(InstallOutcome {
            success: false,
            message: message.into(),
        });
    }
}

impl DepsRuntime for FakeDeps {
    fn check(&self, need_terminal: bool) -> Report {
        self.checks.lock().unwrap().push(need_terminal);
        let mut f = self.facts.lock().unwrap().clone();
        f.need_terminal = need_terminal;
        let ok = f.bun == BunState::Ok;
        Report {
            facts: f,
            bun_path: ok.then(|| PathBuf::from("/fake/bun")),
            bun_version: ok.then(|| "1.3.3".to_string()),
        }
    }
    fn write_script(&self, state_dir: &Path, script: &Script) -> io::Result<PathBuf> {
        let path = state_dir.join(&script.name);
        self.written
            .lock()
            .unwrap()
            .push((path.clone(), script.content.clone()));
        Ok(path)
    }
    fn start_install(&self, script_path: &Path, _log: &Path) -> Receiver<InstallOutcome> {
        self.installs
            .lock()
            .unwrap()
            .push(script_path.to_path_buf());
        let outcome = self.outcome.lock().unwrap().take().unwrap();
        if outcome.success {
            *self.facts.lock().unwrap() = self.after_install.lock().unwrap().clone();
        }
        let (tx, rx) = mpsc::channel();
        tx.send(outcome).unwrap();
        rx
    }
}

struct Fixture {
    _tmp: TempDir,
    state: PathBuf,
    daemon: Arc<FakeDaemon>,
    deps: Arc<FakeDeps>,
    app: StatusApp,
}

fn config() -> WebConfig {
    WebConfig {
        first_time_acknowledged: true,
        bind_ip: "100.101.102.103".into(),
        bind_source: NetworkSource::Tailscale,
        ttyd_enabled: false,
        ..WebConfig::default()
    }
}

fn fixture(initial: Facts) -> Fixture {
    let tmp = TempDir::new();
    let ws = tmp.path().join("ws");
    fs::create_dir_all(&ws).unwrap();
    let state = tmp.path().join("state");
    fs::create_dir_all(&state).unwrap();
    let daemon = Arc::new(FakeDaemon::default());
    let deps = FakeDeps::new(initial);
    let mut app = StatusApp::new(ws, CoordStore::new(tmp.path().join("c")), None, None)
        .with_web_daemon(daemon.clone(), state.clone())
        .with_deps_runtime(deps.clone());
    app.web_tab.set_web_config(config());
    app.set_tab(StatusTab::Web);
    Fixture {
        _tmp: tmp,
        state,
        daemon,
        deps,
        app,
    }
}

fn press(app: &mut StatusApp, code: KeyCode) {
    app.handle_key(&KeyEvent::new(code, KeyModifiers::NONE));
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

fn enable(f: &mut Fixture) {
    f.app.web_tab.focused_row = ROW_ENABLE;
    press(&mut f.app, KeyCode::Enter);
}

#[test]
fn enabling_with_bun_installed_just_starts_without_asking() {
    let mut f = fixture(facts(BunState::Ok));
    enable(&mut f);
    assert_eq!(f.daemon.calls(), ["stop", "start"]);
    assert!(f.app.web_tab.deps_prompt.is_none() && f.app.web_tab.deps_help.is_none());
    assert!(f.deps.written.lock().unwrap().is_empty());
    assert_eq!(*f.deps.checks.lock().unwrap(), [false]);
}

#[test]
fn enabling_without_bun_asks_before_doing_anything() {
    let mut f = fixture(facts(BunState::Missing));
    enable(&mut f);

    assert!(f.app.web_tab.deps_prompt.is_some());
    assert!(
        f.daemon.calls().is_empty(),
        "nothing may start: {:?}",
        f.daemon.calls()
    );
    assert!(
        f.deps.installs.lock().unwrap().is_empty(),
        "nothing may be installed before the user chooses"
    );
    assert!(!f.app.web_tab.web_config.enabled);

    let text = render(&mut f.app, 120, 14);
    for needle in [
        "bun",
        "macOS arm64",
        "[ Let tandem install ]",
        "[ I'll install it myself ]",
    ] {
        assert!(
            text.contains(needle),
            "prompt must show {needle:?}:\n{text}"
        );
    }
}

#[test]
fn the_prompt_shows_the_generated_script_path_and_what_it_runs() {
    let mut f = fixture(facts(BunState::Missing));
    enable(&mut f);
    let written = f.deps.written.lock().unwrap().clone();
    assert_eq!(written.len(), 1);
    assert!(written[0].0.ends_with("install-web-deps.sh"));
    assert!(
        written[0]
            .1
            .contains("curl -fsSL https://bun.com/install | bash")
    );
    let text = render(&mut f.app, 140, 14);
    assert!(text.contains("install-web-deps.sh"), "{text}");
    assert!(text.contains("bun.com/install"), "{text}");
}

#[test]
fn blocking_on_dependencies_still_remembers_the_first_time_consent_but_not_enabled() {
    let mut f = fixture(facts(BunState::Missing));
    enable(&mut f);
    let saved = load_web_config(&web_config_path(&f.state)).unwrap();
    assert!(!saved.enabled);
    assert!(saved.first_time_acknowledged);
}

#[test]
fn escape_cancels_without_installing_or_starting() {
    let mut f = fixture(facts(BunState::Missing));
    enable(&mut f);
    press(&mut f.app, KeyCode::Esc);
    assert!(f.app.web_tab.deps_prompt.is_none() && f.app.web_tab.deps_help.is_none());
    assert!(f.deps.installs.lock().unwrap().is_empty());
    assert!(f.daemon.calls().is_empty());
    assert!(!f.app.web_tab.web_config.enabled);
}

#[test]
fn letting_tandem_install_runs_the_script_in_the_background_then_starts_web() {
    let mut f = fixture(facts(BunState::Missing));
    enable(&mut f);
    press(&mut f.app, KeyCode::Enter);

    assert_eq!(f.deps.installs.lock().unwrap().len(), 1);
    assert!(f.deps.installs.lock().unwrap()[0].ends_with("install-web-deps.sh"));
    assert!(
        f.app.web_tab.installing,
        "UI must show that it is installing"
    );
    assert!(f.app.web_tab.deps_prompt.is_none());
    assert!(
        f.daemon.calls().is_empty(),
        "web must not start before the install completes"
    );
    assert!(
        render(&mut f.app, 120, 14)
            .to_lowercase()
            .contains("installing")
    );

    f.app.tick_timers();
    assert!(!f.app.web_tab.installing);
    assert_eq!(f.daemon.calls(), ["stop", "start"]);
    assert!(f.app.web_tab.web_config.enabled);
    assert!(matches!(
        f.app.web_tab.web_status,
        WebStatus::Running { .. }
    ));
    assert!(load_web_config(&web_config_path(&f.state)).unwrap().enabled);
}

#[test]
fn toggling_enable_is_ignored_while_an_install_is_running() {
    let mut f = fixture(facts(BunState::Missing));
    enable(&mut f);
    press(&mut f.app, KeyCode::Enter);
    enable(&mut f);
    assert!(f.app.web_tab.deps_prompt.is_none(), "no second prompt");
    assert_eq!(f.deps.installs.lock().unwrap().len(), 1);
    assert!(f.daemon.calls().is_empty());
}

#[test]
fn a_failed_install_is_reported_and_web_stays_off() {
    let mut f = fixture(facts(BunState::Missing));
    f.deps
        .fail_with("the installer script failed (exit status 7); see /tmp/log");
    enable(&mut f);
    press(&mut f.app, KeyCode::Enter);
    f.app.tick_timers();

    assert!(!f.app.web_tab.installing);
    match &f.app.web_tab.web_status {
        WebStatus::Error(msg) => assert!(msg.contains("exit status 7"), "{msg}"),
        other => panic!("expected Error, got {other:?}"),
    }
    assert!(f.daemon.calls().is_empty());
    assert!(!f.app.web_tab.web_config.enabled);
    assert!(!load_web_config(&web_config_path(&f.state)).unwrap().enabled);
    assert!(render(&mut f.app, 160, 14).contains("exit status 7"));
}

#[test]
fn an_install_that_finishes_but_leaves_bun_unusable_is_an_error_not_a_start() {
    let mut f = fixture(facts(BunState::Missing));
    *f.deps.after_install.lock().unwrap() = facts(BunState::Missing);
    enable(&mut f);
    press(&mut f.app, KeyCode::Enter);
    f.app.tick_timers();

    match &f.app.web_tab.web_status {
        WebStatus::Error(msg) => assert!(msg.contains("still not available"), "{msg}"),
        other => panic!("expected Error, got {other:?}"),
    }
    assert!(f.daemon.calls().is_empty());
    assert!(!f.app.web_tab.web_config.enabled);
}

#[test]
fn choosing_to_install_it_yourself_keeps_the_script_and_shows_the_commands() {
    let mut f = fixture(facts(BunState::Missing));
    enable(&mut f);
    press(&mut f.app, KeyCode::Tab);
    press(&mut f.app, KeyCode::Enter);

    assert!(f.deps.installs.lock().unwrap().is_empty());
    assert!(f.daemon.calls().is_empty());
    assert!(!f.app.web_tab.web_config.enabled);
    let help = f
        .app
        .web_tab
        .deps_help
        .clone()
        .expect("manual instructions");
    let joined = help.join("\n");
    assert!(
        joined.contains("curl -fsSL https://bun.com/install | bash"),
        "{joined}"
    );
    assert!(joined.contains("install-web-deps.sh"), "{joined}");

    let text = render(&mut f.app, 140, 16);
    assert!(text.contains("bun.com/install"), "{text}");
    press(&mut f.app, KeyCode::Esc);
    assert!(f.app.web_tab.deps_help.is_none());
}

#[test]
fn an_unsupported_platform_gets_an_explanation_and_no_install_offer() {
    let mut s = facts(BunState::Missing);
    s.platform = Platform {
        os: "freebsd".into(),
        arch: "riscv64".into(),
        libc: None,
    };
    let mut f = fixture(s);
    enable(&mut f);
    assert!(f.app.web_tab.deps_prompt.is_none());
    let help = f
        .app
        .web_tab
        .deps_help
        .clone()
        .expect("explanation")
        .join("\n");
    assert!(help.contains("freebsd riscv64"), "{help}");
    assert!(help.contains("https://bun.com/docs/installation"), "{help}");
    assert!(f.deps.written.lock().unwrap().is_empty());
    assert!(f.daemon.calls().is_empty());
}

#[test]
fn missing_prerequisites_are_explained_instead_of_offering_an_install_that_cannot_work() {
    let mut s = facts(BunState::Missing);
    s.platform = Platform {
        os: "linux".into(),
        arch: "x64".into(),
        libc: Some("glibc".into()),
    };
    s.tools = vec!["curl".into(), "apt".into()];
    let mut f = fixture(s);
    enable(&mut f);
    assert!(f.app.web_tab.deps_prompt.is_none());
    let help = f
        .app
        .web_tab
        .deps_help
        .clone()
        .expect("explanation")
        .join("\n");
    assert!(help.contains("sudo apt install unzip"), "{help}");
    assert!(f.deps.written.lock().unwrap().is_empty());
    assert!(f.daemon.calls().is_empty());
}

#[test]
fn ttyd_alone_missing_does_not_block_but_tells_the_user() {
    let mut s = facts(BunState::Ok);
    s.platform = Platform {
        os: "linux".into(),
        arch: "x64".into(),
        libc: Some("glibc".into()),
    };
    s.tools = vec!["apt".into()];
    let mut f = fixture(s);
    let mut cfg = config();
    cfg.ttyd_enabled = true;
    f.app.web_tab.set_web_config(cfg);
    enable(&mut f);

    assert_eq!(f.daemon.calls(), ["stop", "start"]);
    assert_eq!(
        *f.deps.checks.lock().unwrap(),
        [true],
        "ttyd is only checked when Terminal is on"
    );
    let flash = f.app.web_tab.flash_message.clone().expect("flash").0;
    assert!(flash.contains("ttyd"), "{flash}");
}

#[test]
fn restarting_a_running_server_for_a_bind_change_does_not_ask_again() {
    let mut f = fixture(facts(BunState::Ok));
    enable(&mut f);
    f.app.web_tab.focused_row = ROW_TERMINAL;
    press(&mut f.app, KeyCode::Enter);
    assert!(f.app.web_tab.deps_prompt.is_none());
    assert_eq!(f.daemon.calls(), ["stop", "start", "stop", "start"]);
}

#[test]
fn disabling_never_checks_dependencies() {
    let mut f = fixture(facts(BunState::Ok));
    enable(&mut f);
    f.deps.checks.lock().unwrap().clear();
    enable(&mut f);
    assert!(f.deps.checks.lock().unwrap().is_empty());
    assert_eq!(f.app.web_tab.web_status, WebStatus::Off);
}

#[test]
fn the_prompt_owns_the_keyboard() {
    let mut f = fixture(facts(BunState::Missing));
    enable(&mut f);
    press(&mut f.app, KeyCode::Char('1'));
    press(&mut f.app, KeyCode::Down);
    assert_eq!(f.app.tab, StatusTab::Web);
    assert_eq!(f.app.web_tab.focused_row, ROW_ENABLE);
    assert!(f.app.web_tab.deps_prompt.is_some());
}

#[test]
fn mouse_clicks_on_the_prompt_buttons_choose_install_or_manual() {
    let mut install = fixture(facts(BunState::Missing));
    enable(&mut install);
    render(&mut install.app, 120, 14);
    let area = install.app.body_area;
    let (x, y) = modal_origin(area);
    click(&mut install.app, x + 5, y + 5);
    assert_eq!(install.deps.installs.lock().unwrap().len(), 1);

    let mut manual = fixture(facts(BunState::Missing));
    enable(&mut manual);
    render(&mut manual.app, 120, 14);
    let (x, y) = modal_origin(manual.app.body_area);
    click(&mut manual.app, x + 32, y + 5);
    assert!(manual.deps.installs.lock().unwrap().is_empty());
    assert!(manual.app.web_tab.deps_help.is_some());

    let mut outside = fixture(facts(BunState::Missing));
    enable(&mut outside);
    render(&mut outside.app, 120, 14);
    let (ox, oy) = (outside.app.body_area.x, outside.app.body_area.y);
    click(&mut outside.app, ox, oy);
    assert!(outside.deps.installs.lock().unwrap().is_empty());
    assert!(outside.app.web_tab.deps_prompt.is_none());
}

fn modal_origin(area: ratatui::layout::Rect) -> (u16, u16) {
    let w = 68u16.min(area.width);
    let h = 7u16.min(area.height);
    (
        area.x + area.width.saturating_sub(w) / 2,
        area.y + area.height.saturating_sub(h) / 2,
    )
}

fn click(app: &mut StatusApp, col: u16, row: u16) {
    app.handle_mouse(&MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: col,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

#[test]
fn prompt_help_and_installing_views_never_panic_at_any_size() {
    for scenario in 0..3 {
        let mut f = fixture(facts(BunState::Missing));
        enable(&mut f);
        match scenario {
            1 => {
                press(&mut f.app, KeyCode::Tab);
                press(&mut f.app, KeyCode::Enter);
            }
            2 => press(&mut f.app, KeyCode::Enter),
            _ => {}
        }
        for w in [0u16, 1, 10, 30, 60, 100, 140] {
            for h in [0u16, 1, 4, 7, 10, 20] {
                let _ = render(&mut f.app, w, h);
            }
        }
    }
}

mod system_runtime {
    use super::*;

    fn script(name: &str, body: &str) -> Script {
        Script {
            name: name.into(),
            content: body.into(),
        }
    }

    #[test]
    fn writes_the_script_owner_only_into_the_state_dir() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new();
        let path = SystemDepsRuntime
            .write_script(
                &tmp.path().join("state"),
                &script("install-web-deps.sh", "#!/bin/sh\necho hi\n"),
            )
            .unwrap();
        assert!(path.ends_with("state/install-web-deps.sh"));
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "#!/bin/sh\necho hi\n");
    }

    #[test]
    fn runs_the_script_directly_without_a_shell_wrapper_and_logs_its_output() {
        let tmp = TempDir::new();
        let rt = SystemDepsRuntime;
        let path = rt
            .write_script(
                tmp.path(),
                &script(
                    "ok.sh",
                    "#!/bin/sh\necho installed-fine\necho oops 1>&2\nexit 0\n",
                ),
            )
            .unwrap();
        let log = tmp.path().join("install.log");
        let outcome = rt.start_install(&path, &log).recv().unwrap();
        assert!(outcome.success, "{}", outcome.message);
        let logged = fs::read_to_string(&log).unwrap();
        assert!(
            logged.contains("installed-fine") && logged.contains("oops"),
            "{logged}"
        );
    }

    #[test]
    fn reports_a_failing_script_with_its_exit_status_and_log_path() {
        let tmp = TempDir::new();
        let rt = SystemDepsRuntime;
        let path = rt
            .write_script(
                tmp.path(),
                &script("bad.sh", "#!/bin/sh\necho nope\nexit 3\n"),
            )
            .unwrap();
        let log = tmp.path().join("install.log");
        let outcome = rt.start_install(&path, &log).recv().unwrap();
        assert!(!outcome.success);
        assert!(
            outcome.message.contains("exit status 3"),
            "{}",
            outcome.message
        );
        assert!(
            outcome.message.contains("install.log"),
            "{}",
            outcome.message
        );
    }

    #[test]
    fn reports_a_script_that_cannot_be_executed() {
        let tmp = TempDir::new();
        let log = tmp.path().join("install.log");
        let outcome = SystemDepsRuntime
            .start_install(&tmp.path().join("does-not-exist.sh"), &log)
            .recv()
            .unwrap();
        assert!(!outcome.success);
        assert!(
            outcome.message.contains("could not run"),
            "{}",
            outcome.message
        );
    }
}
