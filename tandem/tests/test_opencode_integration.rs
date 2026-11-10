use std::collections::HashMap;
use std::path::Path;

use tandem::adapters::AgentSpec;
use tandem::commands::agent_launch;
use tandem::conversations;
use tandem::quota::format_line;
use tandem::state::{self, CLIS};
use tandem::updater::{StaleAgent, default_selection, describe_stale};
use tandem::wizard::flow::{Step, WizardError, WizardFlow};

fn opencode_spec(model: &str) -> AgentSpec {
    AgentSpec::new("opencode", "implementer", model)
}

fn flow(opencode_installed: bool) -> WizardFlow {
    let mut installed = HashMap::new();
    installed.insert("claude".to_string(), true);
    installed.insert("agy".to_string(), true);
    installed.insert("opencode".to_string(), opencode_installed);
    WizardFlow::new(installed)
}

#[test]
fn opencode_is_a_known_cli_after_claude_and_agy() {
    assert_eq!(CLIS, ["claude", "agy", "opencode"]);
    assert_eq!(tandem::wizard::CLIS, CLIS);
}

#[test]
fn installed_clis_reports_opencode() {
    assert!(tandem::wizard::runner::installed_clis().contains_key("opencode"));
}

#[test]
fn saved_opencode_agents_load_and_bad_ones_are_dropped() {
    let good = serde_json::json!({
        "cli": "opencode", "role": "planner", "model": "anthropic/claude-sonnet-5-5", "index": 1,
    });
    let with_variant = serde_json::json!({
        "cli": "opencode", "role": "implementer", "model": "openai/gpt-5.4#high", "index": 2,
        "skip_permissions": true,
    });
    let bad = [
        serde_json::json!({"cli": "opencode", "role": "planner", "model": "claude", "index": 1}),
        serde_json::json!({"cli": "opencode", "role": "planner", "model": "a/b/c", "index": 1}),
        serde_json::json!({"cli": "opencode", "role": "planner", "model": "--auto/x", "index": 1}),
        serde_json::json!({"cli": "opencode", "role": "planner", "model": "a/b;rm", "index": 1}),
        serde_json::json!({"cli": "claude", "role": "planner", "model": "a/b", "index": 1}),
        serde_json::json!({"cli": "agy", "role": "planner", "model": "a/b", "index": 1}),
    ];
    let mut agents: Vec<serde_json::Value> = bad.to_vec();
    agents.push(good);
    agents.push(with_variant);
    let parsed = state::parse(&serde_json::json!({ "version": 1, "agents": agents })).unwrap();
    assert_eq!(parsed.agents.len(), 2);
    assert_eq!(parsed.agents[0].model, "anthropic/claude-sonnet-5-5");
    assert_eq!(parsed.agents[1].model, "openai/gpt-5.4#high");
    assert!(parsed.agents[1].skip_permissions);
}

#[test]
fn saved_state_without_opencode_still_loads_unchanged() {
    let parsed = state::parse(&serde_json::json!({
        "version": 1,
        "agents": [{"cli": "claude", "role": "planner", "model": "opus", "index": 1}],
    }))
    .unwrap();
    assert_eq!(parsed.agents.len(), 1);
}

#[test]
fn a_saved_opencode_conversation_is_not_trusted() {
    let parsed = state::parse(&serde_json::json!({
        "version": 2,
        "agents": [{
            "cli": "opencode", "role": "planner", "model": "a/b", "index": 1,
            "conversation": "123e4567-e89b-12d3-a456-426614174000",
        }],
    }))
    .unwrap();
    assert_eq!(parsed.agents[0].conversation, None);
}

#[test]
fn wizard_can_pick_opencode_when_installed_and_asks_for_permissions() {
    let mut f = flow(true);
    f.choose_cli("opencode").unwrap();
    f.choose_role("planner").unwrap();
    f.choose_model("anthropic/claude-sonnet-5-5").unwrap();
    assert_eq!(f.step(), Step::Perms);
    f.choose_skip_permissions(false);
    assert_eq!(f.step(), Step::Summary);
    let a = &f.agents[0];
    assert_eq!(
        (a.cli.as_str(), a.model.as_str(), a.skip_permissions),
        ("opencode", "anthropic/claude-sonnet-5-5", false)
    );
}

#[test]
fn wizard_permissions_choice_is_kept_for_opencode_and_back_returns_to_it() {
    let mut f = flow(true);
    f.choose_cli("opencode").unwrap();
    f.choose_role("implementer").unwrap();
    f.choose_model("openai/gpt-5.4#high").unwrap();
    f.choose_skip_permissions(true);
    assert_eq!(f.step(), Step::Cwd);
    f.back();
    assert_eq!(f.step(), Step::Perms);
}

#[test]
fn wizard_rejects_opencode_when_not_installed() {
    let mut f = flow(false);
    assert!(matches!(
        f.choose_cli("opencode"),
        Err(WizardError::NotInstalled(_))
    ));
}

#[test]
fn wizard_validates_models_per_cli() {
    let mut f = flow(true);
    f.choose_cli("opencode").unwrap();
    f.choose_role("planner").unwrap();
    assert!(f.choose_model("claude").is_err());
    assert!(f.choose_model("a/b/c").is_err());
    assert_eq!(f.step(), Step::Model);

    let mut c = flow(true);
    c.choose_cli("claude").unwrap();
    c.choose_role("planner").unwrap();
    assert!(c.choose_model("anthropic/claude").is_err());
    c.choose_model("opus").unwrap();
}

#[test]
fn opencode_launch_is_the_binary_with_the_model_in_the_environment() {
    let launch = agent_launch(
        &opencode_spec("anthropic/claude-sonnet-5-5"),
        "/c",
        Path::new("/w"),
        false,
        None,
    );
    assert_eq!(launch.argv, vec!["opencode"]);
    let doc: serde_json::Value =
        serde_json::from_str(&launch.env["OPENCODE_CONFIG_CONTENT"]).unwrap();
    assert_eq!(
        doc,
        serde_json::json!({"model": "anthropic/claude-sonnet-5-5"})
    );
    assert_eq!(launch.env["TANDEM_CLI"], "opencode");
    assert_eq!(launch.env["TANDEM_SESSION_ID"], "implementer-1");
    assert_eq!(launch.label, "opencode·anthropic/claude-sonnet-5-5");
    let spec: serde_json::Value = serde_json::from_str(&launch.spec).unwrap();
    assert_eq!(spec["cli"], "opencode");
    assert!(spec["conversation"].is_null());
}

#[test]
fn opencode_skip_permissions_adds_auto() {
    let mut s = opencode_spec("a/b");
    s.skip_permissions = true;
    let launch = agent_launch(&s, "/c", Path::new("/w"), false, None);
    assert_eq!(launch.argv, vec!["opencode", "--auto"]);
}

#[test]
fn opencode_always_starts_fresh_even_when_a_conversation_is_present() {
    let mut s = opencode_spec("a/b");
    s.conversation = Some("ses_0123456789abcdef".to_string());
    for resume in [false, true] {
        let launch = agent_launch(&s, "/c", Path::new("/w"), resume, None);
        assert_eq!(launch.argv, vec!["opencode"], "resume={resume}");
        let spec: serde_json::Value = serde_json::from_str(&launch.spec).unwrap();
        assert!(spec["conversation"].is_null(), "resume={resume}");
    }
}

#[test]
fn other_clis_get_no_opencode_environment() {
    let launch = agent_launch(
        &AgentSpec::new("claude", "planner", "opus"),
        "/c",
        Path::new("/w"),
        false,
        None,
    );
    assert!(!launch.env.contains_key("OPENCODE_CONFIG_CONTENT"));
    let agy = agent_launch(
        &AgentSpec::new("agy", "planner", "m1"),
        "/c",
        Path::new("/w"),
        false,
        None,
    );
    assert!(!agy.env.contains_key("OPENCODE_CONFIG_CONTENT"));
}

#[test]
fn opencode_conversations_are_not_resumable_yet() {
    let claude_dir = Path::new("/nonexistent");
    assert!(!conversations::can_resume(
        "opencode",
        Some("ses_0123456789abcdef"),
        claude_dir
    ));
    assert!(!conversations::can_resume("opencode", None, claude_dir));
    assert_eq!(
        conversations::current(Path::new("/nonexistent"), "implementer-1", "opencode"),
        None
    );
}

#[test]
fn quota_line_says_not_available_for_opencode() {
    assert_eq!(
        format_line(&["opencode"], None, 1.0),
        "Quota  opencode: quota not available"
    );
    let all = format_line(&["claude", "agy", "opencode"], None, 1.0);
    assert_eq!(all.matches("quota not available").count(), 3);
    assert!(all.contains("opencode: quota not available"));
}

#[test]
fn quota_line_for_other_clis_is_unchanged() {
    assert_eq!(
        format_line(&["agy"], None, 1.0),
        "Quota  agy: quota not available"
    );
}

#[test]
fn stale_opencode_agents_are_not_selected_for_restart_by_default() {
    let agent = StaleAgent {
        pane_id: "%7".to_string(),
        role: "implementer".to_string(),
        index: 1,
        cli: "opencode".to_string(),
        model: "a/b".to_string(),
        launched: "2.2.7".to_string(),
        installed: "2.3.0".to_string(),
        resumable: false,
        busy: false,
        claim: String::new(),
    };
    assert!(default_selection(std::slice::from_ref(&agent)).is_empty());
    let line = describe_stale(&agent);
    assert!(line.contains("opencode·a/b"));
    assert!(line.contains("would start a new chat"));
}
