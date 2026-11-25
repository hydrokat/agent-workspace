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
    let rotated = spec_json(&launch)["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(
        rotated, CONV,
        "unresumable Claude conversation must rotate to a fresh UUID instead of reusing {CONV} with --session-id"
    );
    assert_eq!(
        &launch.argv[launch.argv.len() - 2..],
        &["--session-id", &rotated]
    );

    let fresh_with_stale = agent_launch(
        &spec,
        tmp.path().to_str().unwrap(),
        tmp.path(),
        false,
        Some(&claude_dir),
    );
    let fresh_conv = spec_json(&fresh_with_stale)["conversation"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(
        fresh_conv, CONV,
        "fresh Claude launch with resume=false must rotate a pre-existing conversation UUID"
    );
    assert_eq!(
        &fresh_with_stale.argv[fresh_with_stale.argv.len() - 2..],
        &["--session-id", &fresh_conv]
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

#[test]
fn test_resume_specs_deduplicates_duplicate_conversation_ids() {
    let tmp = TempDir::new();
    let mut first = AgentSpec::new("claude", "orchestrator", "opus");
    first.index = 1;
    first.idea = Some("i-11111111".to_string());
    first.conversation = Some(CONV.to_string());

    let mut duplicate = AgentSpec::new("claude", "orchestrator", "opus");
    duplicate.index = 2;
    duplicate.idea = Some("i-22222222".to_string());
    duplicate.conversation = Some(CONV.to_ascii_uppercase());

    let other_uuid = "7a9b8c0d-1e2f-3a4b-5c6d-7e8f9a0b1c2d";
    let mut distinct = AgentSpec::new("claude", "orchestrator", "opus");
    distinct.index = 3;
    distinct.idea = Some("i-33333333".to_string());
    distinct.conversation = Some(other_uuid.to_string());

    let saved = SavedState::new(
        vec![first, duplicate, distinct],
        std::collections::BTreeMap::new(),
        None,
        None,
        1.0,
    );
    let (specs, notes) = resume_specs(&saved, tmp.path(), |c| {
        Some(std::path::PathBuf::from(format!("/bin/{c}")))
    });

    assert_eq!(specs.len(), 3);
    assert_eq!(specs[0].conversation.as_deref(), Some(CONV));
    assert_eq!(specs[1].conversation, None);
    assert_eq!(specs[2].conversation.as_deref(), Some(other_uuid));
    assert_eq!(notes.len(), 1);
    assert!(notes[0].contains("duplicate"), "notes: {notes:?}");
}

#[test]
fn test_add_agent_resumes_transcript_and_rotates_on_live_pane_collision() {
    if !common::require_tool("tmux") {
        return;
    }
    let tmp = TempDir::new();
    let ws = tmp.path().join("ws");
    let state_dir = tmp.path().join("state");
    let claude_dir = tmp.path().join("claude");
    fs::create_dir_all(&ws).unwrap();
    fs::create_dir_all(&state_dir).unwrap();

    let proj = claude_dir.join("projects").join("p");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join(format!("{CONV}.jsonl")), "{}\n").unwrap();

    struct ServerGuard(String);
    impl Drop for ServerGuard {
        fn drop(&mut self) {
            let _ = std::process::Command::new("tmux")
                .args(["-L", &self.0, "kill-server"])
                .output();
        }
    }
    let _guard = ServerGuard(tandem::tmux::client::socket_for(&ws));

    let cfg = tandem::config::Config {
        coord_dir: None,
        color: false,
        tmux_pane: String::new(),
        workspace: Some(ws.clone()),
        state_dir,
        update_spec: "hydrokat/agent-workspace".to_string(),
        session_id: String::new(),
        claude_dir,
        update_interval: 14400,
        update_check: false,
        factory: true,
    };

    let layout = tandem::commands::make_layout(&ws, &cfg);
    let ph_main = tandem::commands::placeholder_launch("orchestrator", None, Some(&ws));
    let ph_sb = tandem::tmux::layout::Launch::new(
        vec!["sleep".to_string(), "60".to_string()],
        "sidebar",
        0,
        "sidebar",
    );
    let ph_st = tandem::tmux::layout::Launch::new(
        vec!["sleep".to_string(), "60".to_string()],
        "status",
        0,
        "status",
    );
    layout.build(160, 45, &ph_sb, &ph_main, &ph_st).unwrap();

    let mut spec1 = AgentSpec::new("claude", "orchestrator", "opus");
    spec1.idea = Some("i-aaaaaaaa".to_string());
    spec1.conversation = Some(CONV.to_string());

    let launched1 = tandem::commands::add_agent(&spec1, &ws, &cfg).expect("first add_agent");
    assert_eq!(launched1.conversation.as_deref(), Some(CONV));

    let panes_after_1 = layout.panes().unwrap();
    let pane1 = panes_after_1
        .iter()
        .find(|p| p.kind == "agent" && p.idea == "i-aaaaaaaa")
        .expect("pane1 created");
    let argv1_raw = layout
        .client
        .run_checked(&["display-message", "-p", "-t", &pane1.id, "#{@tandem_argv}"])
        .unwrap();
    let argv1: Vec<String> = serde_json::from_str(argv1_raw.trim()).unwrap();
    assert_eq!(
        &argv1[argv1.len() - 2..],
        &["--resume", CONV],
        "add_agent with existing transcript must use --resume"
    );

    let mut spec2 = AgentSpec::new("claude", "orchestrator", "opus");
    spec2.idea = Some("i-bbbbbbbb".to_string());
    spec2.conversation = Some(CONV.to_string());

    let launched2 = tandem::commands::add_agent(&spec2, &ws, &cfg).expect("second add_agent");
    let conv2 = launched2
        .conversation
        .as_deref()
        .expect("second agent gets fresh conversation UUID");
    assert_ne!(
        conv2, CONV,
        "add_agent must rotate conversation UUID when a live pane already uses {CONV}"
    );

    let panes_after_2 = layout.panes().unwrap();
    let pane2 = panes_after_2
        .iter()
        .find(|p| p.kind == "agent" && p.idea == "i-bbbbbbbb")
        .expect("pane2 created");
    let argv2_raw = layout
        .client
        .run_checked(&["display-message", "-p", "-t", &pane2.id, "#{@tandem_argv}"])
        .unwrap();
    let argv2: Vec<String> = serde_json::from_str(argv2_raw.trim()).unwrap();
    assert_eq!(
        &argv2[argv2.len() - 2..],
        &["--session-id", conv2],
        "add_agent colliding with a live pane must launch with fresh --session-id"
    );
}
