use tandem::adapters::{
    AdapterError, AgyAdapter, Capabilities, ClaudeAdapter, HeadlessAdapter, HeadlessRequest,
    OutputFormat, ToolFlag,
};

const DENIED_PERMISSION_FLAGS: &[&str] = &[
    "--dangerously-skip-permissions",
    "--allow-dangerously-skip-permissions",
    "--yolo",
    "-y",
    "bypassPermissions",
    "--permission-mode=bypassPermissions",
];

fn assert_no_denied_flags(argv: &[String]) {
    for arg in argv {
        for denied in DENIED_PERMISSION_FLAGS {
            assert_ne!(
                arg, denied,
                "headless argv must never emit denied flag {denied}: {argv:?}"
            );
        }
    }
}

#[test]
fn test_claude_headless_capabilities() {
    let adapter = ClaudeAdapter::new();
    assert_eq!(
        adapter.capabilities(),
        Capabilities {
            prompt_on_stdin: true,
            structured_output: true,
            usage_reporting: true,
            temperature: false,
        }
    );
}

#[test]
fn test_agy_headless_capabilities() {
    let adapter = AgyAdapter::new();
    assert_eq!(
        adapter.capabilities(),
        Capabilities {
            prompt_on_stdin: true,
            structured_output: true,
            usage_reporting: false,
            temperature: false,
        }
    );
}

#[test]
fn test_claude_headless_golden_argv_text() {
    let adapter = ClaudeAdapter::new();
    let req = HeadlessRequest {
        model: "sonnet".to_string(),
        tools: Vec::new(),
        temperature: None,
        output: OutputFormat::Text,
    };
    let argv = adapter.build_headless_argv(&req).unwrap();
    assert_eq!(
        argv,
        vec![
            "claude",
            "-p",
            "--model",
            "sonnet",
            "--output-format",
            "text"
        ]
    );
    assert_no_denied_flags(&argv);
}

#[test]
fn test_claude_headless_golden_argv_json_with_tools_and_ignored_temperature() {
    let adapter = ClaudeAdapter::new();
    let req = HeadlessRequest {
        model: "claude-sonnet-4-5".to_string(),
        tools: vec![
            ToolFlag::Allow("Read".to_string()),
            ToolFlag::Allow("Edit".to_string()),
            ToolFlag::Disallow("Bash".to_string()),
        ],
        temperature: Some(0.2),
        output: OutputFormat::Json,
    };
    let argv = adapter.build_headless_argv(&req).unwrap();
    assert_eq!(
        argv,
        vec![
            "claude",
            "-p",
            "--model",
            "claude-sonnet-4-5",
            "--output-format",
            "json",
            "--allowedTools",
            "Read,Edit",
            "--disallowedTools",
            "Bash",
        ]
    );
    assert_no_denied_flags(&argv);
}

#[test]
fn test_claude_headless_golden_argv_stream_json() {
    let adapter = ClaudeAdapter::new();
    let req = HeadlessRequest {
        model: "opus".to_string(),
        tools: Vec::new(),
        temperature: None,
        output: OutputFormat::StreamJson,
    };
    let argv = adapter.build_headless_argv(&req).unwrap();
    assert_eq!(
        argv,
        vec![
            "claude",
            "-p",
            "--model",
            "opus",
            "--output-format",
            "stream-json"
        ]
    );
    assert_no_denied_flags(&argv);
}

#[test]
fn test_agy_headless_golden_argv_and_unsupported_fields_omitted() {
    let adapter = AgyAdapter::new();

    for (output, expected_format) in [
        (OutputFormat::Text, "text"),
        (OutputFormat::Json, "json"),
        (OutputFormat::StreamJson, "stream-json"),
    ] {
        let req = HeadlessRequest {
            model: "gemini-2.5-pro".to_string(),
            tools: vec![ToolFlag::Allow("Read".to_string())],
            temperature: Some(0.4),
            output,
        };
        let argv = adapter.build_headless_argv(&req).unwrap();
        assert_eq!(
            argv,
            vec![
                "agy",
                "--print",
                "--model",
                "gemini-2.5-pro",
                "--output-format",
                expected_format,
            ]
        );
        assert_no_denied_flags(&argv);
    }
}

#[test]
fn test_headless_rejects_invalid_model_ids() {
    let claude = ClaudeAdapter::new();
    let agy = AgyAdapter::new();
    let bad_models = ["", "--evil", "-p", "a;rm -rf /", "a b", "$(id)"];

    for bad in bad_models {
        let req = HeadlessRequest {
            model: bad.to_string(),
            tools: Vec::new(),
            temperature: None,
            output: OutputFormat::Json,
        };
        assert!(
            matches!(
                claude.build_headless_argv(&req),
                Err(AdapterError::InvalidModel)
            ),
            "claude should reject invalid model {bad:?}"
        );
        assert!(
            matches!(
                agy.build_headless_argv(&req),
                Err(AdapterError::InvalidModel)
            ),
            "agy should reject invalid model {bad:?}"
        );
    }
}

#[test]
fn test_headless_rejects_flag_injection_in_tool_names() {
    let claude = ClaudeAdapter::new();
    for bad_tool in ["", "--dangerously-skip-permissions", "-x", "Read\n--yolo"] {
        let req = HeadlessRequest {
            model: "sonnet".to_string(),
            tools: vec![ToolFlag::Allow(bad_tool.to_string())],
            temperature: None,
            output: OutputFormat::Json,
        };
        assert!(
            matches!(
                claude.build_headless_argv(&req),
                Err(AdapterError::InvalidInput(_))
            ),
            "claude should reject invalid tool flag {bad_tool:?}"
        );
    }
}
