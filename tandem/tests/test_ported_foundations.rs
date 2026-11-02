use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tandem::config::Config;
use tandem::sanitize::clean;
use tandem::selfexec::{is_frozen, self_argv, self_command, shlex_quote};
use tandem::versions::verify_release_versions;

#[test]
fn test_config_ported_cases() {
    let empty = HashMap::new();
    let cfg = Config::from_env_source(&empty).unwrap();
    assert_eq!(cfg.coord_dir, None);
    assert!(cfg.color);
    assert_eq!(cfg.tmux_pane, "");
    assert_eq!(cfg.session_id, "");
    assert_eq!(cfg.update_interval, 14400);
    assert!(!cfg.update_check);

    let mut env = HashMap::new();
    env.insert("TANDEM_COORD_DIR".to_string(), "/tmp/coord".to_string());
    assert_eq!(
        Config::from_env_source(&env).unwrap().coord_dir,
        Some(PathBuf::from("/tmp/coord"))
    );

    let mut rel_env = HashMap::new();
    rel_env.insert("TANDEM_COORD_DIR".to_string(), "relative/path".to_string());
    assert!(Config::from_env_source(&rel_env).is_err());

    let mut ws_env = HashMap::new();
    ws_env.insert("TANDEM_WORKSPACE".to_string(), "/workspace".to_string());
    assert_eq!(
        Config::from_env_source(&ws_env).unwrap().workspace,
        Some(PathBuf::from("/workspace"))
    );

    let mut rel_ws = HashMap::new();
    rel_ws.insert("TANDEM_WORKSPACE".to_string(), "relative/ws".to_string());
    assert_eq!(Config::from_env_source(&rel_ws).unwrap().workspace, None);

    let mut spec_env = HashMap::new();
    spec_env.insert(
        "TANDEM_UPDATE_SPEC".to_string(),
        "custom/repo#main".to_string(),
    );
    assert_eq!(
        Config::from_env_source(&spec_env).unwrap().update_spec,
        "custom/repo#main"
    );

    let mut bad_spec = HashMap::new();
    bad_spec.insert(
        "TANDEM_UPDATE_SPEC".to_string(),
        "invalid spec with spaces".to_string(),
    );
    assert!(Config::from_env_source(&bad_spec).is_err());
}

#[test]
fn test_sanitize_ported_cases() {
    assert_eq!(clean("normal text"), "normal text");
    assert_eq!(clean("line 1\r\nline 2"), "line 1\nline 2");
    assert_eq!(
        clean("\x1b[31mred\x1b[0m and \x1b[1mbold\x1b[0m"),
        "red and bold"
    );
    assert_eq!(clean("\x1b]0;Title\x07Text"), "Text");
    assert_eq!(clean("\x1b]0;Title\x1b\\Text"), "Text");
    assert_eq!(clean("Null\x00byte and bell\x07"), "Nullbyte and bell");
    assert_eq!(
        clean("Tabs\tand\nnewlines\r\nkept"),
        "Tabs\tand\nnewlines\nkept"
    );
}

#[test]
fn test_selfexec_ported_cases() {
    assert!(!is_frozen());
    let argv = self_argv();
    assert!(!argv.is_empty());
    assert!(argv[0].contains("tandem"));

    assert_eq!(shlex_quote(""), "''");
    assert_eq!(shlex_quote("word"), "word");
    assert_eq!(shlex_quote("with space"), "'with space'");
    assert_eq!(shlex_quote("it's"), "'it'\\''s'");

    let custom = ["/custom/bin", "status", "--yes"];
    assert_eq!(self_command(Some(&custom)), "/custom/bin status --yes");
}

#[test]
fn test_verify_release_versions_checks_tag_package_and_cargo() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.parent().unwrap();
    let pkg_json = repo_root.join("package.json");
    let content = std::fs::read_to_string(pkg_json).expect("read package.json");
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    let version = parsed["version"].as_str().unwrap();

    let tag = format!("v{version}");
    let (ok, msg) = verify_release_versions(&tag, repo_root);
    assert!(ok, "verify_release_versions failed: {msg}");
    assert!(msg.contains("all versions match"));

    let bad_tag = "v0.0.1";
    let (bad_ok, _) = verify_release_versions(bad_tag, repo_root);
    assert!(!bad_ok, "Expected failure for mismatching tag version");

    let no_v_tag = version;
    let (no_v_ok, no_v_msg) = verify_release_versions(no_v_tag, repo_root);
    assert!(!no_v_ok);
    assert!(no_v_msg.contains("does not start with 'v'"));
}
