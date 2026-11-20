mod common;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use common::TempDir;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tandem::factory::broker::gate::interactive_ready;
use tandem::factory::login::{
    LOGIN_SPECS, LoginError, LoginProbe, all_statuses, check_status, open_login_pane,
};
use tandem::factory::settings as factory_settings;
use tandem::factory::store::FactoryStore;
use tandem::status::settings_tab::SettingsTab;
use tandem::tmux::client::{Result as TmuxResult, TmuxClient, TmuxRunner};

struct FakeLoginProbe {
    bins: HashMap<String, PathBuf>,
    versions: HashMap<String, String>,
    outputs: HashMap<String, (i32, String, String)>,
}

impl FakeLoginProbe {
    fn new() -> Self {
        Self {
            bins: HashMap::new(),
            versions: HashMap::new(),
            outputs: HashMap::new(),
        }
    }

    fn with_cli(mut self, cli: &str, version: &str, code: i32, out: &str, err: &str) -> Self {
        self.bins
            .insert(cli.to_string(), PathBuf::from(format!("/usr/bin/{cli}")));
        self.versions.insert(cli.to_string(), version.to_string());
        self.outputs
            .insert(cli.to_string(), (code, out.to_string(), err.to_string()));
        self
    }
}

impl LoginProbe for FakeLoginProbe {
    fn which(&self, bin: &str) -> Option<PathBuf> {
        self.bins.get(bin).cloned()
    }

    fn version(&self, cli: &str) -> String {
        self.versions.get(cli).cloned().unwrap_or_default()
    }

    fn run(&self, argv: &[&str]) -> (i32, String, String) {
        let key = argv.first().copied().unwrap_or("");
        self.outputs
            .get(key)
            .cloned()
            .unwrap_or((1, String::new(), "not found".to_string()))
    }
}

struct RecordingTmuxRunner {
    calls: Mutex<Vec<Vec<String>>>,
}

impl RecordingTmuxRunner {
    fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl TmuxRunner for RecordingTmuxRunner {
    fn run(&self, argv: &[String]) -> TmuxResult {
        self.calls.lock().unwrap().push(argv.to_vec());
        if argv.iter().any(|a| a == "new-window") {
            return TmuxResult {
                code: 0,
                out: "%77\n".to_string(),
                err: String::new(),
            };
        }
        TmuxResult {
            code: 0,
            out: String::new(),
            err: String::new(),
        }
    }
}

#[test]
fn login_specs_cover_all_three_clis_with_verified_commands() {
    let clis: Vec<&str> = LOGIN_SPECS.iter().map(|s| s.cli).collect();
    assert_eq!(clis, vec!["claude", "agy", "opencode"]);

    let claude = LOGIN_SPECS.iter().find(|s| s.cli == "claude").unwrap();
    assert_eq!(claude.status_argv, &["claude", "auth", "status", "--json"]);
    assert_eq!(claude.login_argv, Some(&["claude", "auth", "login"][..]));

    let agy = LOGIN_SPECS.iter().find(|s| s.cli == "agy").unwrap();
    assert_eq!(agy.status_argv, &["agy", "models"]);
    assert_eq!(agy.login_argv, None);
    assert!(
        agy.login_hint
            .is_some_and(|h| h.to_lowercase().contains("own tui"))
    );

    let opencode = LOGIN_SPECS.iter().find(|s| s.cli == "opencode").unwrap();
    assert_eq!(opencode.status_argv, &["opencode", "auth", "list"]);
    assert_eq!(
        opencode.login_argv,
        Some(&["opencode", "auth", "login"][..])
    );
}

#[test]
fn check_status_parses_subscription_api_key_and_signed_out_states() {
    let probe = FakeLoginProbe::new()
        .with_cli(
            "claude",
            "2.3.1",
            0,
            r#"{"loggedIn":true,"authMethod":"claude.ai","subscriptionType":"pro"}"#,
            "",
        )
        .with_cli("agy", "1.4.0", 0, "gemini-4-neon-high\n", "")
        .with_cli("opencode", "0.5.0", 0, "No credentials stored\n", "");

    let claude = check_status("claude", &probe);
    assert!(claude.installed);
    assert_eq!(claude.version, "2.3.1");
    assert!(claude.signed_in);
    assert_eq!(claude.method.as_deref(), Some("subscription"));
    assert!(claude.ready);
    assert!(claude.has_login_command);

    let agy = check_status("agy", &probe);
    assert!(agy.installed);
    assert!(agy.signed_in);
    assert!(agy.ready);
    assert!(!agy.has_login_command);

    let opencode = check_status("opencode", &probe);
    assert!(opencode.installed);
    assert!(!opencode.signed_in);
    assert!(!opencode.ready);

    let missing_probe = FakeLoginProbe::new().with_cli(
        "claude",
        "2.3.1",
        0,
        r#"{"loggedIn":true,"authMethod":"apiKey"}"#,
        "",
    );
    let statuses = all_statuses(&missing_probe);
    assert_eq!(statuses.len(), 3);
    assert_eq!(statuses[0].method.as_deref(), Some("apiKey"));
    assert!(!statuses[1].installed);
    assert!(!statuses[1].ready);
}

#[test]
fn interactive_ready_depends_on_install_and_signin_not_headless_consent() {
    let probe = FakeLoginProbe::new()
        .with_cli(
            "claude",
            "2.3.1",
            0,
            r#"{"loggedIn":true,"authMethod":"claude.ai","subscriptionType":"pro"}"#,
            "",
        )
        .with_cli("agy", "1.4.0", 1, "", "not logged in");

    let claude = interactive_ready("claude", &probe);
    assert!(claude.ready);
    assert_eq!(claude.reason, None);

    let agy = interactive_ready("agy", &probe);
    assert!(!agy.ready);
    assert!(
        agy.reason
            .as_deref()
            .unwrap_or("")
            .contains("not signed in")
    );

    let opencode = interactive_ready("opencode", &probe);
    assert!(!opencode.ready);
    assert!(
        opencode
            .reason
            .as_deref()
            .unwrap_or("")
            .contains("not installed")
    );
}

#[test]
fn open_login_pane_tags_pane_as_login_and_never_sends_keys_or_captures() {
    let ws = TempDir::new();
    let probe = FakeLoginProbe::new()
        .with_cli("claude", "2.3.1", 0, r#"{"loggedIn":false}"#, "")
        .with_cli("agy", "1.4.0", 0, "", "");
    let runner = Arc::new(RecordingTmuxRunner::new());
    let client = TmuxClient::with_runner("tandem-test", None, runner.clone());

    let res = open_login_pane(&client, ws.path(), "claude", &probe).unwrap();
    assert_eq!(res.pane_id, "%77");
    assert_eq!(res.cli, "claude");

    let calls = runner.calls();
    assert!(calls.iter().any(|c| c.iter().any(|a| a == "new-window")));
    let joined: Vec<String> = calls.iter().map(|c| c.join(" ")).collect();
    assert!(joined.iter().any(|s| s.contains("@tandem_kind login")));
    assert!(joined.iter().any(|s| s.contains("@tandem_cli claude")));
    assert!(!joined.iter().any(|s| s.contains("send-keys")));
    assert!(!joined.iter().any(|s| s.contains("capture-pane")));

    let agy_err = open_login_pane(&client, ws.path(), "agy", &probe).unwrap_err();
    assert!(matches!(agy_err, LoginError::NoLoginSubcommand { .. }));

    let missing_err = open_login_pane(&client, ws.path(), "opencode", &probe).unwrap_err();
    assert!(matches!(missing_err, LoginError::NotInstalled(_)));
}

#[test]
fn factory_settings_persists_default_worker_cli_and_orchestrator_may_choose() {
    let ws = TempDir::new();
    let store = FactoryStore::new(ws.path());
    let caller = tandem::factory::approval::Caller::cli(true, &|_| None, &|_| None);

    let r1 = factory_settings::set(&store, "default_worker_cli", "claude", "you", &caller).unwrap();
    assert_eq!(r1.settings.default_worker_cli, "claude");

    let r2 =
        factory_settings::set(&store, "orchestrator_may_choose", "false", "you", &caller).unwrap();
    assert!(!r2.settings.orchestrator_may_choose);

    assert!(factory_settings::set(&store, "default_worker_cli", "bogus", "you", &caller).is_err());
}

#[test]
fn settings_tab_renders_agents_and_signin_and_supports_keyboard_and_mouse() {
    let ws = TempDir::new();
    let mut tab = SettingsTab::default();
    tab.configure_factory(true, Some(ws.path().to_path_buf()));
    tab.factory_logins = all_statuses(
        &FakeLoginProbe::new()
            .with_cli(
                "claude",
                "2.3.1",
                0,
                r#"{"loggedIn":true,"authMethod":"claude.ai","subscriptionType":"pro"}"#,
                "",
            )
            .with_cli("agy", "1.4.0", 1, "", "sign in required"),
    );

    let area = Rect::new(0, 0, 100, 45);
    let mut buf = Buffer::empty(area);
    tab.render(area, &mut buf);

    let mut screen_text = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            screen_text.push_str(buf[(x, y)].symbol());
        }
        screen_text.push('\n');
    }
    assert!(screen_text.contains("Agents & sign-in"));
    assert!(screen_text.contains("Signed in (subscription)"));
    assert!(screen_text.contains("Not signed in"));
    assert!(screen_text.contains("Not installed"));
    assert!(screen_text.contains("Default worker CLI"));
    assert!(screen_text.contains("Orchestrator may choose"));

    let find_line = |needle: &str| -> (u16, u16) {
        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                line.push_str(buf[(x, y)].symbol());
            }
            if let Some(byte) = line.find(needle) {
                return (line[..byte].chars().count() as u16, y);
            }
        }
        panic!("{needle:?} not rendered");
    };

    let (x, y) = find_line("Orchestrator may choose");
    tab.handle_mouse(
        &MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    assert!(!tab.factory_settings.orchestrator_may_choose);

    tab.handle_key(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    tab.handle_key(&KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_ne!(tab.factory_settings.default_worker_cli, "auto");
}
