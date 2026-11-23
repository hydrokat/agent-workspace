use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use tandem::factory::worker::env::{build_env, validate_worktree};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-worker-env-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn environment_is_strictly_allow_listed_and_drops_secrets() {
    let parent = BTreeMap::from([
        ("PATH".to_string(), "/usr/bin".to_string()),
        ("HOME".to_string(), "/tmp/tandem-test-home".to_string()),
        ("LANG".to_string(), "en_US.UTF-8".to_string()),
        ("LC_ALL".to_string(), "en_US.UTF-8".to_string()),
        ("TERM".to_string(), "xterm-256color".to_string()),
        (
            "CLAUDE_CONFIG_DIR".to_string(),
            "/tmp/tandem-test-claude-cfg".to_string(),
        ),
        (
            "AGY_CONFIG_DIR".to_string(),
            "/tmp/not-for-claude".to_string(),
        ),
        ("ANTHROPIC_API_KEY".to_string(), "sk-ant-secret".to_string()),
        ("OPENAI_API_KEY".to_string(), "sk-openai-secret".to_string()),
        (
            "CLAUDE_CODE_OAUTH_TOKEN".to_string(),
            "oauth-secret".to_string(),
        ),
        (
            "AWS_SECRET_ACCESS_KEY".to_string(),
            "aws-secret".to_string(),
        ),
        ("CUSTOM_SECRET".to_string(), "should-not-leak".to_string()),
    ]);
    let factory = BTreeMap::from([
        ("TANDEM_SESSION_ID".to_string(), "impl-1".to_string()),
        ("TANDEM_ROLE".to_string(), "implementer".to_string()),
        ("TANDEM_RUN_ID".to_string(), "run-1".to_string()),
        ("EVIL_INJECTED".to_string(), "nope".to_string()),
        ("TANDEM_SECRET_TOKEN".to_string(), "nope-token".to_string()),
    ]);

    let env = build_env("claude", &parent, &factory);

    for (key, value) in [
        ("HOME", "/tmp/tandem-test-home"),
        ("LANG", "en_US.UTF-8"),
        ("LC_ALL", "en_US.UTF-8"),
        ("TERM", "xterm-256color"),
        ("CLAUDE_CONFIG_DIR", "/tmp/tandem-test-claude-cfg"),
        ("TANDEM_SESSION_ID", "impl-1"),
        ("TANDEM_ROLE", "implementer"),
        ("TANDEM_RUN_ID", "run-1"),
    ] {
        assert_eq!(env.get(key).map(String::as_str), Some(value), "{key}");
    }
    for forbidden in [
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "AWS_SECRET_ACCESS_KEY",
        "CUSTOM_SECRET",
        "EVIL_INJECTED",
        "TANDEM_SECRET_TOKEN",
        "AGY_CONFIG_DIR",
    ] {
        assert!(!env.contains_key(forbidden), "{forbidden} leaked: {env:?}");
    }
}

#[test]
fn rejects_cwd_outside_worktrees_and_symlinks() {
    let ws = temp_dir("cwd");
    let worktrees = ws.join("worktrees");
    let valid = worktrees.join("run-1");
    let outside = temp_dir("outside");
    fs::create_dir_all(&valid).unwrap();
    let evil_link = worktrees.join("evil-link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &evil_link).unwrap();

    for bad in [
        ws.clone(),
        outside.clone(),
        worktrees.clone(),
        evil_link,
        valid.join("..").join(".."),
    ] {
        let err = validate_worktree(&ws, &bad).unwrap_err();
        assert!(err.to_string().contains("invalid worktree path"), "{err}");
    }

    let ok = validate_worktree(&ws, &valid).unwrap();
    assert!(ok.starts_with(worktrees.canonicalize().unwrap()));

    let _ = fs::remove_dir_all(&ws);
    let _ = fs::remove_dir_all(&outside);
}
