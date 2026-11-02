use std::path::Path;
use tandem::cli::{Command, FocusTarget, TabDirection, build_parser, parse_from};

#[test]
fn test_version_matches_package_json() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let package_json_path = manifest_dir.parent().unwrap().join("package.json");
    let content = std::fs::read_to_string(&package_json_path).expect("read package.json");
    let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse package.json");
    let pkg_version = parsed["version"]
        .as_str()
        .expect("package.json has version string");
    let cargo_version = env!("CARGO_PKG_VERSION");
    assert_eq!(
        cargo_version, pkg_version,
        "Cargo.toml version ({cargo_version}) must match package.json ({pkg_version})"
    );
}

#[test]
fn test_version_flag_output() {
    let cmd = build_parser();
    let version = cmd.render_version();
    assert_eq!(
        version.trim(),
        format!("tandem {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn test_resume_and_fresh_conflict() {
    let result = parse_from(["tandem", "--resume", "--fresh"]);
    assert!(result.is_err(), "Expected --resume and --fresh to conflict");
}

#[test]
fn test_resume_flag_alone() {
    let cli = parse_from(["tandem", "--resume"]).expect("parse --resume");
    assert!(cli.resume);
    assert!(!cli.fresh);
    assert!(cli.command.is_none());
}

#[test]
fn test_fresh_flag_alone() {
    let cli = parse_from(["tandem", "--fresh"]).expect("parse --fresh");
    assert!(cli.fresh);
    assert!(!cli.resume);
    assert!(cli.command.is_none());
}

#[test]
#[allow(clippy::type_complexity)]
fn test_all_subcommands_accepted() {
    let cases: Vec<(&[&str], Box<dyn Fn(&Command) -> bool>)> = vec![
        (
            &["tandem", "add-agent"],
            Box::new(|c| matches!(c, Command::AddAgent)),
        ),
        (
            &["tandem", "tab", "prev"],
            Box::new(|c| match c {
                Command::Tab(args) => args.direction == TabDirection::Prev && args.number.is_none(),
                _ => false,
            }),
        ),
        (
            &["tandem", "tab", "next"],
            Box::new(|c| match c {
                Command::Tab(args) => args.direction == TabDirection::Next && args.number.is_none(),
                _ => false,
            }),
        ),
        (
            &["tandem", "tab", "select", "3"],
            Box::new(|c| match c {
                Command::Tab(args) => {
                    args.direction == TabDirection::Select && args.number == Some(3)
                }
                _ => false,
            }),
        ),
        (
            &["tandem", "focus", "planner"],
            Box::new(|c| match c {
                Command::Focus(args) => args.target == FocusTarget::Planner,
                _ => false,
            }),
        ),
        (
            &["tandem", "focus", "implementer"],
            Box::new(|c| match c {
                Command::Focus(args) => args.target == FocusTarget::Implementer,
                _ => false,
            }),
        ),
        (
            &["tandem", "focus", "status"],
            Box::new(|c| match c {
                Command::Focus(args) => args.target == FocusTarget::Status,
                _ => false,
            }),
        ),
        (
            &["tandem", "focus", "queries"],
            Box::new(|c| match c {
                Command::Focus(args) => args.target == FocusTarget::Queries,
                _ => false,
            }),
        ),
        (
            &["tandem", "close-agent"],
            Box::new(|c| matches!(c, Command::CloseAgent)),
        ),
        (
            &["tandem", "restart"],
            Box::new(|c| matches!(c, Command::Restart)),
        ),
        (
            &["tandem", "refresh"],
            Box::new(|c| matches!(c, Command::Refresh)),
        ),
        (
            &["tandem", "reflow"],
            Box::new(|c| matches!(c, Command::Reflow)),
        ),
        (
            &["tandem", "reset-layout"],
            Box::new(|c| matches!(c, Command::ResetLayout)),
        ),
        (
            &["tandem", "quit"],
            Box::new(|c| matches!(c, Command::Quit)),
        ),
        (
            &["tandem", "help"],
            Box::new(|c| matches!(c, Command::Help)),
        ),
        (
            &["tandem", "first-run"],
            Box::new(|c| matches!(c, Command::FirstRun)),
        ),
        (
            &["tandem", "status"],
            Box::new(|c| matches!(c, Command::Status)),
        ),
        (
            &["tandem", "whats-new"],
            Box::new(|c| matches!(c, Command::WhatsNew)),
        ),
        (
            &["tandem", "update"],
            Box::new(|c| match c {
                Command::Update(args) => !args.yes,
                _ => false,
            }),
        ),
        (
            &["tandem", "update", "--yes"],
            Box::new(|c| match c {
                Command::Update(args) => args.yes,
                _ => false,
            }),
        ),
        (
            &["tandem", "click"],
            Box::new(|c| match c {
                Command::Click(args) => args.action.is_empty(),
                _ => false,
            }),
        ),
        (
            &["tandem", "click", "add-agent"],
            Box::new(|c| match c {
                Command::Click(args) => args.action == "add-agent",
                _ => false,
            }),
        ),
        (
            &["tandem", "click-tab", "%1"],
            Box::new(|c| match c {
                Command::ClickTab(args) => args.pane == "%1" && args.select.is_empty(),
                _ => false,
            }),
        ),
        (
            &["tandem", "click-tab", "%1", "--select", "%2"],
            Box::new(|c| match c {
                Command::ClickTab(args) => args.pane == "%1" && args.select == "%2",
                _ => false,
            }),
        ),
        (
            &["tandem", "quota-feed"],
            Box::new(|c| matches!(c, Command::QuotaFeed)),
        ),
        (
            &["tandem", "quota-line"],
            Box::new(|c| matches!(c, Command::QuotaLine)),
        ),
        (
            &["tandem", "save-exit"],
            Box::new(|c| matches!(c, Command::SaveExit)),
        ),
        (
            &["tandem", "discard-exit"],
            Box::new(|c| matches!(c, Command::DiscardExit)),
        ),
        (
            &["tandem", "reload"],
            Box::new(|c| matches!(c, Command::Reload)),
        ),
        (
            &["tandem", "placeholder", "planner"],
            Box::new(|c| match c {
                Command::Placeholder(args) => args.role == "planner",
                _ => false,
            }),
        ),
    ];

    for (argv, validator) in cases {
        let parsed = parse_from(argv).unwrap_or_else(|err| {
            panic!("Failed to parse {:?}: {}", argv, err);
        });
        let cmd = parsed.command.as_ref().unwrap_or_else(|| {
            panic!("Expected subcommand for {:?}", argv);
        });
        assert!(validator(cmd), "Validation failed for {:?}", argv);
    }
}

#[test]
fn test_subcommand_invalid_arguments_rejected() {
    assert!(parse_from(["tandem", "tab", "invalid"]).is_err());
    assert!(parse_from(["tandem", "focus", "invalid"]).is_err());
    assert!(parse_from(["tandem", "click-tab"]).is_err());
    assert!(parse_from(["tandem", "placeholder"]).is_err());
    assert!(parse_from(["tandem", "--nope"]).is_err());
}

use std::sync::{Arc, Mutex};
use tandem::commands::{click, click_tab};
use tandem::config::Config;
use tandem::tmux::client::{Result as TmuxResult, TmuxClient, TmuxRunner};
use tandem::tmux::keys::CLICK_ACTIONS;
use tandem::tmux::layout::Layout;

struct MockCliRunner {
    calls: Mutex<Vec<Vec<String>>>,
    role: String,
    panes_output: String,
}

impl MockCliRunner {
    fn new(role: &str, panes_output: &str) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            role: role.to_string(),
            panes_output: panes_output.to_string(),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl TmuxRunner for MockCliRunner {
    fn run(&self, argv: &[String]) -> TmuxResult {
        self.calls.lock().unwrap().push(argv.to_vec());
        let mut out = String::new();
        if argv.contains(&"list-panes".to_string()) {
            out = self.panes_output.clone();
        } else if argv.contains(&"#{@tandem_role}".to_string()) {
            out = format!("{}\n", self.role);
        }
        TmuxResult {
            code: 0,
            out,
            err: String::new(),
        }
    }
}

fn sample_panes_tsv() -> String {
    [
        "%1\tmain\tagent\tplanner\t1\tclaude\t0\t\t\t{\"cli\":\"claude\",\"model\":\"opus\"}\t1.0",
        "%2\tstash\tagent\tplanner\t2\tagy\t1\t3\t\t{\"cli\":\"agy\",\"model\":\"m\"}\t1.0",
        "%9\tmain\tagent\timplementer\t1\tcodex\t0\t\t\t{\"cli\":\"codex\",\"model\":\"m\"}\t1.0",
    ]
    .join("\n")
}

#[test]
fn test_click_unknown_action_is_no_op() {
    let runner = Arc::new(MockCliRunner::new("planner", ""));
    let client = TmuxClient::with_runner("tandem-test", None, runner.clone());
    let mut layout = Layout::new(&client);
    let config = Config::from_env().unwrap();

    click(&mut layout, &config, "");
    click(&mut layout, &config, "BADACTION");
    click(&mut layout, &config, "send-keys");

    assert!(runner.calls().is_empty());
}

#[test]
fn test_click_allow_list_matches_click_actions() {
    assert!(CLICK_ACTIONS.contains(&"help"));
    assert!(CLICK_ACTIONS.contains(&"add-agent"));
    assert!(CLICK_ACTIONS.contains(&"quit"));
    assert!(!CLICK_ACTIONS.contains(&"send-keys"));
    assert!(!CLICK_ACTIONS.contains(&"capture-pane"));
    assert!(!CLICK_ACTIONS.contains(&""));
}

#[test]
fn test_click_tab_opens_a_menu_of_the_columns_agents() {
    let runner = Arc::new(MockCliRunner::new("planner", &sample_panes_tsv()));
    let client = TmuxClient::with_runner("tandem-test", None, runner.clone());
    let mut layout = Layout::new(&client);
    layout.self_cmd = Some(vec!["/bin/tandem".to_string()]);
    let config = Config::from_env().unwrap();

    click_tab(&mut layout, &config, "%1", "");

    let calls = runner.calls();
    let display_menu_calls: Vec<&Vec<String>> = calls
        .iter()
        .filter(|c| c.contains(&"display-menu".to_string()))
        .collect();
    assert_eq!(display_menu_calls.len(), 1);

    let flat = display_menu_calls[0].join(" ");
    assert!(flat.contains("-t %1"));
    assert!(flat.contains("#{pane_left}"));
    assert!(flat.contains("#{pane_top}"));
    assert!(flat.contains("▶ 1 claude"));
    assert!(flat.contains("click-tab %1 --select %1"));
    assert!(flat.contains("⚠2 agy(exited 3)"));
    assert!(flat.contains("click-tab %2 --select %2"));
    assert!(!flat.contains("codex"));
    assert!(flat.contains("New agent"));
    assert!(flat.contains("add-agent"));
}

#[test]
fn test_click_tab_empty_column_opens_add_agent() {
    let runner = Arc::new(MockCliRunner::new("planner", ""));
    let client = TmuxClient::with_runner("tandem-test", None, runner.clone());
    let mut layout = Layout::new(&client);
    layout.self_cmd = Some(vec!["/bin/tandem".to_string()]);
    let config = Config::from_env().unwrap();

    click_tab(&mut layout, &config, "%1", "");

    let calls = runner.calls();
    let popups: Vec<&Vec<String>> = calls
        .iter()
        .filter(|c| c.contains(&"display-popup".to_string()))
        .collect();
    assert_eq!(popups.len(), 1);
    assert!(popups[0].iter().any(|arg| arg.contains("add-agent")));
}

#[test]
fn test_click_tab_select_switches_to_that_agent() {
    let runner = Arc::new(MockCliRunner::new("planner", &sample_panes_tsv()));
    let client = TmuxClient::with_runner("tandem-test", None, runner.clone());
    let mut layout = Layout::new(&client);
    let config = Config::from_env().unwrap();

    click_tab(&mut layout, &config, "%1", "%2");

    let calls = runner.calls();
    assert!(calls.iter().any(|c| c.contains(&"swap-pane".to_string())));
}

#[test]
fn test_click_tab_select_ignores_other_columns_and_unknown_panes() {
    for bad in ["%9", "%77", "2", "%2; kill-server", "$(id)"] {
        let runner = Arc::new(MockCliRunner::new("planner", &sample_panes_tsv()));
        let client = TmuxClient::with_runner("tandem-test", None, runner.clone());
        let mut layout = Layout::new(&client);
        let config = Config::from_env().unwrap();

        click_tab(&mut layout, &config, "%1", bad);

        let calls = runner.calls();
        assert!(!calls.iter().any(|c| c.contains(&"swap-pane".to_string())));
        assert!(
            !calls
                .iter()
                .any(|c| c.contains(&"display-menu".to_string()))
        );
    }
}

#[test]
fn test_click_tab_ignores_invalid_pane_ids_and_non_column_panes() {
    for bad in ["", "1", "%x", "%1; kill-server", "\\%1"] {
        let runner = Arc::new(MockCliRunner::new("planner", &sample_panes_tsv()));
        let client = TmuxClient::with_runner("tandem-test", None, runner.clone());
        let mut layout = Layout::new(&client);
        let config = Config::from_env().unwrap();

        click_tab(&mut layout, &config, bad, "");

        let calls = runner.calls();
        assert!(
            !calls
                .iter()
                .any(|c| c.contains(&"display-menu".to_string()))
        );
        assert!(
            !calls
                .iter()
                .any(|c| c.contains(&"display-popup".to_string()))
        );
    }

    let runner_status = Arc::new(MockCliRunner::new("status", &sample_panes_tsv()));
    let client_status = TmuxClient::with_runner("tandem-test", None, runner_status.clone());
    let mut layout_status = Layout::new(&client_status);
    let config = Config::from_env().unwrap();

    click_tab(&mut layout_status, &config, "%3", "");

    let calls = runner_status.calls();
    assert!(
        !calls
            .iter()
            .any(|c| c.contains(&"display-menu".to_string()))
    );
    assert!(
        !calls
            .iter()
            .any(|c| c.contains(&"display-popup".to_string()))
    );
}

#[test]
fn test_no_forbidden_network_crates() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let lock_path = manifest_dir.join("Cargo.lock");
    if lock_path.exists() {
        let content = std::fs::read_to_string(&lock_path).expect("read Cargo.lock");
        let forbidden = ["reqwest", "ureq", "hyper", "curl"];
        for crate_name in forbidden {
            let pattern = format!("name = \"{}\"", crate_name);
            assert!(
                !content.contains(&pattern),
                "Forbidden network crate '{}' found in Cargo.lock",
                crate_name
            );
        }
    }
}
