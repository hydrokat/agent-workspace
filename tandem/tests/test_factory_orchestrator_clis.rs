mod common;

use std::process::Command;

use common::TempDir;
use tandem::config::Config;
use tandem::factory::commands::start_idea;
use tandem::factory::model::FactorySettings;
use tandem::factory::routing::ProviderModelTiers;
use tandem::factory::settings::validate;
use tandem::status::factory_tab::ORCHESTRATOR_CLIS;

fn tiers(model: &str) -> ProviderModelTiers {
    ProviderModelTiers {
        economy: model.to_string(),
        standard: model.to_string(),
        premium: model.to_string(),
    }
}

#[test]
fn test_orchestrator_clis_include_opencode() {
    assert_eq!(ORCHESTRATOR_CLIS, ["claude", "agy", "opencode"]);
}

#[test]
fn test_start_idea_accepts_opencode_cli_and_provider_slash_models() {
    let ws = TempDir::new();
    let err = start_idea(
        ws.path(),
        &Config::default(),
        "opencode",
        "opencode/big-pickle#high",
        None,
    )
    .unwrap_err();
    assert!(
        !err.contains("unsupported orchestrator cli") && !err.contains("invalid model"),
        "opencode passes validation and only fails later at pane launch: {err}"
    );
}

#[test]
fn test_start_idea_rejects_unknown_cli_listing_all_three() {
    let ws = TempDir::new();
    let err = start_idea(ws.path(), &Config::default(), "bogus", "opus", None).unwrap_err();
    assert!(err.contains("claude, agy, or opencode"), "{err}");
}

#[test]
fn test_start_idea_keeps_strict_model_ids_for_claude_and_agy() {
    let ws = TempDir::new();
    for cli in ["claude", "agy"] {
        let err = start_idea(
            ws.path(),
            &Config::default(),
            cli,
            "opencode/big-pickle",
            None,
        )
        .unwrap_err();
        assert!(err.to_lowercase().contains("model"), "{cli}: {err}");
    }
    let err = start_idea(
        ws.path(),
        &Config::default(),
        "opencode",
        "not a model",
        None,
    )
    .unwrap_err();
    assert!(err.to_lowercase().contains("model"), "{err}");
}

#[test]
fn test_settings_validation_uses_provider_aware_model_rules() {
    let mut settings = FactorySettings::default();
    settings
        .model_tiers
        .insert("opencode".to_string(), tiers("opencode/big-pickle#high"));
    settings.allowed_models.insert(
        "opencode".to_string(),
        vec!["openrouter/claude-sonnet-latest".to_string()],
    );
    settings.price_table.insert(
        "opencode/big-pickle".to_string(),
        tandem::factory::model::ModelPrice {
            input: 1.0,
            output: 2.0,
            as_of: "2026-01-01".to_string(),
        },
    );
    validate(&settings).expect("opencode ids validate");

    let mut bad = FactorySettings::default();
    bad.model_tiers
        .insert("claude".to_string(), tiers("opencode/big-pickle"));
    assert!(validate(&bad).is_err(), "slash ids stay invalid for claude");
}

#[test]
fn test_factory_models_command_lists_models_as_json() {
    let ws = TempDir::new();
    let out = Command::new(env!("CARGO_BIN_EXE_tandem"))
        .args(["factory", "models", "--cli", "claude", "--json"])
        .env("TANDEM_WORKSPACE", ws.path())
        .env("XDG_STATE_HOME", ws.path().join("state"))
        .env_remove("TANDEM_FACTORY")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = v.as_array().expect("array of models");
    assert!(arr.iter().any(|m| m["id"] == "opus"));
    assert!(arr.iter().all(|m| m["label"].is_string()));

    let bad = Command::new(env!("CARGO_BIN_EXE_tandem"))
        .args(["factory", "models", "--cli", "nope", "--json"])
        .env("TANDEM_WORKSPACE", ws.path())
        .env("XDG_STATE_HOME", ws.path().join("state"))
        .env_remove("TANDEM_FACTORY")
        .output()
        .unwrap();
    assert!(!bad.status.success());
}
