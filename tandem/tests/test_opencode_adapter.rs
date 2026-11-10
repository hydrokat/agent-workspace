use std::sync::{Arc, Mutex};

use tandem::adapters::{
    Adapter, AdapterError, AgentSpec, AgyAdapter, ClaudeAdapter, OpencodeAdapter, adapter_for,
    parse_opencode_models, validate_model_id, validate_opencode_model_id,
};
use tandem::conversations::valid_opencode_session_id;

fn spec(model: &str) -> AgentSpec {
    AgentSpec::new("opencode", "implementer", model)
}

#[test]
fn adapter_for_knows_opencode_and_still_rejects_unknown_clis() {
    let adapter = adapter_for("opencode").unwrap();
    assert_eq!(adapter.cli(), "opencode");
    assert!(matches!(
        adapter_for("nope"),
        Err(AdapterError::UnknownCli(_))
    ));
}

#[test]
fn valid_opencode_model_ids() {
    for good in [
        "anthropic/claude-sonnet-5-5",
        "openai/gpt-5.4#high",
        "opencode/big-pickle",
        "openrouter/anthropic.claude:beta_1",
        "a/b",
    ] {
        assert_eq!(validate_opencode_model_id(good).unwrap(), good, "{good}");
    }
}

#[test]
fn opencode_model_ids_reject_anything_that_is_not_provider_slash_model() {
    let long_model = format!("p/{}", "x".repeat(99));
    let bad = [
        "",
        "claude",
        "/model",
        "provider/",
        "a/b/c",
        "../x",
        "a/../b",
        "-m/x",
        "a/-x",
        "a b/c",
        "a/b c",
        "a/b;rm",
        "a/$(id)",
        "a/`id`",
        "a/b\nc",
        "a/b#",
        "a/b#c#d",
        "a/#high",
        "a/b#h i",
        "a/b\"c",
        "\u{202e}a/b",
        "p/\u{202e}evil",
        long_model.as_str(),
    ];
    for model in bad {
        assert!(
            validate_opencode_model_id(model).is_err(),
            "expected rejection: {model:?}"
        );
        assert!(
            OpencodeAdapter::new()
                .build_argv(&spec(model), false, None)
                .is_err(),
            "build_argv must validate: {model:?}"
        );
        assert!(OpencodeAdapter::new().env(&spec(model)).is_err());
    }
}

#[test]
fn opencode_model_id_at_the_length_limit_is_accepted() {
    let id = format!("p/{}", "x".repeat(98));
    assert_eq!(id.len(), 100);
    assert!(validate_opencode_model_id(&id).is_ok());
    let over = format!("p/{}", "x".repeat(99));
    assert!(validate_opencode_model_id(&over).is_err());
}

#[test]
fn the_strict_validator_for_other_clis_is_unchanged() {
    assert!(validate_model_id("anthropic/claude").is_err());
    assert!(
        ClaudeAdapter::new()
            .build_argv(&AgentSpec::new("claude", "p", "a/b"), false, None)
            .is_err()
    );
    assert!(
        AgyAdapter::new()
            .build_argv(&AgentSpec::new("agy", "p", "a/b"), false, None)
            .is_err()
    );
}

#[test]
fn fresh_launch_is_just_the_binary() {
    let argv = OpencodeAdapter::new()
        .build_argv(&spec("anthropic/claude-sonnet-5-5"), false, None)
        .unwrap();
    assert_eq!(argv, vec!["opencode"]);
}

#[test]
fn model_travels_in_the_environment_as_serialized_json() {
    let env = OpencodeAdapter::new()
        .env(&spec("openai/gpt-5.4#high"))
        .unwrap();
    assert_eq!(env.len(), 1);
    let doc: serde_json::Value = serde_json::from_str(&env["OPENCODE_CONFIG_CONTENT"]).unwrap();
    assert_eq!(doc, serde_json::json!({ "model": "openai/gpt-5.4#high" }));
}

#[test]
fn other_adapters_add_no_environment() {
    assert!(
        ClaudeAdapter::new()
            .env(&AgentSpec::new("claude", "p", "opus"))
            .unwrap()
            .is_empty()
    );
    assert!(
        AgyAdapter::new()
            .env(&AgentSpec::new("agy", "p", "m1"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn skip_permissions_maps_to_auto() {
    let mut s = spec("anthropic/claude-sonnet-5-5");
    s.skip_permissions = true;
    let argv = OpencodeAdapter::new().build_argv(&s, false, None).unwrap();
    assert_eq!(argv, vec!["opencode", "--auto"]);
}

#[test]
fn resume_adds_the_validated_session_id() {
    let mut s = spec("anthropic/claude-sonnet-5-5");
    s.conversation = Some("ses_0123456789abcdef".to_string());
    let adapter = OpencodeAdapter::new();
    assert_eq!(
        adapter.build_argv(&s, true, None).unwrap(),
        vec!["opencode", "--session", "ses_0123456789abcdef"]
    );
    assert_eq!(
        adapter.build_argv(&s, false, None).unwrap(),
        vec!["opencode"]
    );
}

#[test]
fn resume_without_a_conversation_is_a_fresh_launch() {
    let argv = OpencodeAdapter::new()
        .build_argv(&spec("a/b"), true, None)
        .unwrap();
    assert_eq!(argv, vec!["opencode"]);
}

#[test]
fn resume_rejects_a_session_id_that_could_inject_arguments() {
    for bad in [
        "--auto",
        "-s",
        "a b",
        "x;y",
        "short",
        "a$(id)bcdefgh",
        "ses/../x",
        "",
    ] {
        let mut s = spec("a/b");
        s.conversation = Some(bad.to_string());
        assert!(
            matches!(
                OpencodeAdapter::new().build_argv(&s, true, None),
                Err(AdapterError::InvalidInput(_))
            ),
            "{bad:?}"
        );
    }
}

#[test]
fn session_id_validator_is_conservative() {
    for good in [
        "ses_0123456789abcdef",
        "abcdefgh",
        "A_b-c_d-e1234567",
        &"x".repeat(64),
    ] {
        assert!(valid_opencode_session_id(good).is_some(), "{good}");
    }
    for bad in [
        "",
        "short",
        "-abcdefgh",
        "abc defgh",
        "ab/cdefgh",
        &"x".repeat(65),
    ] {
        assert!(valid_opencode_session_id(bad).is_none(), "{bad:?}");
    }
}

#[test]
fn the_log_file_is_ignored() {
    let argv = OpencodeAdapter::new()
        .build_argv(&spec("a/b"), false, Some("/tmp/x.log"))
        .unwrap();
    assert_eq!(argv, vec!["opencode"]);
}

#[test]
fn parse_opencode_models_keeps_valid_ids_and_uses_the_id_as_label() {
    let out = "opencode/big-pickle\nanthropic/claude-sonnet-5-5\n\n  openai/gpt-5.4  \nnot a model\n../evil/x\na/b/c\n--flag\n";
    let models = parse_opencode_models(out);
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "opencode/big-pickle",
            "anthropic/claude-sonnet-5-5",
            "openai/gpt-5.4"
        ]
    );
    assert!(models.iter().all(|m| m.label == m.id));
}

#[test]
fn parse_opencode_models_strips_control_characters_and_ansi() {
    let models = parse_opencode_models("opencode/x\r\n\u{1b}[31manthropic/y\u{1b}[0m\n");
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["opencode/x"]);
}

#[test]
fn list_models_runs_opencode_models_standalone_with_a_timeout() {
    type Call = (Vec<String>, u64);
    let seen: Arc<Mutex<Vec<Call>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let adapter = OpencodeAdapter::with_runner(move |argv, timeout| {
        sink.lock().unwrap().push((
            argv.iter().map(|s| s.to_string()).collect(),
            timeout.as_secs(),
        ));
        Ok((
            0,
            "opencode/big-pickle\nanthropic/claude-sonnet-5-5\n".to_string(),
        ))
    });
    let models = adapter.list_models().unwrap();
    assert_eq!(models.len(), 2);
    let calls = seen.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, vec!["opencode", "models", "--standalone"]);
    assert_eq!(calls[0].1, 10);
}

#[test]
fn list_models_fails_on_nonzero_exit_and_on_empty_output() {
    let failing = OpencodeAdapter::with_runner(|_, _| Ok((1, "x/y\n".to_string())));
    assert!(matches!(
        failing.list_models(),
        Err(AdapterError::ModelList(_))
    ));
    let empty = OpencodeAdapter::with_runner(|_, _| Ok((0, "\n\nnot a model\n".to_string())));
    assert!(matches!(
        empty.list_models(),
        Err(AdapterError::ModelList(_))
    ));
    let runner_error = OpencodeAdapter::with_runner(|_, _| {
        Err(AdapterError::ModelList("spawn failed".to_string()))
    });
    assert!(matches!(
        runner_error.list_models(),
        Err(AdapterError::ModelList(_))
    ));
}
