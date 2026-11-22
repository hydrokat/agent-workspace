use std::fs;
use std::path::{Path, PathBuf};

use tandem::factory::approval::{ApprovalError, Caller};
use tandem::factory::auth_risk::{
    AuthRisk, acknowledge_with_terms, assess, current_acks, method_from_login,
};
use tandem::factory::broker::terms::{AuthType, PROVIDER_TERMS, ProviderTermsRow, TermsVerdict};
use tandem::factory::events::read_all;
use tandem::factory::store::FactoryStore;

const TODAY: &str = "2026-11-22";

fn temp_ws(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tandem-test-auth-risk-{}-{}-{}",
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

fn store(name: &str) -> (FactoryStore, PathBuf) {
    let ws = temp_ws(name);
    let store = FactoryStore::new(&ws);
    store.ensure().unwrap();
    (store, ws)
}

fn no_env(_: &str) -> Option<String> {
    None
}

fn agent_env(k: &str) -> Option<String> {
    (k == "TANDEM_ROLE").then(|| "orchestrator".to_string())
}

fn user() -> Caller<'static> {
    Caller::cli(true, &no_env, &no_env)
}

fn events_of(ws: &Path) -> Vec<tandem::factory::events::Event> {
    read_all(&ws.join(".agent-workspace/factory/events.jsonl"))
        .map(|r| r.events)
        .unwrap_or_default()
}

fn terms_with_verified_on(
    verified_on: &'static str,
    reverify_by: &'static str,
) -> Vec<ProviderTermsRow> {
    PROVIDER_TERMS
        .iter()
        .map(|r| ProviderTermsRow {
            verified_on,
            reverify_by,
            ..*r
        })
        .collect()
}

#[test]
fn method_from_login_maps_cli_reported_methods() {
    assert_eq!(
        method_from_login("claude", Some("subscription")),
        Some(AuthType::Subscription)
    );
    assert_eq!(
        method_from_login("claude", Some("claude.ai")),
        Some(AuthType::Subscription)
    );
    assert_eq!(
        method_from_login("claude", Some("api_key")),
        Some(AuthType::ApiKey)
    );
    assert_eq!(
        method_from_login("claude", Some("api-key")),
        Some(AuthType::ApiKey)
    );
    assert_eq!(
        method_from_login("claude", Some("bedrock")),
        Some(AuthType::Cloud)
    );
    assert_eq!(
        method_from_login("claude", Some("vertex")),
        Some(AuthType::Cloud)
    );
    assert_eq!(
        method_from_login("opencode", Some("api-key")),
        Some(AuthType::ApiKey)
    );
    assert_eq!(method_from_login("agy", None), None);
    assert_eq!(method_from_login("claude", Some("mystery")), None);
}

#[test]
fn not_allowed_and_unclear_methods_are_risky() {
    let claude_sub = assess(
        "claude",
        Some(AuthType::Subscription),
        TODAY,
        PROVIDER_TERMS,
        &[],
    );
    match claude_sub {
        AuthRisk::Risky {
            verdict,
            reason,
            conditions,
        } => {
            assert_eq!(verdict, Some(TermsVerdict::NotAllowed));
            assert!(reason.contains("not-allowed"), "{reason}");
            assert!(conditions.unwrap().contains("Consumer Terms"));
        }
        other => panic!("expected Risky, got {other:?}"),
    }

    let agy_sub = assess(
        "agy",
        Some(AuthType::Subscription),
        TODAY,
        PROVIDER_TERMS,
        &[],
    );
    assert!(matches!(
        agy_sub,
        AuthRisk::Risky {
            verdict: Some(TermsVerdict::Unclear),
            ..
        }
    ));
}

#[test]
fn allowed_methods_are_ok() {
    for (cli, method) in [
        ("claude", AuthType::ApiKey),
        ("claude", AuthType::Cloud),
        ("agy", AuthType::ApiKey),
        ("agy", AuthType::Cloud),
    ] {
        let risk = assess(cli, Some(method), TODAY, PROVIDER_TERMS, &[]);
        assert!(
            matches!(risk, AuthRisk::Ok { .. }),
            "{cli} {method:?}: {risk:?}"
        );
    }
}

#[test]
fn unknown_method_and_missing_terms_row_are_risky_never_ok() {
    let unknown = assess("agy", None, TODAY, PROVIDER_TERMS, &[]);
    match unknown {
        AuthRisk::Risky {
            verdict, reason, ..
        } => {
            assert_eq!(verdict, None);
            assert!(reason.contains("could not be detected"), "{reason}");
        }
        other => panic!("expected Risky, got {other:?}"),
    }

    let no_row = assess(
        "opencode",
        Some(AuthType::ApiKey),
        TODAY,
        PROVIDER_TERMS,
        &[],
    );
    match no_row {
        AuthRisk::Risky {
            verdict, reason, ..
        } => {
            assert_eq!(verdict, None);
            assert!(reason.contains("no verified terms"), "{reason}");
        }
        other => panic!("expected Risky, got {other:?}"),
    }
}

#[test]
fn expired_terms_make_an_allowed_method_risky() {
    let risk = assess(
        "claude",
        Some(AuthType::ApiKey),
        "2027-02-15",
        PROVIDER_TERMS,
        &[],
    );
    match risk {
        AuthRisk::Risky { reason, .. } => assert!(reason.contains("expired"), "{reason}"),
        other => panic!("expected Risky, got {other:?}"),
    }
}

#[test]
fn acknowledging_records_one_event_and_makes_the_method_acknowledged() {
    let (store, ws) = store("ack");
    let ack = acknowledge_with_terms(
        &store,
        "claude",
        Some(AuthType::Subscription),
        "subscription",
        "claude",
        "alice",
        &user(),
        PROVIDER_TERMS,
    )
    .unwrap();
    assert_eq!(ack.cli, "claude");
    assert_eq!(ack.method, "subscription");
    assert_eq!(ack.verdict, "not-allowed");

    let evs = events_of(&ws);
    let acks: Vec<_> = evs
        .iter()
        .filter(|e| e.r#type == "auth.risk_acknowledged")
        .collect();
    assert_eq!(acks.len(), 1);

    let current = current_acks(&evs);
    let risk = assess(
        "claude",
        Some(AuthType::Subscription),
        TODAY,
        PROVIDER_TERMS,
        &current,
    );
    assert!(
        matches!(
            risk,
            AuthRisk::Acknowledged {
                verdict: Some(TermsVerdict::NotAllowed),
                ..
            }
        ),
        "{risk:?}"
    );

    let other_cli = assess(
        "agy",
        Some(AuthType::Subscription),
        TODAY,
        PROVIDER_TERMS,
        &current,
    );
    assert!(matches!(other_cli, AuthRisk::Risky { .. }));
}

#[test]
fn acknowledging_is_user_only() {
    let (store, ws) = store("user-only");
    let agent = Caller::cli(true, &agent_env, &no_env);
    let err = acknowledge_with_terms(
        &store,
        "claude",
        Some(AuthType::Subscription),
        "subscription",
        "claude",
        "alice",
        &agent,
        PROVIDER_TERMS,
    )
    .unwrap_err();
    assert!(matches!(err, ApprovalError::AgentCaller));
    assert_eq!(err.exit_code(), 3);

    let no_tty = Caller::cli(false, &no_env, &no_env);
    let err = acknowledge_with_terms(
        &store,
        "claude",
        Some(AuthType::Subscription),
        "subscription",
        "claude",
        "alice",
        &no_tty,
        PROVIDER_TERMS,
    )
    .unwrap_err();
    assert!(matches!(err, ApprovalError::TerminalRequired));
    assert!(
        events_of(&ws)
            .iter()
            .all(|e| e.r#type != "auth.risk_acknowledged")
    );
}

#[test]
fn confirmation_must_match_and_method_must_be_the_detected_one() {
    let (store, ws) = store("mismatch");
    let err = acknowledge_with_terms(
        &store,
        "claude",
        Some(AuthType::Subscription),
        "subscription",
        "agy",
        "alice",
        &user(),
        PROVIDER_TERMS,
    )
    .unwrap_err();
    assert!(matches!(err, ApprovalError::InvalidInput(_)));
    assert!(err.to_string().contains("--confirm claude"), "{err}");

    let err = acknowledge_with_terms(
        &store,
        "claude",
        Some(AuthType::ApiKey),
        "subscription",
        "claude",
        "alice",
        &user(),
        PROVIDER_TERMS,
    )
    .unwrap_err();
    assert!(matches!(err, ApprovalError::InvalidInput(_)));
    assert!(err.to_string().contains("api-key"), "{err}");

    let err = acknowledge_with_terms(
        &store,
        "not-a-cli",
        None,
        "unknown",
        "not-a-cli",
        "alice",
        &user(),
        PROVIDER_TERMS,
    )
    .unwrap_err();
    assert!(matches!(err, ApprovalError::InvalidInput(_)));
    assert!(
        events_of(&ws)
            .iter()
            .all(|e| e.r#type != "auth.risk_acknowledged")
    );
}

#[test]
fn an_unknown_method_can_be_acknowledged_when_it_is_still_unknown() {
    let (store, ws) = store("unknown-ack");
    acknowledge_with_terms(
        &store,
        "agy",
        None,
        "unknown",
        "agy",
        "alice",
        &user(),
        PROVIDER_TERMS,
    )
    .unwrap();
    let current = current_acks(&events_of(&ws));
    let risk = assess("agy", None, TODAY, PROVIDER_TERMS, &current);
    assert!(
        matches!(risk, AuthRisk::Acknowledged { verdict: None, .. }),
        "{risk:?}"
    );
}

#[test]
fn an_acknowledgement_lapses_when_terms_are_reverified_expire_or_the_method_changes() {
    let (store, ws) = store("lapse");
    acknowledge_with_terms(
        &store,
        "claude",
        Some(AuthType::Subscription),
        "subscription",
        "claude",
        "alice",
        &user(),
        PROVIDER_TERMS,
    )
    .unwrap();
    let acks = current_acks(&events_of(&ws));

    let reverified = terms_with_verified_on("2027-03-01", "2027-05-30");
    let risk = assess(
        "claude",
        Some(AuthType::Subscription),
        "2027-03-02",
        &reverified,
        &acks,
    );
    assert!(
        matches!(risk, AuthRisk::Risky { .. }),
        "re-verified terms: {risk:?}"
    );

    let expired = assess(
        "claude",
        Some(AuthType::Subscription),
        "2027-02-15",
        PROVIDER_TERMS,
        &acks,
    );
    assert!(
        matches!(expired, AuthRisk::Risky { .. }),
        "expired: {expired:?}"
    );

    let changed = assess(
        "claude",
        Some(AuthType::ApiKey),
        TODAY,
        PROVIDER_TERMS,
        &acks,
    );
    assert!(
        matches!(changed, AuthRisk::Ok { .. }),
        "api key is ok, not acknowledged: {changed:?}"
    );

    let back_to_other_risky = assess(
        "agy",
        Some(AuthType::Subscription),
        TODAY,
        PROVIDER_TERMS,
        &acks,
    );
    assert!(matches!(back_to_other_risky, AuthRisk::Risky { .. }));
}

#[test]
fn a_second_acknowledgement_for_the_same_cli_and_method_is_idempotent() {
    let (store, ws) = store("idempotent");
    for _ in 0..2 {
        acknowledge_with_terms(
            &store,
            "claude",
            Some(AuthType::Subscription),
            "subscription",
            "claude",
            "alice",
            &user(),
            PROVIDER_TERMS,
        )
        .unwrap();
    }
    let current = current_acks(&events_of(&ws));
    assert_eq!(current.iter().filter(|a| a.cli == "claude").count(), 1);
}

mod cli {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use tandem::config::Config;
    use tandem::factory::approval::Caller;
    use tandem::factory::commands::run_with_caller_and_probe;
    use tandem::factory::login::LoginProbe;

    use super::{agent_env, no_env, temp_ws};

    struct Probe(HashMap<String, (i32, String, String)>);

    impl Probe {
        fn claude_subscription() -> Self {
            let mut m = HashMap::new();
            m.insert(
                "claude".to_string(),
                (
                    0,
                    r#"{"loggedIn":true,"authMethod":"claude.ai","subscriptionType":"max"}"#
                        .to_string(),
                    String::new(),
                ),
            );
            m.insert(
                "agy".to_string(),
                (0, "gemini-4".to_string(), String::new()),
            );
            m.insert(
                "opencode".to_string(),
                (1, String::new(), "none".to_string()),
            );
            Self(m)
        }
    }

    impl LoginProbe for Probe {
        fn which(&self, bin: &str) -> Option<PathBuf> {
            self.0
                .contains_key(bin)
                .then(|| PathBuf::from(format!("/usr/bin/{bin}")))
        }
        fn version(&self, _cli: &str) -> String {
            "1.0.0".to_string()
        }
        fn run(&self, argv: &[&str]) -> (i32, String, String) {
            self.0
                .get(argv[0])
                .cloned()
                .unwrap_or((1, String::new(), String::new()))
        }
    }

    fn run(args: &[&str], ws: &Path, caller: &Caller<'_>) -> (i32, String, String) {
        let mut full = vec!["tandem"];
        full.extend_from_slice(args);
        let cli = tandem::cli::parse_from(full).unwrap();
        let tandem::cli::Command::Factory(f) = cli.command.unwrap() else {
            panic!("expected factory command");
        };
        let cfg = Config {
            factory: true,
            ..Config::default()
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run_with_caller_and_probe(
            &f,
            ws,
            &cfg,
            caller,
            &Probe::claude_subscription(),
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn login_risk_json_flags_unacknowledged_methods_and_prints_the_command() {
        let ws = temp_ws("cli-risk");
        let user = Caller::cli(true, &no_env, &no_env);
        let (code, out, _) = run(&["factory", "login", "risk", "--json"], &ws, &user);
        assert_eq!(code, 0, "{out}");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let claude = v
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["cli"] == "claude")
            .unwrap();
        assert_eq!(claude["method"], "subscription");
        assert_eq!(claude["verdict"], "not-allowed");
        assert_eq!(claude["risk"], "risky");
        assert_eq!(
            claude["acknowledge_command"],
            "tandem factory login acknowledge-risk claude --method subscription --confirm claude"
        );
        let agy = v
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["cli"] == "agy")
            .unwrap();
        assert_eq!(agy["method"], "unknown");
        assert_eq!(agy["risk"], "risky");
        let opencode = v
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["cli"] == "opencode")
            .unwrap();
        assert!(
            opencode.get("risk").is_none(),
            "not signed in has no risk: {opencode}"
        );
    }

    #[test]
    fn login_status_json_keeps_existing_fields_and_adds_risk() {
        let ws = temp_ws("cli-status");
        let user = Caller::cli(true, &no_env, &no_env);
        let (code, out, _) = run(&["factory", "login", "status", "--json"], &ws, &user);
        assert_eq!(code, 0, "{out}");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let claude = v["clis"]
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["cli"] == "claude")
            .unwrap();
        for key in [
            "cli",
            "installed",
            "version",
            "signed_in",
            "method",
            "ready",
            "has_login_command",
        ] {
            assert!(claude.get(key).is_some(), "missing {key}: {claude}");
        }
        assert_eq!(claude["risk"], "risky");
        assert_eq!(claude["verdict"], "not-allowed");
    }

    #[test]
    fn acknowledge_risk_cli_is_user_only_and_then_login_risk_reports_acknowledged() {
        let ws = temp_ws("cli-ack");
        let agent = Caller::cli(true, &agent_env, &no_env);
        let (code, _, err) = run(
            &[
                "factory",
                "login",
                "acknowledge-risk",
                "claude",
                "--method",
                "subscription",
                "--confirm",
                "claude",
            ],
            &ws,
            &agent,
        );
        assert_eq!(code, 3, "{err}");

        let user = Caller::cli(true, &no_env, &no_env);
        let (code, _, err) = run(
            &[
                "factory",
                "login",
                "acknowledge-risk",
                "claude",
                "--method",
                "subscription",
                "--confirm",
                "nope",
            ],
            &ws,
            &user,
        );
        assert_eq!(code, 1, "{err}");

        let (code, out, err) = run(
            &[
                "factory",
                "login",
                "acknowledge-risk",
                "claude",
                "--method",
                "api-key",
                "--confirm",
                "claude",
            ],
            &ws,
            &user,
        );
        assert_eq!(code, 1, "{out}{err}");

        let (code, out, err) = run(
            &[
                "factory",
                "login",
                "acknowledge-risk",
                "claude",
                "--method",
                "subscription",
                "--confirm",
                "claude",
                "--json",
            ],
            &ws,
            &user,
        );
        assert_eq!(code, 0, "{out}{err}");

        let (_, out, _) = run(&["factory", "login", "risk", "--json"], &ws, &user);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let claude = v
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["cli"] == "claude")
            .unwrap();
        assert_eq!(claude["risk"], "acknowledged");
        assert!(claude.get("acknowledge_command").is_none());
    }

    #[test]
    fn acknowledge_risk_requires_a_signed_in_cli() {
        let ws = temp_ws("cli-not-signed-in");
        let user = Caller::cli(true, &no_env, &no_env);
        let (code, _, err) = run(
            &[
                "factory",
                "login",
                "acknowledge-risk",
                "opencode",
                "--method",
                "api-key",
                "--confirm",
                "opencode",
            ],
            &ws,
            &user,
        );
        assert_eq!(code, 1, "{err}");
    }
}
