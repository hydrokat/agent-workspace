mod common;

use std::fs;
use std::path::Path;

use common::TempDir;
use tandem::adapters::AgentSpec;
use tandem::commands::{agent_launch, placeholder_launch, resume_specs, status_launch};
use tandem::conversations;
use tandem::state::SavedState;

const CONV: &str = "46f2fc12-7f1f-4ca2-b714-06cbbfd669e9";

fn spec_json(launch: &tandem::tmux::layout::Launch) -> serde_json::Value {
    serde_json::from_str(&launch.spec).expect("parse spec json")
}

#[test]
fn test_agent_launch_builds_argv_env_and_label() {
    let mut spec = AgentSpec::new("claude", "implementer", "sonnet");
    spec.index = 2;
    spec.cwd = Some("/w/wt".to_string());

    let launch = agent_launch(&spec, "/c", Path::new("/w"), false, None);

    assert_eq!(&launch.argv[..3], &["claude", "--permission-mode", "auto"]);
    assert_eq!(
        launch.env.get("TANDEM_SESSION_ID"),
        Some(&"implementer-2".to_string())
    );
    assert_eq!(launch.env.get("TANDEM_COORD_DIR"), Some(&"/c".to_string()));
    assert_eq!(launch.cwd.as_deref(), Some("/w/wt"));
    assert_eq!(launch.label, "claude·sonnet");
    assert_eq!(launch.kind, "agent");
    assert_eq!(launch.session_id, "implementer-2");
    assert_eq!(launch.index, 2);
}

#[test]
fn test_agent_launch_defaults_cwd_to_workspace() {
    let spec = AgentSpec::new("agy", "planner", "m");
    let launch = agent_launch(&spec, "/c", Path::new("/w"), false, None);
    assert_eq!(launch.cwd.as_deref(), Some("/w"));
}

#[test]
fn test_placeholders_are_tandem_owned_processes() {
    let self_cmd = vec!["/bin/tandem".to_string()];
    let p = placeholder_launch("planner", Some(&self_cmd), None);
    assert_eq!(p.kind, "own");
    assert_eq!(p.argv, vec!["/bin/tandem", "placeholder", "planner"]);

    let s = status_launch(Some(&self_cmd), None);
    assert_eq!(s.kind, "own");
    assert_eq!(s.role, "status");
    assert_eq!(s.argv, vec!["/bin/tandem", "status"]);
}

#[test]
fn test_new_claude_agent_gets_its_own_conversation_id() {
    let tmp = TempDir::new();
    let spec1 = AgentSpec::new("claude", "planner", "opus");
    let spec2 = AgentSpec::new("claude", "planner", "opus");

    let one = agent_launch(
        &spec1,
        tmp.path().to_str().unwrap(),
        tmp.path(),
        false,
        None,
    );
    let two = agent_launch(
        &spec2,
        tmp.path().to_str().unwrap(),
        tmp.path(),
        false,
        None,
    );

    let conv1 = spec_json(&one)["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    let conv2 = spec_json(&two)["conversation"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(&one.argv[one.argv.len() - 2..], &["--session-id", &conv1]);
    assert_ne!(conv1, conv2);
}

#[test]
fn test_claude_resume_reopens_only_a_saved_transcript() {
    let tmp = TempDir::new();
    let claude_dir = tmp.path().join("claude");
    let mut spec = AgentSpec::new("claude", "planner", "opus");
    spec.conversation = Some(CONV.to_string());

    let launch = agent_launch(
        &spec,
        tmp.path().to_str().unwrap(),
        tmp.path(),
        true,
        Some(&claude_dir),
    );
    assert_eq!(
        &launch.argv[launch.argv.len() - 2..],
        &["--session-id", CONV]
    );

    let proj = claude_dir.join("projects").join("p");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join(format!("{CONV}.jsonl")), "{}\n").unwrap();

    let launch_resumed = agent_launch(
        &spec,
        tmp.path().to_str().unwrap(),
        tmp.path(),
        true,
        Some(&claude_dir),
    );
    assert_eq!(
        &launch_resumed.argv[launch_resumed.argv.len() - 2..],
        &["--resume", CONV]
    );
    assert_eq!(
        spec_json(&launch_resumed)["conversation"].as_str(),
        Some(CONV)
    );
}

#[test]
fn test_agy_launch_logs_per_agent_and_resumes_known_conversation() {
    let tmp = TempDir::new();
    let log = conversations::agy_log_path(tmp.path(), "implementer-1").unwrap();
    fs::create_dir_all(log.parent().unwrap()).unwrap();
    fs::write(&log, format!("Created conversation {CONV}\n")).unwrap();

    let mut spec = AgentSpec::new("agy", "implementer", "m");
    spec.conversation = Some(CONV.to_string());

    let resumed = agent_launch(&spec, tmp.path().to_str().unwrap(), tmp.path(), true, None);
    assert_eq!(
        &resumed.argv[resumed.argv.len() - 4..],
        &[
            "--log-file".to_string(),
            log.to_string_lossy().to_string(),
            "--conversation".to_string(),
            CONV.to_string()
        ]
    );
    assert!(log.exists());

    let fresh = agent_launch(&spec, tmp.path().to_str().unwrap(), tmp.path(), false, None);
    assert!(!fresh.argv.contains(&"--conversation".to_string()));
    assert!(spec_json(&fresh)["conversation"].is_null());
    assert!(!log.exists());
}

#[test]
fn test_resume_specs_drop_claude_chat_when_its_folder_is_gone() {
    let tmp = TempDir::new();
    let mut claude_spec = AgentSpec::new("claude", "planner", "opus");
    claude_spec.index = 1;
    claude_spec.cwd = Some(tmp.path().join("gone").to_string_lossy().to_string());
    claude_spec.conversation = Some(CONV.to_string());

    let mut agy_spec = AgentSpec::new("agy", "implementer", "m");
    agy_spec.index = 1;
    agy_spec.cwd = Some(tmp.path().join("gone").to_string_lossy().to_string());
    agy_spec.conversation = Some(CONV.to_string());

    let saved = SavedState::new(
        vec![claude_spec, agy_spec],
        std::collections::BTreeMap::new(),
        None,
        None,
        1.0,
    );
    let (specs, notes) = resume_specs(&saved, tmp.path(), |c| {
        Some(std::path::PathBuf::from(format!("/bin/{c}")))
    });

    assert_eq!(specs.len(), 2);
    assert_eq!(specs[0].cwd, None);
    assert_eq!(specs[0].conversation, None);
    assert_eq!(specs[1].cwd, None);
    assert_eq!(specs[1].conversation, Some(CONV.to_string()));
    assert_eq!(notes.len(), 2);
}
