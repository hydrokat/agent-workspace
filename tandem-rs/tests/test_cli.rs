use std::path::Path;
use tandem::cli::{CliError, Command, FocusTarget, TabDirection, build_parser, parse_from};

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

#[test]
fn test_dispatch_returns_not_implemented() {
    let subcommands = [
        vec!["tandem", "add-agent"],
        vec!["tandem", "tab", "prev"],
        vec!["tandem", "focus", "planner"],
        vec!["tandem", "close-agent"],
        vec!["tandem", "restart"],
        vec!["tandem", "refresh"],
        vec!["tandem", "reflow"],
        vec!["tandem", "reset-layout"],
        vec!["tandem", "quit"],
        vec!["tandem", "help"],
        vec!["tandem", "first-run"],
        vec!["tandem", "status"],
        vec!["tandem", "whats-new"],
        vec!["tandem", "update"],
        vec!["tandem", "click"],
        vec!["tandem", "click-tab", "%1"],
        vec!["tandem", "quota-feed"],
        vec!["tandem", "quota-line"],
        vec!["tandem", "save-exit"],
        vec!["tandem", "discard-exit"],
        vec!["tandem", "reload"],
        vec!["tandem", "placeholder", "planner"],
    ];

    for argv in subcommands {
        let cli = parse_from(&argv).expect("parse subcommand");
        let result = tandem::cli::dispatch(&cli);
        match result {
            Err(CliError::NotImplemented) => {}
            other => panic!("Expected NotImplemented for {:?}, got {:?}", argv, other),
        }
    }
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
