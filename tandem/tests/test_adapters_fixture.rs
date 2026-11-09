mod common;

use common::parity::load_fixture_value;
use tandem::adapters::{Adapter, AgentSpec, AgyAdapter, ClaudeAdapter, statusline_settings};

#[test]
fn test_adapters_parity_fixture() {
    let fixture = load_fixture_value("adapters");

    let custom_settings = statusline_settings(Some(&["/usr/local/bin/tandem"]));
    assert_eq!(
        custom_settings,
        fixture["statusline_settings"]["custom"].as_str().unwrap()
    );

    let claude_models = ClaudeAdapter::new().list_models().unwrap();
    let expected_claude: Vec<String> =
        serde_json::from_value(fixture["claude_models"].clone()).unwrap();
    let ids: Vec<String> = claude_models.into_iter().map(|m| m.id).collect();
    assert_eq!(ids, expected_claude);

    let matrix = fixture["build_argv_matrix"].as_array().unwrap();
    assert_eq!(matrix.len(), 192);

    for case in matrix {
        let cli = case["cli"].as_str().unwrap();
        let role = case["role"].as_str().unwrap();
        let model = case["model"].as_str().unwrap();
        let skip_permissions = case["skip_permissions"].as_bool().unwrap();
        let resume = case["resume"].as_bool().unwrap();
        let log_file = case["log_file"].as_str();
        let conversation = case["conversation"].as_str().map(|s| s.to_string());

        let spec = AgentSpec {
            cli: cli.to_string(),
            role: role.to_string(),
            model: model.to_string(),
            index: 1,
            skip_permissions,
            cwd: Some("/workspace".to_string()),
            conversation,
            name: None,
        };

        let (adapter_claude, adapter_agy);
        let adapter: &dyn Adapter = if cli == "claude" {
            adapter_claude = ClaudeAdapter::with_self_cmd(vec!["tandem".to_string()]);
            &adapter_claude
        } else {
            adapter_agy = AgyAdapter::new();
            &adapter_agy
        };

        let result = adapter.build_argv(&spec, resume, log_file);
        if case["error"].is_null() {
            let expected_argv: Vec<String> = serde_json::from_value(case["argv"].clone()).unwrap();
            assert_eq!(
                result.unwrap(),
                expected_argv,
                "Mismatch for case: {:?}",
                case
            );
        } else {
            assert!(result.is_err(), "Expected error for case: {:?}", case);
        }
    }
}
