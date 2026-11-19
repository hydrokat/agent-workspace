use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tandem::factory::approval::ApprovalError;
use tandem::factory::events::read_all;
use tandem::factory::machine::EventData;
use tandem::factory::model::{Budget, FactorySettings, ModelPrice, Stage, StageState};
use tandem::factory::routing::{ProviderModelTiers, RoutingContext, TemperatureSupport, route};
use tandem::factory::settings::{
    SETTINGS_VERSION, effective, load as load_settings, save as save_settings, set as set_setting,
    validate as validate_settings,
};
use tandem::factory::store::FactoryStore;
use tandem::factory::workflow::parse::parse_workflow_str;
use tandem::factory::workflow::scheduler::SchedulerLimits;
use tandem::factory::workflow::schema::Policy;
use tandem::factory::workflow::validate::validate as validate_workflow;

struct TestWorkspace {
    root: PathBuf,
    state_dir: PathBuf,
}

impl TestWorkspace {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "tandem-factory-settings-{name}-{}-{nanos}",
            std::process::id()
        ));
        let root = base.join("ws");
        let state_dir = base.join("state");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&state_dir).unwrap();
        Self { root, state_dir }
    }

    fn run_cli(&self, args: &[&str], extra_env: &[(&str, &str)]) -> (i32, Vec<u8>, Vec<u8>) {
        use clap::Parser;
        use tandem::cli::{Cli, Command as CliCommand};
        use tandem::config::Config;
        use tandem::factory::approval::Caller;
        use tandem::factory::commands::run_with_caller;

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
        let caller = Caller::cli(true, &env_fn, &no_pane);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run_with_caller(&f_args, &self.root, &cfg, &caller, &mut stdout, &mut stderr);
        (code, stdout, stderr)
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        if let Some(parent) = self.root.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

fn no_env(_: &str) -> Option<String> {
    None
}

fn no_pane(_: &str) -> Option<String> {
    None
}

#[test]
fn missing_file_returns_defaults_and_older_phase_loads_cleanly() {
    let ws = TestWorkspace::new("compat");
    let store = FactoryStore::new(&ws.root);

    let defaults = load_settings(&store).unwrap();
    assert_eq!(defaults, FactorySettings::default());
    assert_eq!(defaults.version, SETTINGS_VERSION);
    assert_eq!(defaults.max_parallel_agents, 3);
    assert_eq!(defaults.default_temperature, 0.4);
    assert_eq!(defaults.default_workflow, "default-delivery");
    assert_eq!(defaults.max_review_rounds, 2);
    assert_eq!(defaults.approval_grace_secs, 30);
    assert!(defaults.price_table.is_empty());

    store.ensure().unwrap();
    let legacy_json = json!({
        "max_parallel_agents": 4,
        "default_temperature": 0.5,
        "max_review_rounds": 3,
        "provider_gates": [],
        "default_budget": {
            "max_cost_usd": 12.5,
            "max_duration_secs": 900,
            "max_retries": 2
        }
    });
    fs::write(
        ws.root.join(".agent-workspace/factory/settings.json"),
        serde_json::to_vec_pretty(&legacy_json).unwrap(),
    )
    .unwrap();

    let loaded = load_settings(&store).unwrap();
    assert_eq!(loaded.version, SETTINGS_VERSION);
    assert_eq!(loaded.max_parallel_agents, 4);
    assert_eq!(loaded.default_temperature, 0.5);
    assert_eq!(loaded.max_review_rounds, 3);
    assert_eq!(loaded.default_workflow, "default-delivery");
    assert_eq!(loaded.approval_grace_secs, 30);
    assert_eq!(loaded.default_budget.max_cost_usd, Some(12.5));
}

#[test]
fn save_writes_version_mode_0600_atomically_and_preserves_unknown_keys() {
    let ws = TestWorkspace::new("persist");
    let store = FactoryStore::new(&ws.root);
    store.ensure().unwrap();

    let settings_path = ws.root.join(".agent-workspace/factory/settings.json");
    let with_unknown = json!({
        "version": 1,
        "max_parallel_agents": 4,
        "future_org_policy": { "strict": true },
        "custom_tag": "keep-me"
    });
    fs::write(
        &settings_path,
        serde_json::to_vec_pretty(&with_unknown).unwrap(),
    )
    .unwrap();

    let mut loaded = load_settings(&store).unwrap();
    assert_eq!(loaded.extra.get("custom_tag"), Some(&json!("keep-me")));
    loaded.max_review_rounds = 4;
    save_settings(&store, &loaded).unwrap();

    #[cfg(unix)]
    {
        let mode = fs::metadata(&settings_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    let raw: Value = serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
    assert_eq!(raw["version"], json!(SETTINGS_VERSION));
    assert_eq!(raw["max_review_rounds"], json!(4));
    assert_eq!(raw["custom_tag"], json!("keep-me"));
    assert_eq!(raw["future_org_policy"]["strict"], json!(true));
    assert!(!settings_path.with_extension("json.tmp").exists());
}

#[test]
fn validation_checks_all_fields_and_reports_allowed_ranges() {
    let base = FactorySettings::default();
    assert!(validate_settings(&base).is_ok());

    for bad_parallel in [0, 9] {
        let mut s = base.clone();
        s.max_parallel_agents = bad_parallel;
        let err = validate_settings(&s).unwrap_err().to_string();
        assert!(err.contains("1") && err.contains("8"), "got: {err}");
    }

    for bad_temp in [-0.1, 1.1, f64::NAN] {
        let mut s = base.clone();
        s.default_temperature = bad_temp;
        let err = validate_settings(&s).unwrap_err().to_string();
        assert!(err.contains("0.0") && err.contains("1.0"), "got: {err}");
    }

    let mut bad_wf = base.clone();
    bad_wf.default_workflow = "../escape".to_string();
    assert!(validate_settings(&bad_wf).is_err());

    let mut bad_rounds = base.clone();
    bad_rounds.max_review_rounds = 6;
    let err = validate_settings(&bad_rounds).unwrap_err().to_string();
    assert!(err.contains("0") && err.contains("5"), "got: {err}");

    let mut bad_budget = base.clone();
    bad_budget.default_budget = Budget {
        max_cost_usd: Some(-1.0),
        max_duration_secs: None,
        max_retries: None,
    };
    assert!(validate_settings(&bad_budget).is_err());

    let mut bad_tiers = base.clone();
    bad_tiers.model_tiers.insert(
        "claude".to_string(),
        ProviderModelTiers {
            economy: "".to_string(),
            standard: "claude-sonnet-4-6".to_string(),
            premium: "claude-opus-4-7".to_string(),
        },
    );
    assert!(validate_settings(&bad_tiers).is_err());

    let mut bad_price = base.clone();
    bad_price.price_table.insert(
        "claude-sonnet-4-6".to_string(),
        ModelPrice {
            input: -0.5,
            output: 1.0,
            as_of: "2026-11-18".to_string(),
        },
    );
    assert!(validate_settings(&bad_price).is_err());

    let mut bad_price_date = base.clone();
    bad_price_date.price_table.insert(
        "claude-sonnet-4-6".to_string(),
        ModelPrice {
            input: 0.5,
            output: 1.0,
            as_of: "  ".to_string(),
        },
    );
    assert!(validate_settings(&bad_price_date).is_err());

    let mut bad_grace = base;
    bad_grace.approval_grace_secs = 601;
    let err = validate_settings(&bad_grace).unwrap_err().to_string();
    assert!(err.contains("0") && err.contains("600"), "got: {err}");
}

#[test]
fn effective_upper_bound_rule_and_wiring() {
    let policy = Policy {
        max_parallel_agents: 4,
        default_temperature: 0.6,
        require_human_approval: vec![],
    };

    let tighter = FactorySettings {
        max_parallel_agents: 2,
        default_temperature: 0.3,
        ..Default::default()
    };
    let eff_tighter = effective(&policy, &tighter);
    assert_eq!(eff_tighter.max_parallel_agents, 2);
    assert_eq!(eff_tighter.default_temperature, 0.3);

    let looser = FactorySettings {
        max_parallel_agents: 8,
        default_temperature: 0.9,
        ..Default::default()
    };
    let eff_looser = effective(&policy, &looser);
    assert_eq!(eff_looser.max_parallel_agents, 4);
    assert_eq!(eff_looser.default_temperature, 0.6);

    let yaml = r#"
name: custom-flow
version: 1
trigger: ready-for-development
policy:
  max_parallel_agents: 3
  default_temperature: 0.7
stages:
  - name: implement
    agent: implementer
    tools: [repo_read, repo_write, test_runner, diff_review]
"#;
    let raw = parse_workflow_str(yaml).unwrap();
    let wf = validate_workflow(
        &raw,
        &FactorySettings {
            max_parallel_agents: 5,
            default_temperature: 0.5,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(wf.policy.max_parallel_agents, 3);
    assert_eq!(wf.policy.default_temperature, 0.5);

    let stages = vec![Stage {
        name: "implement".to_string(),
        agent: "implementer".to_string(),
        state: StageState::Running,
        attempts: 1,
        interrupted_attempts: 0,
        started: None,
        finished: None,
        artifacts: vec![],
        rationale: None,
        retry_hypothesis: None,
    }];
    let limits = SchedulerLimits::from_workflow_and_settings(&wf, &tighter, &stages);
    assert_eq!(limits.effective_max_parallel, 2);
    assert_eq!(limits.available_slots(), 1);

    let mut custom_tiers = BTreeMap::new();
    custom_tiers.insert(
        "claude".to_string(),
        ProviderModelTiers {
            economy: "claude-haiku-custom".to_string(),
            standard: "claude-sonnet-custom".to_string(),
            premium: "claude-opus-custom".to_string(),
        },
    );
    let route_settings = FactorySettings {
        default_temperature: 0.25,
        model_tiers: custom_tiers,
        ..Default::default()
    };
    let grant0 = tandem::factory::permissions::ToolGrant::from_tools(
        wf.stages[0].agent,
        &wf.stages[0].name,
        "workspace",
        wf.stages[0].tools.clone(),
    );
    let routed = route(
        &wf.stages[0],
        &grant0,
        &wf,
        &route_settings,
        &["claude"],
        1,
        &RoutingContext::default(),
    )
    .unwrap();
    assert_eq!(routed.model, "claude-sonnet-custom");
    assert_eq!(routed.temperature, TemperatureSupport::Unsupported);
}

#[test]
fn settings_set_is_user_only_blocks_protected_keys_and_emits_event() {
    let ws = TestWorkspace::new("set-api");
    let store = FactoryStore::new(&ws.root);

    let agent_env = |k: &str| -> Option<String> {
        if k == "TANDEM_ROLE" {
            Some("implementer".to_string())
        } else {
            None
        }
    };
    let agent_caller = tandem::factory::approval::Caller::cli(true, &agent_env, &no_pane);
    let err = set_setting(&store, "max_parallel_agents", "4", "bob", &agent_caller).unwrap_err();
    assert!(matches!(err, ApprovalError::AgentCaller));

    let user_caller = tandem::factory::approval::Caller::cli(true, &no_env, &no_pane);
    for protected in [
        "provider_gates",
        "providers",
        "checks",
        "deploy",
        "deploy_command",
    ] {
        let err = set_setting(&store, protected, "{}", "alice", &user_caller).unwrap_err();
        assert!(
            matches!(err, ApprovalError::InvalidInput(_)),
            "expected InvalidInput for {protected}, got: {err:?}"
        );
    }

    let res = set_setting(&store, "max_parallel_agents", "5", "alice", &user_caller).unwrap();
    assert_eq!(res.key, "max_parallel_agents");
    assert_eq!(res.old_value, json!(3));
    assert_eq!(res.new_value, json!(5));
    assert_eq!(res.actor, "alice");
    assert_eq!(res.settings.max_parallel_agents, 5);

    let log = read_all(&ws.root.join(".agent-workspace/factory/events.jsonl")).unwrap();
    assert!(log.events.iter().any(|ev| {
        matches!(
            ev.to_event_data(),
            Ok(EventData::SettingsChanged {
                ref key,
                ref old_value,
                ref new_value,
                ref actor,
            }) if key == "max_parallel_agents"
                && *old_value == json!(3)
                && *new_value == json!(5)
                && actor == "alice"
        )
    }));
}

#[test]
fn cli_settings_get_and_set_round_trip() {
    let ws = TestWorkspace::new("cli");

    let (code, stdout, stderr) = ws.run_cli(&["factory", "settings", "get", "--json"], &[]);
    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));
    let get_json: Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(get_json["max_parallel_agents"], json!(3));
    assert_eq!(get_json["default_workflow"], json!("default-delivery"));

    let (bad_code, _, _) = ws.run_cli(
        &[
            "factory",
            "settings",
            "set",
            "max_parallel_agents",
            "99",
            "--json",
        ],
        &[("USER", "alice")],
    );
    assert_ne!(bad_code, 0);

    let (agent_code, _, _) = ws.run_cli(
        &[
            "factory",
            "settings",
            "set",
            "max_parallel_agents",
            "4",
            "--json",
        ],
        &[("TANDEM_ROLE", "implementer"), ("USER", "alice")],
    );
    assert_ne!(agent_code, 0);

    let (set_code, set_stdout, set_stderr) = ws.run_cli(
        &[
            "factory",
            "settings",
            "set",
            "max_parallel_agents",
            "4",
            "--json",
        ],
        &[("USER", "alice")],
    );
    assert_eq!(
        set_code,
        0,
        "stderr: {}",
        String::from_utf8_lossy(&set_stderr)
    );
    let set_json: Value = serde_json::from_slice(&set_stdout).unwrap();
    assert_eq!(set_json["key"], json!("max_parallel_agents"));
    assert_eq!(set_json["old_value"], json!(3));
    assert_eq!(set_json["new_value"], json!(4));

    let (after_code, after_stdout, _) = ws.run_cli(&["factory", "settings", "get", "--json"], &[]);
    assert_eq!(after_code, 0);
    let after_json: Value = serde_json::from_slice(&after_stdout).unwrap();
    assert_eq!(after_json["max_parallel_agents"], json!(4));
}
