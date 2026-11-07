mod common;

use common::TempDir;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tandem::commands;
use tandem::config::Config;
use tandem::coordination::store::CoordStore;
use tandem::tmux::client::{Result as ClientResult, SubprocessRunner, TmuxClient, TmuxRunner};
use tandem::tmux::keys::{self, bind_commands, default_keymap};
use tandem::tmux::layout::{Launch, Layout, SESSION, conf_text};
use tandem::tmux::tabs::{agents_in, visible_pane};

fn tmux_available() -> bool {
    common::require_tool("tmux")
}

struct TestContext {
    socket: String,
    temp_dir: TempDir,
    layout: Layout,
    #[allow(dead_code)]
    bin_dir: PathBuf,
}

impl TestContext {
    fn new() -> Option<Self> {
        if !tmux_available() {
            return None;
        }
        let temp_dir = TempDir::new();
        let socket = format!(
            "tandem-test-{}-{}",
            std::process::id(),
            &uuid::Uuid::new_v4().to_string()[..8]
        );
        let bin_dir = temp_dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).expect("create bin dir");
        for stub in ["claude", "agy"] {
            let stub_path = bin_dir.join(stub);
            std::fs::write(
                &stub_path,
                "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo \"1.0.0\"; exit 0; fi\nexec sleep 60\n",
            )
            .expect("write stub");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&stub_path).expect("meta").permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&stub_path, perms).expect("chmod");
            }
        }
        let client = TmuxClient::new(&socket, None);
        let coord = CoordStore::new(temp_dir.path().join("coord"));
        let mut lay = Layout::new(&client);
        lay.coord = Some(coord);
        let conf_path = temp_dir.path().join("tandem.conf");
        let conf = lay.write_conf(&conf_path).expect("write_conf");
        let client_with_conf = TmuxClient::new(&socket, Some(conf));
        lay = Layout::new(&client_with_conf);
        lay.coord = Some(CoordStore::new(temp_dir.path().join("coord")));

        Some(Self {
            socket,
            temp_dir,
            layout: lay,
            bin_dir,
        })
    }
}

impl Drop for TestContext {
    fn drop(&mut self) {
        let _ = std::process::Command::new("tmux")
            .args(["-L", &self.socket, "kill-server"])
            .output();
    }
}

fn sleeper(role: &str, index: usize, kind: &str, label: &str) -> Launch {
    let lbl = if label.is_empty() {
        format!("fake·{}{}", role, index)
    } else {
        label.to_string()
    };
    let mut env = HashMap::new();
    env.insert(
        "TANDEM_SESSION_ID".to_string(),
        format!("{}-{}", role, index),
    );
    let session_id = if kind == "agent" {
        format!("{}-{}", role, index)
    } else {
        String::new()
    };
    let mut launch = Launch::new(
        vec!["sleep".to_string(), "60".to_string()],
        role,
        index,
        &lbl,
    );
    launch.env = env;
    launch.session_id = session_id;
    launch.kind = kind.to_string();
    launch
}

fn spec_launch(role: &str, index: usize, cli: &str, model: &str, cwd: Option<&str>) -> Launch {
    let mut launch = sleeper(role, index, "agent", &format!("{cli}·{model}"));
    let spec = serde_json::json!({
        "cli": cli,
        "model": model,
        "skip_permissions": false,
        "cwd": cwd,
    });
    launch.spec = spec.to_string();
    launch
}

const CONV: &str = "46f2fc12-7f1f-4ca2-b714-06cbbfd669e9";
const LIVE: &str = "0b9a3c1e-1111-4222-8333-444455556666";

fn conv_launch(role: &str, index: usize, cli: &str, conversation: Option<&str>) -> Launch {
    let mut launch = sleeper(role, index, "agent", &format!("{cli}·m"));
    let spec = serde_json::json!({
        "cli": cli,
        "model": "m",
        "skip_permissions": false,
        "cwd": None::<String>,
        "conversation": conversation,
    });
    launch.spec = spec.to_string();
    launch
}

fn built(ctx: &TestContext) -> HashMap<String, String> {
    ctx.layout
        .build(
            160,
            40,
            &sleeper("planner", 1, "agent", ""),
            &sleeper("implementer", 1, "agent", ""),
            &sleeper("status", 0, "own", ""),
        )
        .expect("build layout")
}

#[test]
fn test_build_creates_columns_status_and_tags() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    let ids = built(&ctx);
    let panes = ctx.layout.panes().unwrap();
    let pane_ids: std::collections::HashSet<String> = panes.iter().map(|p| p.id.clone()).collect();
    for v in ids.values() {
        assert!(pane_ids.contains(v));
    }
    let sb = visible_pane(&panes, "sidebar").unwrap();
    let main = visible_pane(&panes, "planner").unwrap();
    assert_eq!(sb.id, ids["sidebar"]);
    assert_eq!(main.id, ids["main"]);

    let out = ctx
        .layout
        .client
        .run_checked(&[
            "list-panes",
            "-t",
            &format!("{SESSION}:main"),
            "-F",
            "#{pane_id} #{pane_left} #{pane_top}",
        ])
        .unwrap();
    let mut pos: HashMap<String, (i32, i32)> = HashMap::new();
    for line in out.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            pos.insert(
                parts[0].to_string(),
                (parts[1].parse().unwrap(), parts[2].parse().unwrap()),
            );
        }
    }
    assert!(pos[&ids["sidebar"]].0 < pos[&ids["main"]].0);
    assert!(pos[&ids["status"]].1 > pos[&ids["main"]].1);
}

#[test]
fn test_border_title_lists_tabs() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    let ids = built(&ctx);
    let fmt = ctx
        .layout
        .client
        .run_checked(&[
            "show-options",
            "-p",
            "-t",
            &ids["main"],
            "pane-border-format",
        ])
        .unwrap();
    assert!(fmt.contains("planner-1 · fake"));
}

#[test]
fn test_options_mouse_and_hint_bar() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    let mouse = ctx
        .layout
        .client
        .run_checked(&["show-options", "-g", "mouse"])
        .unwrap();
    assert!(mouse.contains("on"));
    let ext = ctx
        .layout
        .client
        .run_checked(&["show-options", "-g", "extended-keys"])
        .unwrap();
    assert!(ext.contains("on"));
    let platform = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    let status_left = ctx
        .layout
        .client
        .run_checked(&["show-options", "-g", "status-left"])
        .unwrap();
    assert!(status_left.contains(&format!("{}+h Help", keys::alt_name(platform))));
}

#[test]
fn test_tmux_root_bindings_single_and_multi_modifier() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    let platform = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    let km = default_keymap();
    let cmds = bind_commands(None, None, platform, &km);
    for cmd in cmds {
        let str_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
        ctx.layout.client.run_checked(&str_refs).unwrap();
    }
    let output = std::process::Command::new("tmux")
        .args(["-L", &ctx.socket, "list-keys", "-T", "root"])
        .output()
        .expect("list-keys");
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("M-h"));
    assert!(out.contains("C-M-n") || out.contains("M-C-n"));
    if cfg!(target_os = "macos") {
        assert!(out.contains("˙"));
    }
}

#[test]
fn test_add_agent_swaps_in_and_tab_navigation() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    let new = ctx
        .layout
        .add_agent(&sleeper("planner", 2, "agent", ""), true)
        .unwrap();
    let panes = ctx.layout.panes().unwrap();
    assert_eq!(visible_pane(&panes, "planner").unwrap().id, new);
    assert_eq!(agents_in(&panes, "planner").len(), 2);
    ctx.layout.step("planner", -1).unwrap();
    assert_eq!(
        visible_pane(&ctx.layout.panes().unwrap(), "planner")
            .unwrap()
            .index,
        1
    );
    ctx.layout.select("planner", 2).unwrap();
    assert_eq!(
        visible_pane(&ctx.layout.panes().unwrap(), "planner")
            .unwrap()
            .index,
        2
    );
    let fmt = ctx
        .layout
        .client
        .run_checked(&["show-options", "-p", "-t", &new, "pane-border-format"])
        .unwrap();
    assert!(fmt.contains("planner-2 · fake"));
}

#[test]
fn test_add_agent_records_session_with_pane() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    let new = ctx
        .layout
        .add_agent(&sleeper("implementer", 2, "agent", ""), true)
        .unwrap();
    assert!(ctx.layout.coord.is_some());
    let sessions = ctx.layout.coord.as_ref().unwrap().sessions();
    let map: HashMap<String, String> = sessions.into_iter().map(|s| (s.id, s.pane)).collect();
    assert_eq!(
        map.get("implementer-2").map(|s| s.as_str()),
        Some(new.as_str())
    );
}

#[test]
fn test_close_visible_agent_shows_the_other_tab() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    ctx.layout
        .add_agent(&sleeper("planner", 2, "agent", ""), true)
        .unwrap();
    let panes = ctx.layout.panes().unwrap();
    let first = agents_in(&panes, "planner")
        .into_iter()
        .find(|p| p.index == 2)
        .unwrap();
    ctx.layout
        .close_agent(&first.id, &sleeper("planner", 0, "own", "placeholder"))
        .unwrap();
    let panes = ctx.layout.panes().unwrap();
    let indexes: Vec<usize> = agents_in(&panes, "planner")
        .into_iter()
        .map(|p| p.index)
        .collect();
    assert_eq!(indexes, vec![1]);
    assert_eq!(visible_pane(&panes, "planner").unwrap().index, 1);
    let sessions = ctx.layout.coord.as_ref().unwrap().sessions();
    assert!(!sessions.iter().any(|s| s.id == "planner-2"));
}

#[test]
fn test_closing_last_agent_leaves_placeholder() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    let ids = built(&ctx);
    ctx.layout
        .close_agent(&ids["left"], &sleeper("planner", 0, "own", "placeholder"))
        .unwrap();
    ctx.layout
        .close_agent(&ids["right"], &sleeper("agent", 0, "own", "placeholder"))
        .unwrap();
    let panes = ctx.layout.panes().unwrap();
    let shown = panes
        .iter()
        .find(|p| p.window == "main" && p.role != "sidebar" && p.role != "status")
        .unwrap();
    assert_eq!(shown.kind, "own");
    let fmt = ctx
        .layout
        .client
        .run_checked(&["show-options", "-p", "-t", &shown.id, "pane-border-format"])
        .unwrap();
    assert!(fmt.contains("No agent"));
}

#[test]
fn test_adding_to_empty_column_replaces_placeholder() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    let ids = built(&ctx);
    ctx.layout
        .close_agent(&ids["left"], &sleeper("planner", 0, "own", "placeholder"))
        .unwrap();
    ctx.layout
        .close_agent(&ids["right"], &sleeper("agent", 0, "own", "placeholder"))
        .unwrap();
    let new = ctx
        .layout
        .add_agent(&sleeper("planner", 1, "agent", ""), true)
        .unwrap();
    let panes = ctx.layout.panes().unwrap();
    assert_eq!(visible_pane(&panes, "planner").unwrap().id, new);
    assert!(!panes.iter().any(|p| p.kind == "own" && p.role == "planner"));
}

#[test]
fn test_exited_agent_stays_and_shows_exit_code() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    let mut launch = Launch::new(
        vec!["sh".into(), "-c".into(), "exit 3".into()],
        "planner",
        2,
        "boom",
    );
    launch.session_id = "planner-2".to_string();
    ctx.layout.add_agent(&launch, true).unwrap();

    let mut dead_panes = Vec::new();
    for _ in 0..40 {
        let panes = ctx.layout.panes().unwrap();
        dead_panes = panes.into_iter().filter(|p| p.dead).collect();
        if !dead_panes.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    ctx.layout.refresh().unwrap();
    assert!(!dead_panes.is_empty());
    assert_eq!(dead_panes[0].dead_status, Some(3));
    let panes = ctx.layout.panes().unwrap();
    let vis = visible_pane(&panes, "planner").unwrap();
    let fmt = ctx
        .layout
        .client
        .run_checked(&["show-options", "-p", "-t", &vis.id, "pane-border-format"])
        .unwrap();
    assert!(fmt.contains("exited 3"));
}

#[test]
fn test_restart_respawns_dead_pane() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    let mut launch = Launch::new(
        vec!["sh".into(), "-c".into(), "exit 1".into()],
        "planner",
        2,
        "boom",
    );
    launch.session_id = "planner-2".to_string();
    let new = ctx.layout.add_agent(&launch, true).unwrap();
    for _ in 0..40 {
        let panes = ctx.layout.panes().unwrap();
        if panes.iter().any(|p| p.dead) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    ctx.layout.restart(&new, "/tmp/coord", None).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));
    let panes = ctx.layout.panes().unwrap();
    assert!(panes.iter().any(|p| p.id == new));
}

#[test]
fn test_reflow_preserves_layout() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    let _ids = built(&ctx);
    assert!(ctx.layout.reflow(80, 40).is_ok());
    assert!(ctx.layout.reflow(160, 40).is_ok());
}

#[test]
fn test_reset_restores_default_dimensions_after_manual_resize() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    let ids = built(&ctx);
    let width = |pane: &str| -> i32 {
        ctx.layout
            .client
            .run_checked(&["display-message", "-p", "-t", pane, "#{pane_width}"])
            .unwrap()
            .trim()
            .parse()
            .unwrap()
    };
    ctx.layout
        .client
        .run_checked(&["resize-pane", "-t", &ids["sidebar"], "-x", "40"])
        .unwrap();
    assert_eq!(width(&ids["sidebar"]), 40);
    ctx.layout.reset().unwrap();
    assert_eq!(width(&ids["sidebar"]), 26);
}

#[test]
fn test_reflow_clamps_a_status_pane_that_is_too_tall() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    let ids = built(&ctx);
    ctx.layout
        .client
        .run_checked(&["resize-pane", "-t", &ids["status"], "-y", "20"])
        .unwrap();
    ctx.layout.reflow(200, 30).unwrap();
    let h: i32 = ctx
        .layout
        .client
        .run_checked(&[
            "display-message",
            "-p",
            "-t",
            &ids["status"],
            "#{pane_height}",
        ])
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(h <= 12);
}

#[test]
fn test_focus_selects_column_pane() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    let ids = built(&ctx);
    ctx.layout.focus("implementer").unwrap();
    let active = ctx
        .layout
        .client
        .run_checked(&["display-message", "-p", "-t", SESSION, "#{pane_id}"])
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(active, ids["right"]);
}

#[test]
fn test_user_tmux_conf_is_not_loaded() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    assert!(
        ctx.layout
            .client
            .conf
            .as_ref()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("tandem.conf")
    );
}

#[test]
fn test_first_run_hook_registered() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    let hooks = ctx
        .layout
        .client
        .run_checked(&["show-options", "-g", "client-attached"])
        .unwrap();
    assert!(hooks.contains("first-run"));
}

#[test]
fn test_capture_state_records_lineup_active_tab_and_sizes() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    ctx.layout
        .build(
            160,
            40,
            &spec_launch("planner", 1, "claude", "sonnet", None),
            &spec_launch("implementer", 1, "agy", "gemini-3", None),
            &sleeper("status", 0, "own", ""),
        )
        .unwrap();
    ctx.layout
        .add_agent(
            &spec_launch("planner", 2, "claude", "opus", Some("/tmp")),
            true,
        )
        .unwrap();
    ctx.layout.select("planner", 1).unwrap();
    let panes = ctx.layout.panes().unwrap();
    let sb = visible_pane(&panes, "sidebar").unwrap();
    ctx.layout
        .client
        .run_checked(&["resize-pane", "-t", &sb.id, "-x", "30"])
        .unwrap();

    let snap = commands::capture_state(&ctx.layout, true).unwrap();
    let agent_tuples: Vec<(&str, &str, &str, usize, Option<&str>)> = snap
        .agents
        .iter()
        .map(|a| {
            (
                a.cli.as_str(),
                a.role.as_str(),
                a.model.as_str(),
                a.index,
                a.cwd.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        agent_tuples,
        vec![
            ("claude", "planner", "sonnet", 1, None),
            ("claude", "planner", "opus", 2, Some("/tmp")),
            ("agy", "implementer", "gemini-3", 1, None),
        ]
    );
    assert_eq!(snap.active.get("planner"), Some(&1));
    assert_eq!(snap.active.get("implementer"), None);
    let col_pct = snap.columns_percent.unwrap();
    assert_eq!(col_pct, 30);
    assert_eq!(snap.ui.sidebar_cols, 30);
    let st_rows = snap.status_rows.unwrap();
    assert!(st_rows >= 3);
}

#[test]
fn test_apply_sizes_restores_proportions() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    let ids = built(&ctx);
    ctx.layout.apply_sizes(Some(30), Some(8)).unwrap();
    let (percent, rows) = ctx.layout.sizes().unwrap();
    let pct = percent.unwrap();
    assert!((28..=32).contains(&pct));
    assert_eq!(rows, Some(8));
    assert!(!ids["left"].is_empty());
}

#[test]
fn test_save_exit_writes_state_then_ends_session() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    ctx.layout
        .build(
            160,
            40,
            &spec_launch("planner", 1, "claude", "sonnet", None),
            &spec_launch("implementer", 1, "claude", "sonnet", None),
            &sleeper("status", 0, "own", ""),
        )
        .unwrap();
    commands::save_exit(&ctx.layout, ctx.temp_dir.path());
    let loaded = tandem::state::load(&tandem::state::state_path(ctx.temp_dir.path()));
    assert!(loaded.is_some());
    assert_eq!(loaded.unwrap().agents.len(), 2);
    assert!(!ctx.layout.exists());
}

#[test]
fn test_save_exit_keeps_session_when_there_is_nothing_to_save() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    commands::save_exit(&ctx.layout, ctx.temp_dir.path());
    assert!(ctx.layout.exists());
    assert!(!tandem::state::state_path(ctx.temp_dir.path()).exists());
}

#[test]
fn test_respawn_own_restarts_only_tandem_panes() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    let ids = built(&ctx);
    let before = ctx
        .layout
        .client
        .run_checked(&["display-message", "-p", "-t", &ids["status"], "#{pane_pid}"])
        .unwrap()
        .trim()
        .to_string();
    let mut launches = HashMap::new();
    launches.insert("status".to_string(), sleeper("status", 0, "own", "status"));
    ctx.layout.respawn_own(&launches).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    let after = ctx
        .layout
        .client
        .run_checked(&["display-message", "-p", "-t", &ids["status"], "#{pane_pid}"])
        .unwrap()
        .trim()
        .to_string();
    assert_ne!(before, after);
    let cmd = ctx
        .layout
        .client
        .run_checked(&[
            "display-message",
            "-p",
            "-t",
            &ids["left"],
            "#{pane_current_command}",
        ])
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(cmd, "sleep");
}

#[test]
fn test_second_status_row_runs_the_quota_line() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    let status_opt = ctx
        .layout
        .client
        .run_checked(&["show-options", "-g", "status"])
        .unwrap();
    assert_eq!(status_opt.split_whitespace().last().unwrap(), "2");
    let fmt1 = ctx
        .layout
        .client
        .run_checked(&["show-options", "-g", "status-format[1]"])
        .unwrap();
    assert!(fmt1.contains("quota-line"));
}

#[test]
fn test_two_workspaces_run_side_by_side_without_sharing_a_session() {
    if !tmux_available() {
        return;
    }
    let temp_dir = TempDir::new();
    let a = temp_dir.path().join("a");
    let b = temp_dir.path().join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let mut env_map = HashMap::new();
    env_map.insert(
        "XDG_STATE_HOME".to_string(),
        temp_dir.path().join("state").to_str().unwrap().to_string(),
    );
    let config = Config::from_env_source(&env_map).unwrap();
    let la = commands::make_layout(&a, &config);
    let lb = commands::make_layout(&b, &config);

    struct SocketGuard(Vec<String>);
    impl Drop for SocketGuard {
        fn drop(&mut self) {
            for sock in &self.0 {
                let _ = std::process::Command::new("tmux")
                    .args(["-L", sock, "kill-server"])
                    .output();
            }
        }
    }
    let _guard = SocketGuard(vec![la.client.socket.clone(), lb.client.socket.clone()]);

    assert!(!la.exists() && !lb.exists());
    la.build(
        160,
        40,
        &sleeper("planner", 1, "agent", ""),
        &sleeper("implementer", 1, "agent", ""),
        &sleeper("status", 0, "own", ""),
    )
    .unwrap();
    assert!(la.exists() && !lb.exists());
    lb.build(
        160,
        40,
        &sleeper("planner", 1, "agent", ""),
        &sleeper("implementer", 1, "agent", ""),
        &sleeper("status", 0, "own", ""),
    )
    .unwrap();
    assert!(la.exists() && lb.exists());
    lb.client
        .run_checked(&["kill-session", "-t", SESSION])
        .unwrap();
    assert!(la.exists() && !lb.exists());
}

#[test]
fn test_own_panes_are_pinned_to_their_workspace() {
    let temp_dir = TempDir::new();
    let sl = commands::status_launch(None, Some(temp_dir.path()));
    assert_eq!(
        sl.env.get("TANDEM_WORKSPACE").map(|s| s.as_str()),
        Some(temp_dir.path().to_str().unwrap())
    );
    let pl = commands::placeholder_launch("planner", None, Some(temp_dir.path()));
    assert_eq!(
        pl.env.get("TANDEM_WORKSPACE").map(|s| s.as_str()),
        Some(temp_dir.path().to_str().unwrap())
    );
}

#[test]
fn test_status_pane_border_has_a_title_that_is_not_the_hostname() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    let ids = built(&ctx);
    let fmt = ctx
        .layout
        .client
        .run_checked(&[
            "show-options",
            "-p",
            "-t",
            &ids["status"],
            "pane-border-format",
        ])
        .unwrap();
    assert!(fmt.contains("STATUS") && !fmt.contains("pane_title"));
}

#[test]
fn test_registered_session_records_the_model_not_the_label() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    let mut launch = sleeper("planner", 1, "agent", "claude·opus");
    launch.session_id = "planner-1".to_string();
    launch.spec = serde_json::json!({
        "cli": "claude",
        "model": "opus",
        "skip_permissions": false,
        "cwd": None::<String>,
    })
    .to_string();
    ctx.layout
        .build(
            160,
            40,
            &launch,
            &sleeper("implementer", 1, "agent", ""),
            &sleeper("status", 0, "own", ""),
        )
        .unwrap();
    let sessions = ctx.layout.coord.as_ref().unwrap().sessions();
    let session_map: HashMap<String, String> =
        sessions.into_iter().map(|s| (s.id, s.model)).collect();
    assert_eq!(
        session_map.get("planner-1").map(|s| s.as_str()),
        Some("opus")
    );
}

#[test]
fn test_capture_state_saves_each_agents_current_conversation() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    let coord = ctx.temp_dir.path().join("coord");
    ctx.layout
        .build(
            160,
            40,
            &conv_launch("planner", 1, "claude", Some(CONV)),
            &conv_launch("implementer", 1, "agy", None),
            &sleeper("status", 0, "own", ""),
        )
        .unwrap();
    ctx.layout
        .add_agent(&conv_launch("planner", 2, "claude", Some(CONV)), true)
        .unwrap();
    tandem::conversations::record(&coord, "planner-2", LIVE).unwrap();
    let log = tandem::conversations::agy_log_path(&coord, "implementer-1").unwrap();
    if let Some(p) = log.parent() {
        std::fs::create_dir_all(p).unwrap();
    }
    std::fs::write(&log, format!("Created conversation {CONV}\n")).unwrap();
    let snap = commands::capture_state(&ctx.layout, false).unwrap();
    let items: Vec<(&str, usize, Option<&str>)> = snap
        .agents
        .iter()
        .map(|a| (a.role.as_str(), a.index, a.conversation.as_deref()))
        .collect();
    assert_eq!(
        items,
        vec![
            ("planner", 1, Some(CONV)),
            ("planner", 2, Some(LIVE)),
            ("implementer", 1, Some(CONV)),
        ]
    );
    assert!(!snap.clean);
}

#[test]
fn test_autosave_writes_unclean_state_and_stops_once_closing() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    ctx.layout
        .build(
            160,
            40,
            &conv_launch("planner", 1, "claude", Some(CONV)),
            &sleeper("implementer", 0, "own", ""),
            &sleeper("status", 0, "own", ""),
        )
        .unwrap();
    let ws = ctx.temp_dir.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    assert!(commands::autosave(&ctx.layout, &ws));
    let saved = tandem::state::load(&tandem::state::state_path(&ws)).unwrap();
    assert!(!saved.clean);
    assert_eq!(saved.agents[0].conversation.as_deref(), Some(CONV));
    let _ = std::fs::remove_file(tandem::state::state_path(&ws));
    ctx.layout
        .client
        .run_checked(&["set-option", "-t", SESSION, commands::CLOSING, "1"])
        .unwrap();
    assert!(!commands::autosave(&ctx.layout, &ws));
    assert!(!tandem::state::state_path(&ws).exists());
}

#[test]
fn test_discard_exit_removes_saved_state_and_ends_session() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    ctx.layout
        .build(
            160,
            40,
            &conv_launch("planner", 1, "claude", Some(CONV)),
            &sleeper("implementer", 0, "own", ""),
            &sleeper("status", 0, "own", ""),
        )
        .unwrap();
    let ws = ctx.temp_dir.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    assert!(commands::autosave(&ctx.layout, &ws));
    commands::discard_exit(&ctx.layout, &ws);
    assert!(!tandem::state::state_path(&ws).exists());
    assert!(!ctx.layout.exists());
}

#[test]
fn test_save_exit_saves_clean_state() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    ctx.layout
        .build(
            160,
            40,
            &conv_launch("planner", 1, "claude", Some(CONV)),
            &sleeper("implementer", 0, "own", ""),
            &sleeper("status", 0, "own", ""),
        )
        .unwrap();
    let ws = ctx.temp_dir.path().join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    commands::save_exit(&ctx.layout, &ws);
    let saved = tandem::state::load(&tandem::state::state_path(&ws)).unwrap();
    assert!(saved.clean);
    assert!(!ctx.layout.exists());
}

#[test]
fn test_restart_with_launch_retags_the_pane() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    let mut launch = Launch::new(
        vec!["sh".into(), "-c".into(), "exit 1".into()],
        "planner",
        2,
        "boom",
    );
    launch.session_id = "planner-2".to_string();
    let new = ctx.layout.add_agent(&launch, true).unwrap();
    for _ in 0..40 {
        let panes = ctx.layout.panes().unwrap();
        if panes.iter().any(|p| p.dead) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let relaunch = conv_launch("planner", 2, "claude", Some(CONV));
    ctx.layout
        .restart(&new, "/tmp/coord", Some(&relaunch))
        .unwrap();
    let panes = ctx.layout.panes().unwrap();
    let pane = panes.iter().find(|p| p.id == new).unwrap();
    assert!(!pane.dead);
    let parsed: serde_json::Value = serde_json::from_str(&pane.spec).unwrap();
    assert_eq!(parsed["conversation"].as_str(), Some(CONV));
}

#[test]
fn test_conf_text_unsets_tmux_pane() {
    let text = conf_text(50000);
    assert!(text.contains("set-environment -g -u TMUX_PANE"));
}

#[test]
fn test_mouse_bindings_applied_after_apply_chrome() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    let output = std::process::Command::new("tmux")
        .args(["-L", &ctx.socket, "list-keys", "-T", "root"])
        .output()
        .expect("list-keys");
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("MouseDown1Status"));
    assert!(out.contains("MouseDown3Pane"));
}

#[test]
fn test_unsafe_mouse_defaults_unbound_after_apply_chrome() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    let output = std::process::Command::new("tmux")
        .args(["-L", &ctx.socket, "list-keys", "-T", "root"])
        .output()
        .expect("list-keys");
    let out = String::from_utf8_lossy(&output.stdout);
    for unsafe_key in [
        "MouseDown3Status",
        "MouseDown3StatusLeft",
        "M-MouseDown3Pane",
    ] {
        let has_exact = out.lines().any(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            parts.get(1) == Some(&unsafe_key)
        });
        assert!(
            !has_exact,
            "{unsafe_key} should have been unbound; still present in list-keys"
        );
    }
}

#[test]
fn test_safe_tmux_mouse_defaults_still_present() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    let output = std::process::Command::new("tmux")
        .args(["-L", &ctx.socket, "list-keys", "-T", "root"])
        .output()
        .expect("list-keys");
    let out = String::from_utf8_lossy(&output.stdout);
    for safe_key in ["MouseDrag1Border", "MouseDown1Pane", "WheelUpPane"] {
        assert!(
            out.contains(safe_key),
            "{safe_key} default should still be bound"
        );
    }
}

#[test]
fn test_border_click_binding_added_after_apply_chrome() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    let output = std::process::Command::new("tmux")
        .args(["-L", &ctx.socket, "list-keys", "-T", "root"])
        .output()
        .expect("list-keys");
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("MouseDown1Border"));
}

#[test]
fn test_drag_resize_still_bound_after_border_click_binding() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    let output = std::process::Command::new("tmux")
        .args(["-L", &ctx.socket, "list-keys", "-T", "root"])
        .output()
        .expect("list-keys");
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("MouseDrag1Border"));
}

#[test]
fn test_click_tab_select_switches_visible_agent() {
    let mut ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    let ids = built(&ctx);
    ctx.layout
        .add_agent(&sleeper("planner", 2, "agent", ""), true)
        .unwrap();
    let panes = ctx.layout.panes().unwrap();
    let current = visible_pane(&panes, "planner").unwrap();
    assert_eq!(current.index, 2);

    let config = Config::from_env_source(&HashMap::new()).unwrap();
    commands::click_tab(&mut ctx.layout, &config, &current.id, &ids["left"]);

    let panes2 = ctx.layout.panes().unwrap();
    let shown = visible_pane(&panes2, "planner").unwrap();
    assert_eq!(shown.index, 1);
}

#[test]
fn test_bound_border_click_command_line_is_accepted_by_the_cli() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    let exe = env!("CARGO_BIN_EXE_tandem");
    let platform = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    let km = default_keymap();
    let bound = keys::mouse_commands(
        Some(&[exe]),
        Some(ctx.temp_dir.path().to_str().unwrap()),
        platform,
        &km,
    )
    .into_iter()
    .find(|c| c.len() >= 4 && c[0] == "bind-key" && c[1] == "-n" && c[2] == "DoubleClick1Border")
    .expect("DoubleClick1Border binding");
    let bound_cmd = &bound[3];
    let marker = "equalize ";
    let idx = bound_cmd.find(marker).expect("find equalize");
    let template = bound_cmd[idx + marker.len()..].trim_end_matches('"');
    let pane = ctx.layout.panes().unwrap()[0].id.clone();
    let expanded = ctx
        .layout
        .client
        .run_checked(&["display-message", "-p", "-t", &pane, template])
        .unwrap();
    let expanded_trimmed = expanded.trim();

    let output = std::process::Command::new(exe)
        .args(std::iter::once("equalize").chain(expanded_trimmed.split_whitespace()))
        .output()
        .expect("run equalize");
    assert_ne!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("usage:"), "stderr: {}", stderr);
}

#[test]
fn test_restart_fallback_uses_active_pane_when_env_pane_empty() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    let mut launch = Launch::new(
        vec!["sh".into(), "-c".into(), "exit 1".into()],
        "planner",
        2,
        "boom",
    );
    launch.session_id = "planner-2".to_string();
    let new = ctx.layout.add_agent(&launch, true).unwrap();
    for _ in 0..40 {
        let panes = ctx.layout.panes().unwrap();
        if panes.iter().any(|p| p.dead) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    ctx.layout
        .client
        .run_checked(&["select-pane", "-t", &new])
        .unwrap();
    let cfg = Config::from_env_source(&HashMap::new()).unwrap();
    assert!(cfg.tmux_pane.is_empty());
    let pane_id = if !cfg.tmux_pane.is_empty() {
        cfg.tmux_pane
    } else {
        ctx.layout
            .client
            .run_checked(&["display-message", "-p", "#{pane_id}"])
            .unwrap()
            .trim()
            .to_string()
    };
    assert_eq!(pane_id, new);
}

const ATTACH_PY_SCRIPT: &str = r#"
import pty, os, sys, subprocess, fcntl, termios, struct, time, select, re

socket = sys.argv[1]
session = sys.argv[2]
ready_file = sys.argv[3]
out_file = sys.argv[4]

master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 160, 0, 0))
proc = subprocess.Popen(
    ["tmux", "-L", socket, "attach", "-t", session],
    stdin=slave, stdout=slave, stderr=slave, env=dict(os.environ, TERM="xterm-256color")
)
os.close(slave)
try:
    for _ in range(50):
        listed = subprocess.run(["tmux", "-L", socket, "list-clients"], capture_output=True, text=True)
        if listed.stdout.strip():
            break
        time.sleep(0.1)
    with open(ready_file, "w") as f:
        f.write("ready")

    seen = b""
    end = time.time() + 5.0
    while time.time() < end:
        r, _, _ = select.select([master], [], [], 0.1)
        if r:
            seen += os.read(master, 65536)
        if b"\xe2\x96\xb6 2" in seen:
            break
    os.write(master, b"\x1b")
    time.sleep(0.3)
    r, _, _ = select.select([master], [], [], 0.2)
    if r:
        seen += os.read(master, 65536)
finally:
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except Exception:
        proc.kill()
    os.close(master)

text = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b[()][A-Z0-9]|\x1b[=>]", "", seen.decode("utf-8", "replace"))
with open(out_file, "w", encoding="utf-8") as f:
    f.write(text)
"#;

#[test]
fn test_click_tab_menu_renders_on_an_attached_client() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    built(&ctx);
    ctx.layout
        .add_agent(&sleeper("planner", 2, "agent", ""), true)
        .unwrap();
    let panes = ctx.layout.panes().unwrap();
    let current = visible_pane(&panes, "planner").unwrap();

    let ready_file = ctx.temp_dir.path().join("ready.txt");
    let out_file = ctx.temp_dir.path().join("captured.txt");
    let script_path = ctx.temp_dir.path().join("attach.py");
    std::fs::write(&script_path, ATTACH_PY_SCRIPT).unwrap();

    let mut child = std::process::Command::new("python3")
        .args([
            script_path.to_str().unwrap(),
            &ctx.socket,
            SESSION,
            ready_file.to_str().unwrap(),
            out_file.to_str().unwrap(),
        ])
        .spawn()
        .expect("spawn attach script");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(6);
    while std::time::Instant::now() < deadline && !ready_file.exists() {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(ready_file.exists(), "tmux client did not attach");

    let mut layout_clone = ctx.layout.clone();
    let current_id = current.id.clone();
    let opener = std::thread::spawn(move || {
        let config = Config::from_env_source(&HashMap::new()).unwrap();
        commands::click_tab(&mut layout_clone, &config, &current_id, "");
    });

    let _ = child.wait();
    let _ = opener.join();

    let text = std::fs::read_to_string(&out_file).unwrap_or_default();
    assert!(
        text.contains("▶ 2") && text.contains("1") && text.contains("New agent"),
        "Screen content:\n{text}"
    );
}

struct SpyRunner {
    inner: SubprocessRunner,
    captured: Mutex<Vec<Vec<String>>>,
}

impl TmuxRunner for SpyRunner {
    fn run(&self, argv: &[String]) -> ClientResult {
        let is_display_menu = argv.iter().any(|a| a == "display-menu");
        if is_display_menu {
            let mut l = self.captured.lock().unwrap();
            l.push(argv.to_vec());
            return ClientResult {
                code: 0,
                out: String::new(),
                err: String::new(),
            };
        }
        self.inner.run(argv)
    }
}

#[test]
fn test_click_tab_menu_item_commands_are_valid_tmux_commands() {
    let mut ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    let ids = built(&ctx);
    ctx.layout
        .add_agent(&sleeper("planner", 2, "agent", ""), true)
        .unwrap();
    let panes = ctx.layout.panes().unwrap();
    let current = visible_pane(&panes, "planner").unwrap();

    let record = ctx.temp_dir.path().join("item_args.txt");
    let spy_script = ctx.temp_dir.path().join("spy_launcher.sh");
    std::fs::write(
        &spy_script,
        format!("#!/bin/sh\necho \"$@\" >> \"{}\"\n", record.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&spy_script).expect("meta").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&spy_script, perms).expect("chmod");
    }

    ctx.layout.self_cmd = Some(vec![spy_script.to_str().unwrap().to_string()]);

    let spy_runner = Arc::new(SpyRunner {
        inner: SubprocessRunner,
        captured: Mutex::new(Vec::new()),
    });
    let spy_runner_clone = spy_runner.clone();

    let client_with_spy = TmuxClient::with_runner(
        ctx.socket.clone(),
        ctx.layout.client.conf.clone(),
        spy_runner,
    );
    ctx.layout.client = client_with_spy;

    let config = Config::from_env_source(&HashMap::new()).unwrap();
    commands::click_tab(&mut ctx.layout, &config, &current.id, "");

    let captured_calls = spy_runner_clone.captured.lock().unwrap();
    let menu = captured_calls
        .iter()
        .find(|a| a.iter().any(|arg| arg == "display-menu"))
        .expect("display-menu called");

    let top_idx = menu.iter().position(|a| a == "#{pane_top}").unwrap();
    let menu_items = &menu[top_idx + 1..];
    let mut item_commands = Vec::new();
    let mut i = 2;
    while i < menu_items.len() {
        if menu_items[i].starts_with("run-shell") {
            item_commands.push(menu_items[i].clone());
        }
        i += 3;
    }
    assert_eq!(item_commands.len(), 2);

    for cmd in &item_commands {
        let status = std::process::Command::new("tmux")
            .args(["-L", &ctx.socket, "if-shell", "-F", "1", cmd])
            .output()
            .expect("if-shell");
        assert!(
            status.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if record.exists() {
            let content = std::fs::read_to_string(&record).unwrap();
            if content.lines().count() >= 2 {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    let mut actual_lines: Vec<String> = std::fs::read_to_string(&record)
        .unwrap()
        .lines()
        .map(|s| s.trim().to_string())
        .collect();
    actual_lines.sort();
    let mut expected_lines = vec![
        format!("click-tab {} --select {}", ids["left"], ids["left"]),
        format!("click-tab {} --select {}", current.id, current.id),
    ];
    expected_lines.sort();
    assert_eq!(actual_lines, expected_lines);
}

#[test]
fn test_multi_agent_grid_selection_and_tiling_in_tmux() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    let ids = built(&ctx);

    let initial_panes = ctx.layout.panes().unwrap();
    let main_agents: Vec<_> = initial_panes
        .iter()
        .filter(|p| p.window == "main" && p.kind == "agent")
        .collect();
    assert_eq!(main_agents.len(), 1);
    assert_eq!(main_agents[0].id, ids["left"]);

    ctx.layout.select_agent(&ids["right"], true).unwrap();

    let grid_panes = ctx.layout.panes().unwrap();
    let main_grid_agents: Vec<_> = grid_panes
        .iter()
        .filter(|p| p.window == "main" && p.kind == "agent")
        .collect();
    assert_eq!(main_grid_agents.len(), 2);
    let selected = ctx.layout.get_selected();
    assert_eq!(selected.len(), 2);

    ctx.layout
        .close_agent(
            &ids["left"],
            &sleeper("planner", 0, "placeholder", "placeholder"),
        )
        .unwrap();

    let remaining_panes = ctx.layout.panes().unwrap();
    let remaining_main_agents: Vec<_> = remaining_panes
        .iter()
        .filter(|p| p.window == "main" && p.kind == "agent")
        .collect();
    assert_eq!(remaining_main_agents.len(), 1);
    assert_eq!(remaining_main_agents[0].id, ids["right"]);
    let updated_selected = ctx.layout.get_selected();
    assert_eq!(updated_selected.len(), 1);
}

#[test]
fn test_responsive_reflow_resize_cycle_in_tmux() {
    let ctx = match TestContext::new() {
        Some(c) => c,
        None => return,
    };
    let ids = built(&ctx);

    let initial_panes = ctx.layout.panes().unwrap();
    let initial_sidebar = initial_panes
        .iter()
        .find(|p| p.role == "sidebar" && p.window == "main");
    assert!(initial_sidebar.is_some());

    // 1. Medium breakpoint (100 cols, 25 rows)
    ctx.layout.reflow(100, 25).unwrap();
    let med_panes = ctx.layout.panes().unwrap();
    let med_sidebar = med_panes
        .iter()
        .find(|p| p.role == "sidebar" && p.window == "main");
    assert!(med_sidebar.is_some());
    assert!(med_panes.iter().any(|p| p.id == ids["left"]));

    // 2. Narrow breakpoint (70 cols, 18 rows) -> hides sidebar
    ctx.layout.reflow(70, 18).unwrap();
    let narrow_panes = ctx.layout.panes().unwrap();
    let narrow_main_sidebar = narrow_panes
        .iter()
        .find(|p| p.role == "sidebar" && p.window == "main");
    assert!(narrow_main_sidebar.is_none());
    let narrow_stashed_sidebar = narrow_panes
        .iter()
        .find(|p| p.role == "sidebar" && p.window != "main");
    assert!(narrow_stashed_sidebar.is_some());
    assert!(narrow_panes.iter().any(|p| p.id == ids["left"]));

    // 3. Wide breakpoint restored (150 cols, 40 rows) -> un-hides sidebar
    ctx.layout.reflow(150, 40).unwrap();
    let wide_panes = ctx.layout.panes().unwrap();
    let wide_sidebar = wide_panes
        .iter()
        .find(|p| p.role == "sidebar" && p.window == "main");
    assert!(wide_sidebar.is_some());
    assert!(wide_panes.iter().any(|p| p.id == ids["left"]));
    assert!(
        wide_panes
            .iter()
            .any(|p| p.role == "status" && p.window == "main")
    );
}
