use std::sync::{Arc, Mutex};
use std::time::Duration;

use tandem::adapters::{
    Adapter, AdapterError, AgentSpec, AgyAdapter, CLAUDE_ALIASES, ClaudeAdapter, adapter_for,
    parse_models, session_env, session_id, session_name, validate_model_id,
};

fn spec(cli: &str, role: &str, model: &str) -> AgentSpec {
    AgentSpec::new(cli, role, model)
}

#[test]
fn test_claude_argv_snapshot() {
    let adapter = ClaudeAdapter::new();
    let s = spec("claude", "planner", "opus");
    let argv = adapter.build_argv(&s, false, None).unwrap();
    assert_eq!(
        &argv[..7],
        &[
            "claude",
            "--permission-mode",
            "auto",
            "--model",
            "opus",
            "-n",
            "tandem-planner-1"
        ]
    );
    assert_eq!(argv[7], "--settings");
    assert_eq!(argv.len(), 9);
}

#[test]
fn test_claude_settings_only_set_the_statusline_to_the_quota_feed() {
    let adapter = ClaudeAdapter::with_self_cmd(vec!["tandem".to_string()]);
    let s = spec("claude", "planner", "opus");
    let argv = adapter.build_argv(&s, false, None).unwrap();
    let settings: serde_json::Value = serde_json::from_str(&argv[8]).unwrap();
    let obj = settings.as_object().unwrap();
    assert_eq!(obj.len(), 1);
    assert_eq!(settings["statusLine"]["type"], "command");
    assert!(
        settings["statusLine"]["command"]
            .as_str()
            .unwrap()
            .ends_with("tandem quota-feed")
    );
}

#[test]
fn test_claude_always_auto_permission_mode() {
    let adapter = ClaudeAdapter::new();
    let mut s = spec("claude", "implementer", "claude-sonnet-5-5");
    s.index = 3;
    let argv = adapter.build_argv(&s, false, None).unwrap();
    assert_eq!(&argv[1..3], &["--permission-mode", "auto"]);
    let n_idx = argv.iter().position(|a| a == "-n").unwrap();
    assert_eq!(argv[n_idx + 1], "tandem-implementer-3");
}

#[test]
fn test_claude_rejects_skip_permissions() {
    let adapter = ClaudeAdapter::new();
    let mut s = spec("claude", "planner", "opus");
    s.skip_permissions = true;
    let err = adapter.build_argv(&s, false, None);
    assert!(matches!(err, Err(AdapterError::InvalidInput(_))));
}

#[test]
fn test_agy_argv_snapshot() {
    let adapter = AgyAdapter::new();
    let s = spec("agy", "planner", "gemini-3.8-flash-high");
    let argv = adapter.build_argv(&s, false, None).unwrap();
    assert_eq!(argv, vec!["agy", "--model", "gemini-3.8-flash-high"]);
}

#[test]
fn test_agy_skip_permissions_flag_only_when_chosen() {
    let adapter = AgyAdapter::new();
    let mut s = spec("agy", "planner", "m1");
    s.skip_permissions = true;
    let argv = adapter.build_argv(&s, false, None).unwrap();
    assert_eq!(
        argv,
        vec!["agy", "--model", "m1", "--dangerously-skip-permissions"]
    );
}

#[test]
fn test_model_id_injection_rejected() {
    let bad_models = [
        "",
        "--evil-flag",
        "-x",
        "a;rm -rf /",
        "a b",
        "$(id)",
        "`id`",
        "a\nb",
        "a/b",
        &"x".repeat(200),
        "\u{202e}evil",
        "a&b",
        "a|b",
    ];

    for bad in bad_models {
        assert!(
            validate_model_id(bad).is_err(),
            "Expected error for model id: {}",
            bad
        );
        let claude = ClaudeAdapter::new();
        assert!(
            claude
                .build_argv(&spec("claude", "planner", bad), false, None)
                .is_err()
        );
        let agy = AgyAdapter::new();
        assert!(
            agy.build_argv(&spec("agy", "planner", bad), false, None)
                .is_err()
        );
    }
}

#[test]
fn test_valid_model_ids() {
    let good_models = [
        "opus",
        "claude-sonnet-5-5",
        "gemini-3.8-flash-high",
        "a.b:c_d-1",
    ];
    for good in good_models {
        assert_eq!(validate_model_id(good).unwrap(), good);
    }
}

#[test]
fn test_session_naming_and_env() {
    let mut s = spec("agy", "implementer", "m1");
    s.index = 2;
    assert_eq!(session_id(&s), "implementer-2");
    assert_eq!(session_name(&s), "tandem-implementer-2");

    let env = session_env(&s, "/w/.agent-workspace/coordination");
    assert_eq!(env.get("TANDEM_SESSION_ID").unwrap(), "implementer-2");
    assert_eq!(env.get("TANDEM_ROLE").unwrap(), "implementer");
    assert_eq!(env.get("TANDEM_CLI").unwrap(), "agy");
    assert_eq!(
        env.get("TANDEM_COORD_DIR").unwrap(),
        "/w/.agent-workspace/coordination"
    );
    assert_eq!(env.len(), 4);
}

#[test]
fn test_adapter_lookup() {
    assert_eq!(adapter_for("claude").unwrap().cli(), "claude");
    assert_eq!(adapter_for("agy").unwrap().cli(), "agy");
    assert!(adapter_for("gemini").is_err());
}

#[test]
fn test_claude_models_are_aliases() {
    let adapter = ClaudeAdapter::new();
    let models = adapter.list_models().unwrap();
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, CLAUDE_ALIASES);
}

const AGY_OUT: &str = "Fetching available models...\n\
gemini-3.8-flash-high\tGemini 3.8 Flash (High)\n\
claude-opus-5-5-low\tClaude Opus 5.5 (Low)\n";

#[test]
fn test_parse_models_skips_header() {
    let models = parse_models(AGY_OUT);
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, "gemini-3.8-flash-high");
    assert_eq!(models[0].label, "Gemini 3.8 Flash (High)");
    assert_eq!(models[1].id, "claude-opus-5-5-low");
    assert_eq!(models[1].label, "Claude Opus 5.5 (Low)");
}

#[test]
fn test_parse_models_garbage_yields_nothing() {
    assert!(parse_models("\x1b[31mBOOM\x1b[0m\n; rm -rf /\t--evil\n\n").is_empty());
}

#[test]
fn test_list_models_uses_runner_with_timeout() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_clone = Arc::clone(&seen);

    let adapter = AgyAdapter::with_runner(move |argv: &[&str], timeout: Duration| {
        let cmd = argv.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        seen_clone.lock().unwrap().push((cmd, timeout));
        Ok((0, AGY_OUT.to_string()))
    });

    let models = adapter.list_models().unwrap();
    assert_eq!(models.len(), 2);
    let recorded = seen.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].0, vec!["agy", "models"]);
    assert_eq!(recorded[0].1, Duration::from_secs(10));
}

#[test]
fn test_list_models_failure_raises_model_list_error() {
    let failing = AgyAdapter::with_runner(|_argv, _timeout| Ok((1, "nope".to_string())));
    assert!(matches!(
        failing.list_models(),
        Err(AdapterError::ModelList(_))
    ));

    let err_runner = AgyAdapter::with_runner(|_argv, _timeout| {
        Err(AdapterError::ModelList("timeout".to_string()))
    });
    assert!(matches!(
        err_runner.list_models(),
        Err(AdapterError::ModelList(_))
    ));
}

const CONV: &str = "46f2fc12-7f1f-4ca2-b714-06cbbfd669e9";

#[test]
fn test_claude_starts_under_its_conversation_id_and_resumes_it() {
    let adapter = ClaudeAdapter::new();
    let mut s = spec("claude", "planner", "opus");
    s.conversation = Some(CONV.to_string());

    let fresh = adapter.build_argv(&s, false, None).unwrap();
    assert_eq!(&fresh[fresh.len() - 2..], &["--session-id", CONV]);
    assert!(!fresh.contains(&"--resume".to_string()));

    let resumed = adapter.build_argv(&s, true, None).unwrap();
    assert_eq!(&resumed[resumed.len() - 2..], &["--resume", CONV]);
    assert!(!resumed.contains(&"--session-id".to_string()));
}

#[test]
fn test_conversation_ids_must_be_uuids() {
    let claude = ClaudeAdapter::new();
    let mut sc = spec("claude", "planner", "opus");
    sc.conversation = Some("--dangerous".to_string());
    assert!(matches!(
        claude.build_argv(&sc, false, None),
        Err(AdapterError::InvalidInput(_))
    ));

    let agy = AgyAdapter::new();
    let mut sa = spec("agy", "planner", "m1");
    sa.conversation = Some("--x".to_string());
    assert!(matches!(
        agy.build_argv(&sa, true, None),
        Err(AdapterError::InvalidInput(_))
    ));
}

#[test]
fn test_agy_logs_to_tandem_file_and_reopens_on_resume() {
    let adapter = AgyAdapter::new();
    let mut s = spec("agy", "planner", "m1");
    s.conversation = Some(CONV.to_string());

    let argv = adapter
        .build_argv(&s, false, Some("/c/conversations/agy-planner-1.log"))
        .unwrap();
    assert_eq!(
        argv,
        vec![
            "agy",
            "--model",
            "m1",
            "--log-file",
            "/c/conversations/agy-planner-1.log"
        ]
    );

    let resumed = adapter.build_argv(&s, true, None).unwrap();
    assert_eq!(&resumed[resumed.len() - 2..], &["--conversation", CONV]);

    let relative_err =
        adapter.build_argv(&spec("agy", "planner", "m1"), false, Some("relative.log"));
    assert!(matches!(relative_err, Err(AdapterError::InvalidInput(_))));
}
