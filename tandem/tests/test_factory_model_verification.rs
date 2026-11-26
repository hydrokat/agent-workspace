mod common;

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use tandem::adapters::base::{AdapterError, Model};
use tandem::cli::{Cli, Command as CliCommand};
use tandem::config::Config;
use tandem::factory::approval::{ApprovalError, Caller};
use tandem::factory::commands::run_models_check_with_runner;
use tandem::factory::commands::run_with_caller_io;
use tandem::factory::events::read_all;
use tandem::factory::model::{FactorySettings, ModelDecisionAction};
use tandem::factory::model_catalog::{
    ModelFreshnessStatus, ModelSource, discover, verify_configured_models, with_pinned,
};
use tandem::factory::routing::ModelTier;
use tandem::factory::settings::{
    apply_model_decision, load as load_settings, save as save_settings,
};
use tandem::factory::store::FactoryStore;

struct TempWs {
    root: PathBuf,
    state_dir: PathBuf,
}

impl TempWs {
    fn new(tag: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "tandem-model-ver-{tag}-{}-{nanos}",
            std::process::id()
        ));
        let root = base.join("ws");
        let state_dir = base.join("state");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&state_dir).unwrap();
        Self { root, state_dir }
    }

    fn run_cli(
        &self,
        args: &[&str],
        stdin_bytes: &[u8],
        extra_env: &[(&str, &str)],
        tty: bool,
    ) -> (i32, Vec<u8>, Vec<u8>) {
        let mut argv = vec!["tandem"];
        argv.extend_from_slice(args);
        let cli = Cli::try_parse_from(argv).expect("parse cli");
        let Some(CliCommand::Factory(f_args)) = cli.command else {
            panic!("expected factory command");
        };
        let cfg = Config {
            factory: true,
            state_dir: self.state_dir.clone(),
            ..Default::default()
        };
        let env_fn = |k: &str| {
            extra_env
                .iter()
                .rev()
                .find(|(ek, _)| *ek == k)
                .map(|(_, ev)| (*ev).to_string())
        };
        let no_pane = |_: &str| None;
        let caller = Caller::cli(tty, &env_fn, &no_pane);
        let mut stdin = std::io::Cursor::new(stdin_bytes);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run_with_caller_io(
            &f_args,
            &self.root,
            &cfg,
            &caller,
            &mut stdin,
            &mut stdout,
            &mut stderr,
        );
        (code, stdout, stderr)
    }
}

impl Drop for TempWs {
    fn drop(&mut self) {
        if let Some(parent) = self.root.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

fn model(id: &str) -> Model {
    Model {
        id: id.to_string(),
        label: id.to_string(),
    }
}

#[test]
fn verify_configured_models_detects_missing_newer_and_skips_offline_fallback() {
    let ws = TempWs::new("verify");
    let store = FactoryStore::new(&ws.root);
    store.ensure().unwrap();
    let settings = FactorySettings::default();

    let runner = |cli: &str| -> Result<Vec<Model>, AdapterError> {
        match cli {
            "agy" => Ok(vec![
                model("gemini-2.5-flash-lite"),
                model("gemini-3-flash"),
                model("gemini-4-neon-high"),
            ]),
            "claude" => Ok(vec![
                model("claude-haiku-4-5"),
                model("claude-sonnet-4-6"),
                model("claude-sonnet-5-5"),
                model("claude-opus-4-7"),
            ]),
            "opencode" => Err(AdapterError::ModelList("offline".to_string())),
            other => Err(AdapterError::UnknownCli(other.to_string())),
        }
    };

    let report = verify_configured_models(
        &["agy", "claude", "opencode"],
        &settings,
        &store.root,
        true,
        &runner,
    );

    let agy_standard = report
        .items
        .iter()
        .find(|i| i.cli == "agy" && i.tier == ModelTier::Standard)
        .unwrap();
    assert_eq!(
        agy_standard.status,
        ModelFreshnessStatus::MissingFromCatalog
    );
    assert!(agy_standard.needs_attention);
    assert_eq!(agy_standard.recommended_model(), Some("gemini-3-flash"));

    let claude_standard = report
        .items
        .iter()
        .find(|i| i.cli == "claude" && i.tier == ModelTier::Standard)
        .unwrap();
    assert_eq!(
        claude_standard.status,
        ModelFreshnessStatus::NewerAvailable {
            recommended: "claude-sonnet-5-5".to_string(),
        }
    );
    assert!(claude_standard.needs_attention);

    let claude_econ = report
        .items
        .iter()
        .find(|i| i.cli == "claude" && i.tier == ModelTier::Economy)
        .unwrap();
    assert_eq!(claude_econ.status, ModelFreshnessStatus::Current);
    assert!(!claude_econ.needs_attention);

    let opencode_items: Vec<_> = report
        .items
        .iter()
        .filter(|i| i.cli == "opencode")
        .collect();
    assert!(
        opencode_items
            .iter()
            .all(|i| i.source == ModelSource::Fallback && !i.needs_attention),
        "offline Fallback catalog must never raise false-positive drift"
    );
}

#[test]
fn claude_alias_only_catalog_covers_default_pinned_families_without_false_drift() {
    let ws = TempWs::new("aliases");
    let store = FactoryStore::new(&ws.root);
    store.ensure().unwrap();
    let settings = FactorySettings::default();
    let alias_runner = |cli: &str| -> Result<Vec<Model>, AdapterError> {
        if cli == "claude" {
            Ok(vec![model("opus"), model("sonnet"), model("fable")])
        } else {
            Err(AdapterError::ModelList("offline".to_string()))
        }
    };

    let report = verify_configured_models(&["claude"], &settings, &store.root, true, &alias_runner);
    assert_eq!(
        report.needs_attention_count(),
        0,
        "static CLAUDE_ALIASES must satisfy default claude family models: {:?}",
        report.items
    );
}

#[test]
fn with_pinned_keeps_live_models_before_fallback_defaults_at_equal_cost() {
    let ws = TempWs::new("pinned-order");
    let dir = ws.root.join("cache");
    fs::create_dir_all(&dir).unwrap();
    let settings = FactorySettings::default();
    let runner = |_: &str| -> Result<Vec<Model>, AdapterError> {
        Ok(vec![model("gemini-3-flash"), model("gemini-4-neon-high")])
    };
    let live = discover("agy", &settings, &dir, true, &runner);
    let merged = with_pinned("agy", live, &settings);
    let standard_pos_live = merged
        .iter()
        .position(|m| m.id == "gemini-3-flash")
        .unwrap();
    let standard_pos_fallback = merged
        .iter()
        .position(|m| m.id == "gemini-2.0-flash")
        .unwrap();
    assert!(
        standard_pos_live < standard_pos_fallback,
        "live models must precede unverified fallback defaults at equal rank"
    );
}

#[test]
fn apply_model_decision_persists_and_suppresses_reprompt_until_catalog_changes() {
    let ws = TempWs::new("decision");
    let store = FactoryStore::new(&ws.root);
    store.ensure().unwrap();

    // Verify legacy settings.json without model_decisions still deserializes cleanly.
    fs::write(store.root.join("settings.json"), r#"{"version":1}"#).unwrap();
    let legacy = load_settings(&store).unwrap();
    assert!(legacy.model_decisions.is_empty());

    let runner_v1 = |cli: &str| -> Result<Vec<Model>, AdapterError> {
        if cli == "agy" {
            Ok(vec![model("gemini-3-flash"), model("gemini-4-neon-high")])
        } else {
            Err(AdapterError::ModelList("offline".to_string()))
        }
    };
    let report1 = verify_configured_models(&["agy"], &legacy, &store.root, true, &runner_v1);
    let drift = report1
        .items
        .iter()
        .find(|i| i.cli == "agy" && i.tier == ModelTier::Standard)
        .unwrap();
    assert!(drift.needs_attention);

    let env_fn = |_: &str| None;
    let no_pane = |_: &str| None;
    let user = Caller::cli(true, &env_fn, &no_pane);
    let res = apply_model_decision(
        &store,
        "agy",
        ModelTier::Standard,
        &drift.configured,
        ModelDecisionAction::Kept,
        &drift.available_signature,
        "alice",
        &user,
    )
    .unwrap();
    assert_eq!(res.settings.model_tiers["agy"].standard, "gemini-2.0-flash");
    assert_eq!(
        res.settings.model_decisions["agy"]["standard"].action,
        ModelDecisionAction::Kept
    );

    let reloaded = load_settings(&store).unwrap();
    let report2 = verify_configured_models(&["agy"], &reloaded, &store.root, true, &runner_v1);
    let std_after_keep = report2
        .items
        .iter()
        .find(|i| i.cli == "agy" && i.tier == ModelTier::Standard)
        .unwrap();
    assert!(
        !std_after_keep.needs_attention,
        "keeping model must suppress repeat prompt for unchanged catalog"
    );

    // Catalog changes -> must prompt again!
    let runner_v2 = |cli: &str| -> Result<Vec<Model>, AdapterError> {
        if cli == "agy" {
            Ok(vec![
                model("gemini-3-flash"),
                model("gemini-3.5-flash"),
                model("gemini-4-neon-high"),
            ])
        } else {
            Err(AdapterError::ModelList("offline".to_string()))
        }
    };
    let report3 = verify_configured_models(&["agy"], &reloaded, &store.root, true, &runner_v2);
    let std_after_change = report3
        .items
        .iter()
        .find(|i| i.cli == "agy" && i.tier == ModelTier::Standard)
        .unwrap();
    assert!(
        std_after_change.needs_attention,
        "catalog signature change must re-enable prompt"
    );

    // Choose updated model -> persists to both model_tiers and model_decisions + audit event
    let updated = apply_model_decision(
        &store,
        "agy",
        ModelTier::Standard,
        "gemini-3.5-flash",
        ModelDecisionAction::Updated,
        &std_after_change.available_signature,
        "alice",
        &user,
    )
    .unwrap();
    assert_eq!(
        updated.settings.model_tiers["agy"].standard,
        "gemini-3.5-flash"
    );
    let log = read_all(&store.root.join("events.jsonl")).unwrap();
    assert!(!log.events.is_empty());

    // Agent callers must be denied.
    let agent_env = |k: &str| (k == "TANDEM_SESSION_ID").then(|| "agent-1".to_string());
    let agent = Caller::cli(false, &agent_env, &no_pane);
    let denied = apply_model_decision(
        &store,
        "agy",
        ModelTier::Standard,
        "gemini-3-flash",
        ModelDecisionAction::Updated,
        &std_after_change.available_signature,
        "agent",
        &agent,
    );
    assert!(matches!(denied, Err(ApprovalError::AgentCaller)));
}

#[test]
fn cli_models_check_and_choose_handle_interactive_tty_json_and_agent_denial() {
    let ws = TempWs::new("cli");
    let store = FactoryStore::new(&ws.root);
    store.ensure().unwrap();
    save_settings(&store, &FactorySettings::default()).unwrap();

    // Non-user caller refused on `models choose` with exit code 3
    let (denied_code, _out, _err) = ws.run_cli(
        &[
            "factory",
            "models",
            "choose",
            "--cli",
            "agy",
            "--tier",
            "standard",
            "--model",
            "gemini-3-flash",
            "--json",
        ],
        b"",
        &[("TANDEM_SESSION_ID", "worker-1")],
        false,
    );
    assert_eq!(denied_code, 3);

    // User caller succeeds on `models choose` and writes settings.json
    let (ok_code, out, err) = ws.run_cli(
        &[
            "factory",
            "models",
            "choose",
            "--cli",
            "agy",
            "--tier",
            "standard",
            "--model",
            "gemini-3-flash",
            "--json",
        ],
        b"",
        &[],
        true,
    );
    assert_eq!(ok_code, 0, "stderr: {}", String::from_utf8_lossy(&err));
    let parsed: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(parsed["model_tiers"]["agy"]["standard"], "gemini-3-flash");

    // Interactive TTY `run_models_check_with_runner` lets user pick replacement and persist it
    let env_fn = |_: &str| None;
    let no_pane = |_: &str| None;
    let user = Caller::cli(true, &env_fn, &no_pane);
    let runner = |cli: &str| -> Result<Vec<Model>, AdapterError> {
        if cli == "agy" {
            Ok(vec![
                model("gemini-2.5-flash-lite"),
                model("gemini-3-flash"),
                model("gemini-4-neon-high"),
            ])
        } else {
            Err(AdapterError::ModelList("offline".to_string()))
        }
    };
    // Economy (gemini-2.0-flash-lite) -> pick option 1; Premium (gemini-2.5-pro) -> keep (k)
    let mut input = std::io::Cursor::new(b"1\nk\n".as_slice());
    let mut prompt_out = Vec::new();
    let interactive_report = run_models_check_with_runner(
        &store,
        Some("agy"),
        true,
        false,
        &user,
        &mut input,
        &mut prompt_out,
        &runner,
    )
    .unwrap();
    assert!(interactive_report.items.iter().any(|i| i.cli == "agy"));
    let final_settings = load_settings(&store).unwrap();
    assert_eq!(
        final_settings.model_tiers["agy"].economy,
        "gemini-2.5-flash-lite"
    );
    assert_eq!(final_settings.model_tiers["agy"].premium, "gemini-2.5-pro");
    assert_eq!(
        final_settings.model_decisions["agy"]["premium"].action,
        ModelDecisionAction::Kept
    );
}
