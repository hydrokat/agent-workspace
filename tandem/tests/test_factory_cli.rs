use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use tandem::factory::model::TEXT_MAX_LEN;

struct TestWorkspace {
    root: PathBuf,
    state_dir: PathBuf,
}

impl TestWorkspace {
    fn new(name: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "tandem-factory-cli-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        let root = base.join("ws");
        let state_dir = base.join("state");
        fs::create_dir_all(&root).expect("create temp workspace");
        fs::create_dir_all(&state_dir).expect("create temp state dir");
        Self { root, state_dir }
    }

    fn run(&self, args: &[&str], extra_env: &[(&str, &str)], stdin_data: Option<&str>) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tandem"));
        cmd.args(args)
            .env("TANDEM_WORKSPACE", &self.root)
            .env("XDG_STATE_HOME", &self.state_dir)
            .env_remove("TANDEM_FACTORY")
            .env_remove("TANDEM_ROLE")
            .env_remove("TANDEM_SESSION_ID")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for (k, v) in extra_env {
            cmd.env(k, v);
        }

        if stdin_data.is_some() {
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }

        let mut child = cmd.spawn().expect("spawn tandem");
        if let Some(input) = stdin_data
            && let Some(mut stdin) = child.stdin.take()
        {
            stdin.write_all(input.as_bytes()).expect("write stdin");
        }
        child.wait_with_output().expect("wait tandem")
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        if let Some(parent) = self.root.parent() {
            let _ = fs::remove_dir_all(parent);
        }
    }
}

fn parse_single_line_json(bytes: &[u8]) -> Value {
    let text = std::str::from_utf8(bytes).expect("stdout must be valid utf-8");
    assert!(
        text.ends_with('\n'),
        "JSON output must be newline-terminated: {text:?}"
    );
    let trimmed = text.trim_end_matches('\n');
    assert!(
        !trimmed.contains('\n'),
        "JSON output must be a single compact line: {text:?}"
    );
    serde_json::from_str(trimmed).expect("stdout must be valid JSON")
}

#[test]
fn test_factory_status_works_when_disabled_and_empty() {
    let ws = TestWorkspace::new("status-empty");
    let out = ws.run(&["factory", "status", "--json"], &[], None);
    assert_eq!(out.status.code(), Some(0));

    let json = parse_single_line_json(&out.stdout);
    assert_eq!(json["enabled"], false);
    assert_eq!(json["ideas"], 0);
    assert_eq!(json["by_state"], serde_json::json!({}));
    assert!(json["settings"].is_object());
    assert_eq!(json["log"]["events"], 0);
    assert_eq!(json["log"]["truncated"], false);
    assert!(
        !ws.root.join(".agent-workspace/factory").exists(),
        "status must not create factory store directory"
    );
}

#[test]
fn test_factory_reads_return_empty_when_disabled() {
    let ws = TestWorkspace::new("reads-disabled");

    let list_out = ws.run(&["factory", "idea", "list", "--json"], &[], None);
    assert_eq!(list_out.status.code(), Some(0));
    let list_json = parse_single_line_json(&list_out.stdout);
    assert_eq!(list_json["ideas"], serde_json::json!([]));

    let events_out = ws.run(&["factory", "events", "--json"], &[], None);
    assert_eq!(events_out.status.code(), Some(0));
    let events_json = parse_single_line_json(&events_out.stdout);
    assert_eq!(events_json["events"], serde_json::json!([]));
    assert_eq!(events_json["last_seq"], 0);
    assert_eq!(events_json["truncated"], false);
}

#[test]
fn test_factory_writes_exit_2_when_factory_mode_off() {
    let ws = TestWorkspace::new("writes-disabled");

    let new_plain = ws.run(
        &["factory", "idea", "new", "--title", "x", "--text", "y"],
        &[],
        None,
    );
    assert_eq!(new_plain.status.code(), Some(2));
    assert!(new_plain.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&new_plain.stderr);
    assert_eq!(
        stderr.trim(),
        "Factory Mode is not active in this workspace. Start it with ./run.sh --factory"
    );

    let new_json = ws.run(
        &[
            "factory", "idea", "new", "--title", "x", "--text", "y", "--json",
        ],
        &[],
        None,
    );
    assert_eq!(new_json.status.code(), Some(2));
    let err_json = parse_single_line_json(&new_json.stdout);
    assert_eq!(err_json["error"]["code"], "factory_disabled");
    assert_eq!(
        err_json["error"]["message"],
        "Factory Mode is not active in this workspace. Start it with ./run.sh --factory"
    );

    let rebuild_out = ws.run(&["factory", "rebuild", "--json"], &[], None);
    assert_eq!(rebuild_out.status.code(), Some(2));
    let rebuild_err = parse_single_line_json(&rebuild_out.stdout);
    assert_eq!(rebuild_err["error"]["code"], "factory_disabled");
}

#[test]
fn test_factory_idea_lifecycle_new_list_show_events_rebuild() {
    let ws = TestWorkspace::new("lifecycle");
    let env = &[("TANDEM_FACTORY", "1")];

    let created_1 = ws.run(
        &[
            "factory",
            "idea",
            "new",
            "--title",
            "Export team activity",
            "--text",
            "CSV for the last 90 days",
            "--owner",
            "alice",
            "--json",
        ],
        env,
        None,
    );
    assert_eq!(
        created_1.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&created_1.stderr)
    );
    let idea1 = parse_single_line_json(&created_1.stdout);
    let id1 = idea1["id"].as_str().expect("idea id").to_string();
    assert!(id1.starts_with("i-"));
    assert_eq!(idea1["title"], "Export team activity");
    assert_eq!(idea1["text"], "CSV for the last 90 days");
    assert_eq!(idea1["state"], "inbox");
    assert_eq!(idea1["owner"], "alice");

    let created_2 = ws.run(
        &[
            "factory",
            "idea",
            "new",
            "--title",
            "Second idea",
            "--text",
            "More details",
            "--json",
        ],
        env,
        None,
    );
    assert_eq!(created_2.status.code(), Some(0));
    let idea2 = parse_single_line_json(&created_2.stdout);
    let id2 = idea2["id"].as_str().expect("idea2 id").to_string();

    let status_out = ws.run(&["factory", "status", "--json"], env, None);
    assert_eq!(status_out.status.code(), Some(0));
    let status_json = parse_single_line_json(&status_out.stdout);
    assert_eq!(status_json["enabled"], true);
    assert_eq!(status_json["ideas"], 2);
    assert_eq!(status_json["by_state"]["inbox"], 2);
    assert_eq!(status_json["log"]["events"], 2);

    let list_out = ws.run(&["factory", "idea", "list", "--json"], env, None);
    assert_eq!(list_out.status.code(), Some(0));
    let list_json = parse_single_line_json(&list_out.stdout);
    let ideas = list_json["ideas"].as_array().expect("ideas array");
    assert_eq!(ideas.len(), 2);
    assert_eq!(ideas[0]["id"], id2);
    assert_eq!(ideas[1]["id"], id1);

    let filtered_inbox = ws.run(
        &["factory", "idea", "list", "--state", "inbox", "--json"],
        env,
        None,
    );
    assert_eq!(filtered_inbox.status.code(), Some(0));
    assert_eq!(
        parse_single_line_json(&filtered_inbox.stdout)["ideas"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let filtered_planning = ws.run(
        &["factory", "idea", "list", "--state", "planning", "--json"],
        env,
        None,
    );
    assert_eq!(filtered_planning.status.code(), Some(0));
    assert_eq!(
        parse_single_line_json(&filtered_planning.stdout)["ideas"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let show_out = ws.run(&["factory", "idea", "show", &id1, "--json"], env, None);
    assert_eq!(show_out.status.code(), Some(0));
    let show_json = parse_single_line_json(&show_out.stdout);
    assert_eq!(show_json["idea"]["id"], id1);
    let show_events = show_json["events"].as_array().expect("events array");
    assert_eq!(show_events.len(), 1);
    assert_eq!(show_events[0]["actor"]["kind"], "user");
    assert_eq!(show_events[0]["actor"]["id"], "cli");

    let events_since = ws.run(&["factory", "events", "--since", "1", "--json"], env, None);
    assert_eq!(events_since.status.code(), Some(0));
    let events_since_json = parse_single_line_json(&events_since.stdout);
    let evs = events_since_json["events"].as_array().unwrap();
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["idea"], id2);
    assert_eq!(events_since_json["last_seq"], 2);

    let events_by_idea = ws.run(&["factory", "events", "--idea", &id1, "--json"], env, None);
    assert_eq!(events_by_idea.status.code(), Some(0));
    let ev_idea_json = parse_single_line_json(&events_by_idea.stdout);
    assert_eq!(ev_idea_json["events"].as_array().unwrap().len(), 1);
    assert_eq!(ev_idea_json["events"][0]["idea"], id1);

    let idea1_file = ws
        .root
        .join(format!(".agent-workspace/factory/ideas/{id1}.json"));
    fs::remove_file(&idea1_file).expect("remove materialized idea file");
    assert!(!idea1_file.exists());

    let rebuild_out = ws.run(&["factory", "rebuild", "--json"], env, None);
    assert_eq!(rebuild_out.status.code(), Some(0));
    let rebuild_json = parse_single_line_json(&rebuild_out.stdout);
    assert_eq!(rebuild_json["rebuilt"], 2);
    assert_eq!(rebuild_json["truncated"], false);
    assert!(idea1_file.exists());
}

#[test]
fn test_factory_actor_derived_from_environment() {
    let ws = TestWorkspace::new("actor");
    let env = &[
        ("TANDEM_FACTORY", "1"),
        ("TANDEM_ROLE", "orchestrator"),
        ("TANDEM_SESSION_ID", "orchestrator-1"),
    ];
    let created = ws.run(
        &[
            "factory",
            "idea",
            "new",
            "--title",
            "From orchestrator",
            "--text",
            "Body",
            "--json",
        ],
        env,
        None,
    );
    assert_eq!(created.status.code(), Some(0));
    let idea = parse_single_line_json(&created.stdout);
    let id = idea["id"].as_str().unwrap();

    let show = ws.run(&["factory", "idea", "show", id, "--json"], env, None);
    assert_eq!(show.status.code(), Some(0));
    let show_json = parse_single_line_json(&show.stdout);
    assert_eq!(show_json["events"][0]["actor"]["kind"], "orchestrator");
    assert_eq!(show_json["events"][0]["actor"]["id"], "orchestrator-1");
}

#[test]
fn test_factory_text_file_and_stdin_and_security_refusals() {
    let ws = TestWorkspace::new("text-file");
    let env = &[("TANDEM_FACTORY", "1")];

    let text_path = ws.root.join("idea.txt");
    fs::write(&text_path, "Line 1\n\tLine 2").unwrap();
    let out_file = ws.run(
        &[
            "factory",
            "idea",
            "new",
            "--title",
            "From file",
            "--text-file",
            text_path.to_str().unwrap(),
            "--json",
        ],
        env,
        None,
    );
    assert_eq!(out_file.status.code(), Some(0));
    let idea_file = parse_single_line_json(&out_file.stdout);
    assert_eq!(idea_file["text"], "Line 1\n\tLine 2");

    let out_stdin = ws.run(
        &[
            "factory",
            "idea",
            "new",
            "--title",
            "From stdin",
            "--text-file",
            "-",
            "--json",
        ],
        env,
        Some("Stdin body"),
    );
    assert_eq!(out_stdin.status.code(), Some(0));
    let idea_stdin = parse_single_line_json(&out_stdin.stdout);
    assert_eq!(idea_stdin["text"], "Stdin body");

    let dir_path = ws.root.join("subdir");
    fs::create_dir_all(&dir_path).unwrap();
    let out_dir = ws.run(
        &[
            "factory",
            "idea",
            "new",
            "--title",
            "Dir",
            "--text-file",
            dir_path.to_str().unwrap(),
            "--json",
        ],
        env,
        None,
    );
    assert_eq!(out_dir.status.code(), Some(1));
    let dir_err = parse_single_line_json(&out_dir.stdout);
    assert_eq!(dir_err["error"]["code"], "invalid_input");

    #[cfg(unix)]
    {
        let link_path = ws.root.join("link.txt");
        std::os::unix::fs::symlink(&text_path, &link_path).unwrap();
        let out_link = ws.run(
            &[
                "factory",
                "idea",
                "new",
                "--title",
                "Symlink",
                "--text-file",
                link_path.to_str().unwrap(),
                "--json",
            ],
            env,
            None,
        );
        assert_eq!(out_link.status.code(), Some(1));
        let link_err = parse_single_line_json(&out_link.stdout);
        assert_eq!(link_err["error"]["code"], "invalid_input");
    }

    let big_path = ws.root.join("big.txt");
    fs::write(&big_path, "a".repeat(TEXT_MAX_LEN + 10)).unwrap();
    let out_big = ws.run(
        &[
            "factory",
            "idea",
            "new",
            "--title",
            "Too big",
            "--text-file",
            big_path.to_str().unwrap(),
            "--json",
        ],
        env,
        None,
    );
    assert_eq!(out_big.status.code(), Some(1));
}

#[test]
fn test_factory_validation_and_not_found_exit_1_without_touching_store() {
    let ws = TestWorkspace::new("validation");
    let env = &[("TANDEM_FACTORY", "1")];

    let bad_title = ws.run(
        &[
            "factory", "idea", "new", "--title", "  ", "--text", "ok", "--json",
        ],
        env,
        None,
    );
    assert_eq!(bad_title.status.code(), Some(1));
    assert!(
        !ws.root.join(".agent-workspace/factory").exists(),
        "validation failure must not create store"
    );

    let bad_owner = ws.run(
        &[
            "factory",
            "idea",
            "new",
            "--title",
            "ok",
            "--text",
            "ok",
            "--owner",
            "bad owner",
            "--json",
        ],
        env,
        None,
    );
    assert_eq!(bad_owner.status.code(), Some(1));

    let bad_state = ws.run(
        &["factory", "idea", "list", "--state", "nonsense", "--json"],
        env,
        None,
    );
    assert_eq!(bad_state.status.code(), Some(1));

    let bad_show = ws.run(
        &["factory", "idea", "show", "not-an-id", "--json"],
        env,
        None,
    );
    assert_eq!(bad_show.status.code(), Some(1));

    let missing_show = ws.run(
        &["factory", "idea", "show", "i-12345678", "--json"],
        env,
        None,
    );
    assert_eq!(missing_show.status.code(), Some(1));
    let err = parse_single_line_json(&missing_show.stdout);
    assert_eq!(err["error"]["code"], "not_found");

    let bad_events = ws.run(
        &["factory", "events", "--idea", "../etc", "--json"],
        env,
        None,
    );
    assert_eq!(bad_events.status.code(), Some(1));
}

#[test]
fn test_factory_default_owner_falls_back_when_user_invalid() {
    let ws = TestWorkspace::new("owner-fallback");
    let env = &[("TANDEM_FACTORY", "1"), ("USER", "bad user!")];

    let out = ws.run(
        &[
            "factory",
            "idea",
            "new",
            "--title",
            "Owner test",
            "--text",
            "Body",
            "--json",
        ],
        env,
        None,
    );
    assert_eq!(out.status.code(), Some(0));
    let idea = parse_single_line_json(&out.stdout);
    assert_eq!(idea["owner"], "you");
}

#[test]
fn test_factory_agent_list_all_returns_an_empty_list_without_runs() {
    let ws = TestWorkspace::new("agent-list-all-empty");
    let env = &[("TANDEM_FACTORY", "1")];

    let all = ws.run(&["factory", "agent", "list", "--all", "--json"], env, None);
    assert_eq!(all.status.code(), Some(0));
    assert_eq!(parse_single_line_json(&all.stdout), serde_json::json!([]));

    let active = ws.run(
        &["factory", "agent", "list", "--all", "--active", "--json"],
        env,
        None,
    );
    assert_eq!(active.status.code(), Some(0));
    assert_eq!(
        parse_single_line_json(&active.stdout),
        serde_json::json!([])
    );
}

#[test]
fn test_factory_agent_list_requires_a_run_unless_all() {
    let ws = TestWorkspace::new("agent-list-args");
    let env = &[("TANDEM_FACTORY", "1")];

    for args in [
        &["factory", "agent", "list", "--json"][..],
        &["factory", "agent", "list", "--active", "--json"][..],
        &["factory", "agent", "list", "r-12345678", "--all", "--json"][..],
    ] {
        let out = ws.run(args, env, None);
        assert_eq!(out.status.code(), Some(2), "{args:?} must be a usage error");
    }
}

fn tmux_installed() -> bool {
    std::process::Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn test_resume_idea_uses_resume_flag_and_rotates_on_rebound_live_pane_collision() {
    if !tmux_installed() {
        return;
    }
    use tandem::adapters::AgentSpec;
    use tandem::commands::{make_layout, placeholder_launch};
    use tandem::config::Config;
    use tandem::factory::commands::resume_idea;
    use tandem::factory::machine::Command as FactoryCommand;
    use tandem::factory::model::{Actor, ActorKind};
    use tandem::factory::store::FactoryStore;
    use tandem::tmux::layout::Launch;

    const CONV: &str = "46f2fc12-7f1f-4ca2-b714-06cbbfd669e9";
    let ws = TestWorkspace::new("resume-idea-conv");
    let claude_dir = ws.state_dir.join("claude");
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
    let _guard = ServerGuard(tandem::tmux::client::socket_for(&ws.root));

    let cfg = Config {
        coord_dir: None,
        color: false,
        tmux_pane: String::new(),
        workspace: Some(ws.root.clone()),
        state_dir: ws.state_dir.clone(),
        update_spec: "hydrokat/agent-workspace".to_string(),
        session_id: String::new(),
        claude_dir,
        update_interval: 14400,
        update_check: false,
        factory: true,
    };

    let layout = make_layout(&ws.root, &cfg);
    let ph_main = placeholder_launch("orchestrator", None, Some(&ws.root));
    let ph_sb = Launch::new(
        vec!["sleep".to_string(), "60".to_string()],
        "sidebar",
        0,
        "sidebar",
    );
    let ph_st = Launch::new(
        vec!["sleep".to_string(), "60".to_string()],
        "status",
        0,
        "status",
    );
    layout.build(160, 45, &ph_sb, &ph_main, &ph_st).unwrap();

    let store = FactoryStore::new(&ws.root);
    let actor = Actor {
        kind: ActorKind::User,
        id: "alice".to_string(),
    };
    let idea_a = store
        .create_idea("First Idea", "Build feature A", "alice", &actor)
        .unwrap();
    store
        .execute(
            &idea_a.id,
            FactoryCommand::BindOrchestrator {
                cli: "claude".to_string(),
                model: "opus".to_string(),
                conversation: Some(CONV.to_string()),
            },
            &actor,
        )
        .unwrap();

    let resumed_a = resume_idea(&ws.root, &cfg, &idea_a.id).expect("resume closed idea A");
    assert_eq!(
        resumed_a
            .orchestrator
            .as_ref()
            .and_then(|o| o.conversation.as_deref()),
        Some(CONV)
    );
    let panes = layout.panes().unwrap();
    let pane_a = panes
        .iter()
        .find(|p| p.kind == "agent" && p.idea == idea_a.id)
        .expect("pane A launched");
    let argv_a_raw = layout
        .client
        .run_checked(&["display-message", "-p", "-t", &pane_a.id, "#{@tandem_argv}"])
        .unwrap();
    let argv_a: Vec<String> = serde_json::from_str(argv_a_raw.trim()).unwrap();
    assert_eq!(&argv_a[argv_a.len() - 2..], &["--resume", CONV]);

    let idea_b = store
        .create_idea("Second Idea", "Rebound pane idea B", "alice", &actor)
        .unwrap();
    store
        .execute(
            &idea_b.id,
            FactoryCommand::BindOrchestrator {
                cli: "claude".to_string(),
                model: "opus".to_string(),
                conversation: Some(CONV.to_string()),
            },
            &actor,
        )
        .unwrap();
    let mut spec_b = AgentSpec::new("claude", "orchestrator", "opus");
    spec_b.idea = Some(idea_b.id.clone());
    let rebound_spec_json = serde_json::json!({
        "cli": "claude",
        "model": "opus",
        "skip_permissions": false,
        "cwd": null,
        "conversation": CONV,
        "idea": idea_b.id,
    })
    .to_string();
    layout
        .client
        .run_batch_checked(&[
            vec![
                "set-option".to_string(),
                "-p".to_string(),
                "-t".to_string(),
                pane_a.id.clone(),
                "@tandem_idea".to_string(),
                idea_b.id.clone(),
            ],
            vec![
                "set-option".to_string(),
                "-p".to_string(),
                "-t".to_string(),
                pane_a.id.clone(),
                "@tandem_spec".to_string(),
                rebound_spec_json,
            ],
        ])
        .unwrap();

    let reopened_a = resume_idea(&ws.root, &cfg, &idea_a.id)
        .expect("reopen idea A after its pane was rebound to idea B");
    let new_conv = reopened_a
        .orchestrator
        .as_ref()
        .and_then(|o| o.conversation.as_deref())
        .expect("idea A must record rotated conversation");
    assert_ne!(
        new_conv, CONV,
        "resume_idea must sync the rotated conversation UUID onto idea A when another live pane holds {CONV}"
    );
    let stored_a = store.get_idea(&idea_a.id).unwrap().unwrap();
    assert_eq!(
        stored_a
            .orchestrator
            .as_ref()
            .and_then(|o| o.conversation.as_deref()),
        Some(new_conv)
    );
    let panes_after = layout.panes().unwrap();
    let new_pane_a = panes_after
        .iter()
        .find(|p| p.kind == "agent" && p.idea == idea_a.id)
        .expect("new pane A launched");
    let argv_new_a_raw = layout
        .client
        .run_checked(&[
            "display-message",
            "-p",
            "-t",
            &new_pane_a.id,
            "#{@tandem_argv}",
        ])
        .unwrap();
    let argv_new_a: Vec<String> = serde_json::from_str(argv_new_a_raw.trim()).unwrap();
    assert_eq!(
        &argv_new_a[argv_new_a.len() - 2..],
        &["--session-id", new_conv]
    );

    layout
        .client
        .run_checked(&[
            "respawn-pane",
            "-k",
            "-t",
            &new_pane_a.id,
            "sh",
            "-c",
            "exit 1",
        ])
        .unwrap();
    for _ in 0..40 {
        if layout
            .panes()
            .unwrap()
            .iter()
            .any(|p| p.id == new_pane_a.id && p.dead)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let revived_a = resume_idea(&ws.root, &cfg, &idea_a.id).expect("revive dead idea A pane");
    let revived_conv = revived_a
        .orchestrator
        .as_ref()
        .and_then(|o| o.conversation.as_deref())
        .expect("revived dead pane stores rotated conversation");
    assert_ne!(
        revived_conv, new_conv,
        "reviving a dead unresumable pane must rotate to a fresh --session-id instead of reusing {new_conv}"
    );
}
