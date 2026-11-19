use std::fs;
use std::path::{Path, PathBuf};

use tandem::config::Config;
use tandem::factory::approval::Caller;
use tandem::factory::broker::gate::{
    AuthType, GateDenied, GatePass, check, consent_provider, revoke_provider,
};
use tandem::factory::broker::terms::{PROVIDER_TERMS, TermsVerdict, lookup};
use tandem::factory::commands::run_with_caller;
use tandem::factory::events::read_all;
use tandem::factory::model::{FactorySettings, ProviderGate, ProviderGateState};
use tandem::factory::store::FactoryStore;

fn temp_ws(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-gate-{}-{}-{}",
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

fn call_factory(
    args: &[&str],
    ws: &Path,
    cfg: &Config,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
) -> i32 {
    let mut full = vec!["tandem"];
    full.extend_from_slice(args);
    let cli = tandem::cli::parse_from(full).unwrap();
    let tandem::cli::Command::Factory(f_args) = cli.command.unwrap() else {
        panic!("expected factory command");
    };
    let caller = Caller::cli(true, &|_| None, &|_| None);
    run_with_caller(&f_args, ws, cfg, &caller, stdout, stderr)
}

#[test]
fn terms_table_matches_record_and_denies_unverified_or_disallowed() {
    assert!(!PROVIDER_TERMS.is_empty());

    let claude_sub = lookup("claude", AuthType::Subscription).unwrap();
    assert_eq!(claude_sub.verdict, TermsVerdict::NotAllowed);

    let claude_api = lookup("claude", AuthType::ApiKey).unwrap();
    assert_eq!(claude_api.verdict, TermsVerdict::AllowedWithConditions);
    assert_eq!(claude_api.verified_on, "2026-11-16");
    assert_eq!(claude_api.reverify_by, "2027-02-14");

    let agy_sub = lookup("agy", AuthType::Subscription).unwrap();
    assert_eq!(agy_sub.verdict, TermsVerdict::Unclear);

    let agy_api = lookup("agy", AuthType::ApiKey).unwrap();
    assert_eq!(agy_api.verdict, TermsVerdict::AllowedWithConditions);
}

#[test]
fn gate_check_enforces_all_five_conditions_fail_closed() {
    let valid_today = "2026-11-17";
    let expired_today = "2027-02-15";

    // 1. Default settings -> no consent / closed
    let default_settings = FactorySettings::default();
    let err = check("claude", &default_settings, valid_today).unwrap_err();
    assert!(matches!(
        err,
        GateDenied::NoConsent { .. } | GateDenied::GateClosed { .. }
    ));

    // 2. Disallowed auth type (claude subscription) even if gate were marked open manually
    let mut bad_auth_settings = FactorySettings {
        provider_gates: vec![ProviderGate {
            provider: "claude".into(),
            state: ProviderGateState::Open,
            auth_type: Some("subscription".into()),
            verified_on: Some("2026-11-16".into()),
            terms_record: Some(
                "knowledgebase/architecture-decisions/factory-provider-terms.md".into(),
            ),
            consented_at: Some(1000.0),
            consented_by: Some("user".into()),
        }],
        ..Default::default()
    };
    let err = check("claude", &bad_auth_settings, valid_today).unwrap_err();
    assert!(matches!(err, GateDenied::TermsDisallowed { .. }));

    // 3. Past re-verify date
    bad_auth_settings.provider_gates[0].auth_type = Some("api-key".into());
    let err = check("claude", &bad_auth_settings, expired_today).unwrap_err();
    assert!(matches!(err, GateDenied::TermsExpired { .. }));

    // 4. Stale verified_on date
    bad_auth_settings.provider_gates[0].verified_on = Some("2026-01-01".into());
    let err = check("claude", &bad_auth_settings, valid_today).unwrap_err();
    assert!(matches!(err, GateDenied::ConsentStale { .. }));

    // 5. Gate closed
    bad_auth_settings.provider_gates[0].verified_on = Some("2026-11-16".into());
    bad_auth_settings.provider_gates[0].state = ProviderGateState::Closed;
    let err = check("claude", &bad_auth_settings, valid_today).unwrap_err();
    assert!(matches!(err, GateDenied::GateClosed { .. }));

    // 6. All 5 conditions hold -> returns GatePass
    bad_auth_settings.provider_gates[0].state = ProviderGateState::Open;
    let pass: GatePass = check("claude", &bad_auth_settings, valid_today).unwrap();
    assert_eq!(pass.provider(), "claude");
    assert_eq!(pass.auth_type(), AuthType::ApiKey);
}

#[test]
fn consent_and_revoke_are_user_only_record_events_and_enforce_terms() {
    let ws = temp_ws("consent");
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();

    let no_env = |_: &str| None;
    let no_pane = |_: &str| None;
    let agent_env = |k: &str| {
        if k == "TANDEM_ROLE" {
            Some("orchestrator".to_string())
        } else {
            None
        }
    };
    let user_caller = Caller::cli(true, &no_env, &no_pane);
    let agent_caller = Caller::cli(true, &agent_env, &no_pane);

    // Agent caller rejected
    assert!(
        consent_provider(
            &store,
            "claude",
            AuthType::ApiKey,
            "claude",
            "orch",
            &agent_caller
        )
        .is_err()
    );

    // Disallowed auth type rejected
    assert!(
        consent_provider(
            &store,
            "claude",
            AuthType::Subscription,
            "claude",
            "alice",
            &user_caller
        )
        .is_err()
    );
    assert!(
        consent_provider(
            &store,
            "agy",
            AuthType::Subscription,
            "agy",
            "alice",
            &user_caller
        )
        .is_err()
    );

    // Confirm mismatch rejected
    assert!(
        consent_provider(
            &store,
            "claude",
            AuthType::ApiKey,
            "wrong",
            "alice",
            &user_caller
        )
        .is_err()
    );

    // Valid user consent opens gate and writes event
    let gate = consent_provider(
        &store,
        "claude",
        AuthType::ApiKey,
        "claude",
        "alice",
        &user_caller,
    )
    .unwrap();
    assert_eq!(gate.state, ProviderGateState::Open);
    let settings = store.settings().unwrap();
    assert!(check("claude", &settings, "2026-11-17").is_ok());

    // Revoke closes gate and writes event
    let revoked = revoke_provider(&store, "claude", "alice", &user_caller).unwrap();
    assert_eq!(revoked.state, ProviderGateState::Closed);
    let settings2 = store.settings().unwrap();
    assert!(check("claude", &settings2, "2026-11-17").is_err());

    let log = read_all(&ws.join(".agent-workspace/factory/events.jsonl")).unwrap();
    assert!(log.events.iter().any(|e| e.r#type == "provider.consented"));
    assert!(log.events.iter().any(|e| e.r#type == "provider.revoked"));

    // CLI requires Factory Mode
    let cfg_off = Config {
        factory: false,
        state_dir: ws.join("state"),
        ..Default::default()
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let code = call_factory(
        &["factory", "provider", "status", "--json"],
        &ws,
        &cfg_off,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, 2);

    let cfg_on = Config {
        factory: true,
        state_dir: ws.join("state"),
        ..Default::default()
    };
    stdout.clear();
    let code = call_factory(
        &[
            "factory",
            "provider",
            "consent",
            "claude",
            "--auth",
            "api-key",
            "--confirm",
            "claude",
            "--json",
        ],
        &ws,
        &cfg_on,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));

    stdout.clear();
    let code = call_factory(
        &["factory", "provider", "status", "--json"],
        &ws,
        &cfg_on,
        &mut stdout,
        &mut stderr,
    );
    assert_eq!(code, 0);
    let status_json: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert!(
        status_json
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["provider"] == "claude" && p["gate"] == "open")
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn no_env_var_can_bypass_provider_gate() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let broker_dir = manifest_dir.join("src").join("factory").join("broker");
    for entry in fs::read_dir(&broker_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let content = fs::read_to_string(&path).unwrap();
            assert!(
                !content.contains("std::env::var") && !content.contains("env!"),
                "broker module {} must not read environment variables to open or bypass gates",
                path.display()
            );
        }
    }
}
