use std::collections::{BTreeMap, HashMap};
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::adapters::{AgentSpec, adapter_for, session_env, session_id};
use crate::config::Config;
use crate::conversations;
use crate::coordination::schema::STALE_SECONDS;
use crate::coordination::store::CoordStore;
use crate::notice;
use crate::quota;
use crate::selfexec;
use crate::state::{self, SavedState};
use crate::tmux::client::{TmuxClient, TmuxError, socket_for};
use crate::tmux::keys::{self, CLICK_ACTIONS, RESIZE_TABLE};
use crate::tmux::layout::{Launch, Layout, ROLES, SESSION};
use crate::tmux::tabs::{PaneInfo, agents_in, tab_text, visible_pane};
use crate::update_check;
use crate::updater::{self, StaleAgent};
use crate::versions;

pub const CLOSING: &str = "@tandem_closing";

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}

pub fn agent_launch(
    spec: &AgentSpec,
    coord_dir: &str,
    workspace: &Path,
    resume: bool,
    claude_dir: Option<&Path>,
) -> Launch {
    let mut spec = spec.clone();
    if spec.cli == "claude" && spec.conversation.is_none() {
        spec.conversation = Some(conversations::new_id());
    }

    let default_claude = home_dir().join(".claude");
    let claude_path = claude_dir.unwrap_or(&default_claude);

    let reopen =
        resume && conversations::can_resume(&spec.cli, spec.conversation.as_deref(), claude_path);

    if matches!(spec.cli.as_str(), "agy" | "opencode") && !reopen {
        spec.conversation = None;
    }

    let coord = Path::new(coord_dir);
    let log_file = if spec.cli == "agy" {
        conversations::agy_log_path(coord, &session_id(&spec))
            .ok()
            .map(|p| p.to_string_lossy().to_string())
    } else {
        None
    };

    if !reopen {
        conversations::forget(coord, &session_id(&spec));
    }

    let adapter = adapter_for(&spec.cli).unwrap_or_else(|_| {
        panic!("unsupported cli: {}", spec.cli);
    });
    let argv = adapter
        .build_argv(&spec, reopen, log_file.as_deref())
        .unwrap_or_else(|_| vec![spec.cli.clone()]);

    let cli_version = versions::installed_version(&spec.cli);
    let cwd = spec
        .cwd
        .clone()
        .unwrap_or_else(|| workspace.to_string_lossy().to_string());
    let spec_json = serde_json::json!({
        "cli": spec.cli,
        "model": spec.model,
        "skip_permissions": spec.skip_permissions,
        "cwd": spec.cwd,
        "conversation": spec.conversation,
    })
    .to_string();

    let mut env_map: HashMap<String, String> = session_env(&spec, coord_dir).into_iter().collect();
    if let Ok(extra) = adapter.env(&spec) {
        env_map.extend(extra);
    }

    Launch {
        argv,
        role: spec.role.clone(),
        index: spec.index,
        label: format!("{}·{}", spec.cli, spec.model),
        kind: "agent".to_string(),
        env: env_map,
        cwd: Some(cwd),
        session_id: session_id(&spec),
        spec: spec_json,
        cli_version,
        name: spec.name.clone().unwrap_or_default(),
    }
}

pub fn placeholder_launch(
    role: &str,
    self_cmd: Option<&[String]>,
    workspace: Option<&Path>,
) -> Launch {
    let mut cmd = self_cmd
        .map(|s| s.to_vec())
        .unwrap_or_else(selfexec::self_argv);
    cmd.extend(["placeholder".to_string(), role.to_string()]);

    let mut env = HashMap::new();
    if let Some(ws) = workspace {
        env.insert(
            "TANDEM_WORKSPACE".to_string(),
            ws.to_string_lossy().to_string(),
        );
    }

    Launch {
        argv: cmd,
        role: role.to_string(),
        index: 0,
        label: "placeholder".to_string(),
        kind: "own".to_string(),
        env,
        cwd: None,
        session_id: String::new(),
        spec: String::new(),
        cli_version: String::new(),
        name: String::new(),
    }
}

pub fn status_launch(self_cmd: Option<&[String]>, workspace: Option<&Path>) -> Launch {
    let mut cmd = self_cmd
        .map(|s| s.to_vec())
        .unwrap_or_else(selfexec::self_argv);
    cmd.push("status".to_string());

    let mut env = HashMap::new();
    if let Some(ws) = workspace {
        env.insert(
            "TANDEM_WORKSPACE".to_string(),
            ws.to_string_lossy().to_string(),
        );
    }

    Launch {
        argv: cmd,
        role: "status".to_string(),
        index: 0,
        label: "status".to_string(),
        kind: "own".to_string(),
        env,
        cwd: None,
        session_id: String::new(),
        spec: String::new(),
        cli_version: String::new(),
        name: String::new(),
    }
}

pub fn sidebar_launch(self_cmd: Option<&[String]>, workspace: Option<&Path>) -> Launch {
    let mut cmd = self_cmd
        .map(|s| s.to_vec())
        .unwrap_or_else(selfexec::self_argv);
    cmd.push("sidebar".to_string());

    let mut env = HashMap::new();
    if let Some(ws) = workspace {
        env.insert(
            "TANDEM_WORKSPACE".to_string(),
            ws.to_string_lossy().to_string(),
        );
    }

    Launch {
        argv: cmd,
        role: "sidebar".to_string(),
        index: 0,
        label: "sidebar".to_string(),
        kind: "own".to_string(),
        env,
        cwd: None,
        session_id: String::new(),
        spec: String::new(),
        cli_version: String::new(),
        name: String::new(),
    }
}

pub fn coord_dir_for(workspace: &Path, config: &Config) -> PathBuf {
    config
        .coord_dir
        .clone()
        .unwrap_or_else(|| workspace.join(".agent-workspace").join("coordination"))
}

pub fn make_layout(workspace: &Path, config: &Config) -> Layout {
    let conf = config.state_dir.join("tandem.tmux.conf");
    let socket = socket_for(workspace);
    let mut layout = Layout::new(&TmuxClient::new(&socket, None));
    layout.coord = Some(CoordStore::new(coord_dir_for(workspace, config)));
    layout.workspace = Some(workspace.to_string_lossy().to_string());
    let _ = layout.write_conf(&conf);
    layout.client = TmuxClient::new(&socket, Some(conf));
    layout
}

pub fn inspect_layout(workspace: &Path, config: &Config) -> Layout {
    let conf = config.state_dir.join("tandem.tmux.conf");
    let socket = socket_for(workspace);
    let mut layout = Layout::new(&TmuxClient::new(&socket, Some(conf)));
    layout.coord = Some(CoordStore::new(coord_dir_for(workspace, config)));
    layout.workspace = Some(workspace.to_string_lossy().to_string());
    layout
}

pub fn start(
    specs: &[AgentSpec],
    workspace: &Path,
    config: &Config,
    saved: Option<&SavedState>,
) -> Result<(), anyhow::Error> {
    let layout = make_layout(workspace, config);
    let coord_dir = coord_dir_for(workspace, config);
    let _ = conversations::prepare(&coord_dir);
    let resume = saved.is_some();

    let claude_dir = Some(config.claude_dir.as_path());
    let coord_str = coord_dir.to_string_lossy();

    let sidebar_l = sidebar_launch(None, Some(workspace));

    let mut ordered_specs: Vec<&AgentSpec> = specs.iter().collect();
    ordered_specs.sort_by(|a, b| {
        let role_order = |r: &str| match r {
            "planner" => 0,
            "implementer" => 1,
            _ => 2,
        };
        role_order(&a.role)
            .cmp(&role_order(&b.role))
            .then(a.index.cmp(&b.index))
    });

    let main_l = if let Some(first) = ordered_specs.first() {
        agent_launch(first, &coord_str, workspace, resume, claude_dir)
    } else {
        placeholder_launch("agent", None, Some(workspace))
    };

    let status_l = status_launch(None, Some(workspace));

    let (cols, rows) = crossterm::terminal::size().unwrap_or((160, 45));
    layout.build(cols as usize, rows as usize, &sidebar_l, &main_l, &status_l)?;

    for spec in ordered_specs.iter().skip(1) {
        let l = agent_launch(spec, &coord_str, workspace, resume, claude_dir);
        let _ = layout.add_agent(&l, false);
    }

    if let Some(st) = saved {
        let panes = layout.panes().unwrap_or_default();
        let mut activated = false;
        for sess in &st.ui.selected {
            if let Some(target) = panes
                .iter()
                .find(|p| p.session == *sess && p.kind == "agent")
            {
                let _ = layout.show_pane(&target.id);
                activated = true;
                break;
            }
        }
        if !activated {
            for role in &ROLES {
                if let Some(idx) = st.active.get(*role) {
                    let _ = layout.select(role, *idx);
                    break;
                }
            }
        }
        let _ = layout.apply_sizes(Some(st.ui.sidebar_cols), Some(st.ui.status_rows));
        let _ = layout.focus("planner");
    }

    autosave(&layout, workspace);
    let _ = crate::registry::register_or_heartbeat(
        &config.state_dir,
        workspace,
        Some(std::process::id()),
    );
    attach(workspace);
    Ok(())
}

pub fn attach(workspace: &Path) {
    let socket = socket_for(workspace);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new("tmux")
            .args(["-L", &socket, "attach-session", "-t", SESSION])
            .exec();
        eprintln!("Failed to attach tmux session: {}", err);
        std::process::exit(1);
    }
    #[cfg(not(unix))]
    {
        let _ = std::process::Command::new("tmux")
            .args(["-L", &socket, "attach-session", "-t", SESSION])
            .status();
        std::process::exit(0);
    }
}

pub fn add_agent(spec: &AgentSpec, workspace: &Path, config: &Config) -> Result<(), anyhow::Error> {
    let layout = make_layout(workspace, config);
    let panes = layout.panes()?;
    let same_role: Vec<usize> = panes
        .iter()
        .filter(|p| p.role == spec.role && p.kind == "agent")
        .map(|p| p.index)
        .collect();
    let max_index = same_role.into_iter().max().unwrap_or(0);
    let mut indexed = spec.clone();
    indexed.index = max_index + 1;

    let coord_dir = coord_dir_for(workspace, config);
    let _ = conversations::prepare(&coord_dir);
    let launch = agent_launch(
        &indexed,
        &coord_dir.to_string_lossy(),
        workspace,
        false,
        Some(config.claude_dir.as_path()),
    );
    layout.add_agent(&launch, true)?;
    autosave(&layout, workspace);
    Ok(())
}

fn own_role(layout: &Layout, config: &Config) -> String {
    let mut args = vec!["-p".to_string()];
    if !config.tmux_pane.is_empty() {
        args.extend(["-t".to_string(), config.tmux_pane.clone()]);
    }
    args.push("#{@tandem_role}".to_string());
    let str_args: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    layout
        .client
        .run_checked(&str_args)
        .unwrap_or_default()
        .trim()
        .to_string()
}

pub fn tab(
    layout: &mut Layout,
    config: &Config,
    direction: crate::cli::TabDirection,
    number: Option<i32>,
) {
    let role = own_role(layout, config);
    match direction {
        crate::cli::TabDirection::Select => {
            if ROLES.contains(&role.as_str())
                && let Some(num) = number
                && num >= 1
            {
                let _ = layout.select(&role, num as usize);
            }
        }
        crate::cli::TabDirection::Next => {
            let _ = layout.step(&role, 1);
        }
        crate::cli::TabDirection::Prev => {
            let _ = layout.step(&role, -1);
        }
    }
}

pub fn focus(layout: &mut Layout, _config: &Config, target: crate::cli::FocusTarget) {
    match target {
        crate::cli::FocusTarget::Planner => {
            let _ = layout.focus("planner");
        }
        crate::cli::FocusTarget::Implementer => {
            let _ = layout.focus("implementer");
        }
        crate::cli::FocusTarget::Sidebar => {
            let _ = layout.focus("sidebar");
        }
        crate::cli::FocusTarget::Settings => {
            let panes = match layout.panes() {
                Ok(p) => p,
                Err(_) => return,
            };
            let status = match panes.iter().find(|p| p.role == "status") {
                Some(s) => s,
                None => return,
            };
            let _ = layout
                .client
                .run_checked(&["select-pane", "-t", &status.id]);
            let _ = layout.client.send_keys_own(&status.id, &["7"]);
        }
        crate::cli::FocusTarget::Status | crate::cli::FocusTarget::Queries => {
            let panes = match layout.panes() {
                Ok(p) => p,
                Err(_) => return,
            };
            let status = match panes.iter().find(|p| p.role == "status") {
                Some(s) => s,
                None => return,
            };
            let _ = layout
                .client
                .run_checked(&["select-pane", "-t", &status.id]);
            if target == crate::cli::FocusTarget::Queries {
                let _ = layout.client.send_keys_own(&status.id, &["M-q"]);
            } else {
                let _ = layout.client.send_keys_own(&status.id, &["M-s"]);
            }
        }
    }
}

pub fn close_agent(layout: &mut Layout, config: &Config) {
    let pane = if !config.tmux_pane.is_empty() {
        config.tmux_pane.clone()
    } else {
        layout
            .client
            .run_checked(&["display-message", "-p", "#{pane_id}"])
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let role = own_role(layout, config);
    if ROLES.contains(&role.as_str()) {
        let ws_path = layout.workspace.as_ref().map(Path::new);
        let ph = placeholder_launch(&role, None, ws_path);
        let _ = layout.close_agent(&pane, &ph);
        if let Some(ref ws) = layout.workspace {
            autosave(layout, Path::new(ws));
        }
    }
}

pub fn pane_spec(pane: &PaneInfo, coord_dir: Option<&Path>) -> Option<AgentSpec> {
    let raw: serde_json::Value = serde_json::from_str(&pane.spec).ok()?;
    let cli = raw.get("cli")?.as_str()?.to_string();
    let model = raw.get("model")?.as_str()?.to_string();
    let skip_permissions = raw
        .get("skip_permissions")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let cwd = raw
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let raw_conv = raw.get("conversation").and_then(|v| v.as_str());
    let live = if let Some(cd) = coord_dir {
        conversations::current(cd, &pane.session, &cli)
    } else {
        None
    };
    let conv = live.or_else(|| raw_conv.and_then(conversations::valid_id));
    let name = if !pane.name.is_empty() {
        Some(pane.name.clone())
    } else {
        None
    };
    Some(AgentSpec {
        cli,
        role: pane.role.clone(),
        model,
        index: pane.index,
        skip_permissions,
        cwd,
        conversation: conv,
        name,
    })
}

pub fn capture_state(layout: &Layout, clean: bool) -> Result<SavedState, TmuxError> {
    let panes = layout.panes()?;
    let coord_dir = layout.coord.as_ref().map(|c| c.root.as_path());
    let mut agent_panes: Vec<&PaneInfo> = panes.iter().filter(|p| p.kind == "agent").collect();
    agent_panes.sort_by_key(|p| {
        let role_idx = ROLES.iter().position(|r| *r == p.role).unwrap_or(99);
        (role_idx, p.index)
    });
    let agents: Vec<AgentSpec> = agent_panes
        .into_iter()
        .filter_map(|p| pane_spec(p, coord_dir))
        .collect();

    let mut active = BTreeMap::new();
    for role in &ROLES {
        if let Some(shown) = panes
            .iter()
            .find(|p| p.role == *role && p.window == "main" && p.kind == "agent")
        {
            active.insert(role.to_string(), shown.index);
        }
    }
    let (cols, rows) = layout.sizes().unwrap_or((None, None));
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let mut state = SavedState::new(
        agents,
        active,
        cols.map(|c| c as u32),
        rows.map(|r| r as u32),
        now,
    );
    let visible_agent = panes
        .iter()
        .find(|p| p.window == "main" && p.kind == "agent");
    if let Some(vis) = visible_agent
        && !vis.session.is_empty()
    {
        state.ui.selected = vec![vis.session.clone()];
    }
    if let Some(c) = cols {
        state.ui.sidebar_cols = c;
    }
    if let Some(r) = rows {
        state.ui.status_rows = r;
    }
    state.clean = clean;
    Ok(state)
}

pub fn save_state(
    layout: &Layout,
    workspace: &Path,
    clean: bool,
) -> Result<SavedState, anyhow::Error> {
    let snapshot = capture_state(layout, clean)?;
    if snapshot.agents.is_empty() {
        return Err(anyhow::anyhow!("no agents to save"));
    }
    state::save(&state::state_path(workspace), &snapshot)?;
    Ok(snapshot)
}

pub fn is_closing(layout: &Layout) -> bool {
    layout
        .client
        .run(&["show-options", "-qv", "-t", SESSION, CLOSING])
        .map(|r| r.out.trim() == "1")
        .unwrap_or(false)
}

pub fn autosave(layout: &Layout, workspace: &Path) -> bool {
    if !layout.exists() || is_closing(layout) {
        return false;
    }
    let snapshot = match capture_state(layout, false) {
        Ok(s) => s,
        Err(_) => return false,
    };
    if snapshot.agents.is_empty() || is_closing(layout) {
        return false;
    }
    let saved = state::save(&state::state_path(workspace), &snapshot).is_ok();
    if saved {
        let state_dir = crate::registry::resolve_state_dir();
        let _ =
            crate::registry::register_or_heartbeat(&state_dir, workspace, Some(std::process::id()));
    }
    saved
}

pub fn save_exit(layout: &Layout, workspace: &Path) {
    let _ = layout
        .client
        .run_checked(&["set-option", "-t", SESSION, CLOSING, "1"]);
    let state_dir = crate::registry::resolve_state_dir();
    let _ = crate::registry::remove(&state_dir, workspace);
    let snapshot = match save_state(layout, workspace, true) {
        Ok(s) => s,
        Err(e) => {
            let _ = layout
                .client
                .run_checked(&["set-option", "-u", "-t", SESSION, CLOSING]);
            let _ = layout
                .client
                .run_checked(&["display-message", &format!("Not saved, session kept: {e}")]);
            return;
        }
    };
    let _ = layout.client.run_checked(&[
        "display-message",
        &format!("Saved {} agents. Closing.", snapshot.agents.len()),
    ]);
    let _ = layout.client.run_checked(&["kill-session", "-t", SESSION]);
}

pub fn discard_exit(layout: &Layout, workspace: &Path) {
    let _ = layout
        .client
        .run_checked(&["set-option", "-t", SESSION, CLOSING, "1"]);
    let state_dir = crate::registry::resolve_state_dir();
    let _ = crate::registry::remove(&state_dir, workspace);
    let sp = state::state_path(workspace);
    let _ = std::fs::remove_file(sp);
    let _ = layout.client.run_checked(&["kill-session", "-t", SESSION]);
}

pub fn restart(layout: &mut Layout, pane_id: &str, workspace: &Path, config: &Config) {
    let panes = match layout.panes() {
        Ok(p) => p,
        Err(_) => return,
    };
    let pane = panes.iter().find(|p| p.id == pane_id && p.kind == "agent");
    let coord_dir = coord_dir_for(workspace, config);
    let spec = pane.and_then(|p| pane_spec(p, Some(&coord_dir)));
    let coord_str = coord_dir.to_string_lossy();
    if let Some(s) = spec {
        let _ = conversations::prepare(&coord_dir);
        let launch = agent_launch(
            &s,
            &coord_str,
            workspace,
            true,
            Some(config.claude_dir.as_path()),
        );
        let _ = layout.restart(pane_id, &coord_str, Some(&launch));
    } else {
        let _ = layout.restart(pane_id, &coord_str, None);
    }
}

pub fn resume_specs<F>(
    saved: &SavedState,
    _workspace: &Path,
    which: F,
) -> (Vec<AgentSpec>, Vec<String>)
where
    F: Fn(&str) -> Option<PathBuf>,
{
    let mut specs = Vec::new();
    let mut notes = Vec::new();
    for a in &saved.agents {
        if which(&a.cli).is_none() {
            notes.push(format!(
                "Skipped {} {}: {} is not installed.",
                a.role, a.index, a.cli
            ));
            continue;
        }
        let mut agent = a.clone();
        if let Some(ref cwd) = a.cwd
            && !Path::new(cwd).is_dir()
        {
            notes.push(format!(
                "{} {}: {} no longer exists, using the workspace folder.",
                a.role, a.index, cwd
            ));
            agent.cwd = None;
            if agent.cli == "claude" {
                agent.conversation = None;
            }
        }
        specs.push(agent);
    }
    (specs, notes)
}

pub fn quit_menu(layout: &Layout, workspace: Option<&str>) {
    let self_cmd_refs: Option<Vec<&str>> = layout
        .self_cmd
        .as_ref()
        .map(|v| v.iter().map(|s| s.as_str()).collect());
    let launcher = keys::launcher_for(
        self_cmd_refs.as_deref(),
        workspace.or(layout.workspace.as_deref()),
    );
    let mut update_label = "Update tandem and workspace".to_string();
    if let Some(ws) = workspace {
        let ws_path = Path::new(ws);
        let check_state = update_check::read_state(ws_path);
        let installed = update_check::installed_version_from_manifest(ws_path);
        if update_check::should_offer(
            installed.as_deref(),
            Some(&check_state.latest),
            &check_state.skip,
        ) {
            update_label = format!(
                "Update tandem and workspace  ({} available)",
                check_state.latest
            );
        }
    }
    let save_exit_cmd = keys::run_shell(&launcher, "save-exit");
    let update_cmd = keys::popup(&launcher, "update", "80%", "70%");
    let discard_exit_cmd = keys::run_shell(&launcher, "discard-exit");

    let _ = layout.client.run_checked(&[
        "display-menu",
        "-T",
        "tandem",
        "Save state and exit (resume later)",
        "s",
        &save_exit_cmd,
        "Detach (agents keep running)",
        "d",
        "detach-client",
        &update_label,
        "u",
        &update_cmd,
        "Quit and discard saved state (next launch is fresh)",
        "k",
        &discard_exit_cmd,
        "Cancel",
        "c",
        "",
    ]);
}

pub fn reload_ui(layout: &mut Layout, workspace: &Path, _config: &Config) {
    let _ = layout.apply_chrome();
    let panes = layout.panes().unwrap_or_default();
    let has_sidebar = panes
        .iter()
        .any(|p| p.role == "sidebar" && p.window == "main");

    if !has_sidebar {
        let agent_panes: Vec<PaneInfo> = panes
            .iter()
            .filter(|p| p.window == "main" && p.kind == "agent")
            .cloned()
            .collect();
        for p in &agent_panes {
            let stash_name = format!("stash-{}-{}", p.role, p.index);
            let _ = layout.client.run_checked(&[
                "break-pane",
                "-d",
                "-s",
                p.id.as_str(),
                "-n",
                &stash_name,
            ]);
        }
        for p in panes.iter().filter(|p| p.window == "main") {
            let _ = layout
                .client
                .run_checked(&["kill-pane", "-t", p.id.as_str()]);
        }
        let sidebar_l = sidebar_launch(None, Some(workspace));
        let status_l = status_launch(None, Some(workspace));
        let (cols, rows) = crossterm::terminal::size().unwrap_or((160, 45));
        let main_l = if let Some(first) = agent_panes.first() {
            Launch::new(
                vec!["sleep".to_string(), "0".to_string()],
                &first.role,
                first.index,
                &first.label,
            )
        } else {
            placeholder_launch("agent", None, Some(workspace))
        };
        let _ = layout.build(cols as usize, rows as usize, &sidebar_l, &main_l, &status_l);
        if let Some(first) = agent_panes.first() {
            let _ = layout.show_pane(&first.id);
        }
    } else {
        let mut launches = HashMap::new();
        launches.insert("status".to_string(), status_launch(None, Some(workspace)));
        launches.insert("sidebar".to_string(), sidebar_launch(None, Some(workspace)));
        launches.insert(
            "placeholder-agent".to_string(),
            placeholder_launch("agent", None, Some(workspace)),
        );
        for r in &ROLES {
            launches.insert(
                format!("placeholder-{r}"),
                placeholder_launch(r, None, Some(workspace)),
            );
        }
        let _ = layout.respawn_own(&launches);
    }
    let _ = layout.refresh();
}

pub fn reload_in_fresh_process(workspace: &Path, layout: &Layout) -> bool {
    if !layout.exists() {
        return false;
    }
    let self_cmd = selfexec::self_argv();
    let mut cmd = std::process::Command::new(&self_cmd[0]);
    cmd.args(&self_cmd[1..]);
    cmd.arg("reload");
    cmd.current_dir(workspace);
    cmd.env("TANDEM_WORKSPACE", workspace);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

pub fn run_update(workspace: &Path, config: &Config, assume_yes: bool) -> i32 {
    let layout = make_layout(workspace, config);
    println!("tandem update\n");
    for line in updater::plan(workspace, &config.update_spec, None) {
        println!("{line}");
    }

    if !assume_yes {
        print!("\nContinue? [y/N] ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_err() {
            return 1;
        }
        let trimmed = line.trim().to_lowercase();
        if trimmed != "y" && trimmed != "yes" {
            println!("Cancelled.");
            return 1;
        }
    }

    if layout.exists() {
        match save_state(&layout, workspace, false) {
            Ok(_) => println!("Saved current state first."),
            Err(e) => println!("(state not saved: {e})"),
        }
    }

    let result = updater::run_update(
        workspace,
        &config.update_spec,
        || reload_in_fresh_process(workspace, &layout),
        None,
        None,
        None,
        None,
        None,
    );

    println!(
        "\n{}: {}",
        if result.ok { "OK" } else { "FAILED" },
        result.message
    );

    if result.ok
        && !result.old_version.is_empty()
        && !result.new_version.is_empty()
        && result.old_version != result.new_version
    {
        let _ = notice::write_notice(workspace, &result.old_version, &result.new_version, None);
    }

    if result.ok && layout.exists() {
        offer_restarts(
            &layout,
            workspace,
            config,
            io::stdin().is_terminal(),
            &mut |prompt| {
                print!("{prompt}");
                let _ = io::stdout().flush();
                let mut line = String::new();
                if io::stdin().read_line(&mut line).is_ok() {
                    Some(line)
                } else {
                    None
                }
            },
            &mut |s| println!("{s}"),
        );
    }

    if io::stdin().is_terminal() {
        print!("\nPress Enter to close.");
        let _ = io::stdout().flush();
        let mut line = String::new();
        let _ = io::stdin().read_line(&mut line);
    }

    if result.ok { 0 } else { 1 }
}

pub fn find_stale(layout: &Layout, workspace: &Path, config: &Config) -> Vec<StaleAgent> {
    let coord_dir = coord_dir_for(workspace, config);
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let mut busy = HashMap::new();

    if let Some(ref coord) = layout.coord {
        for claim in coord.claims() {
            if now - claim.heartbeat < STALE_SECONDS as f64 {
                let phase_id = claim
                    .phase
                    .split('-')
                    .take(2)
                    .collect::<Vec<&str>>()
                    .join("-");
                busy.insert(claim.session.clone(), format!("{phase_id} {}", claim.task));
            }
        }
    }

    let panes = match layout.panes() {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };

    let resumable = |p: &PaneInfo| {
        let spec = pane_spec(p, Some(&coord_dir));
        spec.as_ref()
            .map(|s| {
                conversations::can_resume(&s.cli, s.conversation.as_deref(), &config.claude_dir)
            })
            .unwrap_or(false)
    };

    updater::stale_agents(
        &panes,
        &|cli: &str| versions::installed_version(cli),
        &resumable,
        &busy,
    )
}

pub fn restart_stale(
    layout: &mut Layout,
    workspace: &Path,
    config: &Config,
    chosen: &[StaleAgent],
) -> Vec<String> {
    let coord_dir = coord_dir_for(workspace, config);
    let _ = conversations::prepare(&coord_dir);
    let panes = match layout.panes() {
        Ok(p) => p,
        Err(_) => return Vec::new(),
    };
    let pane_map: HashMap<String, &PaneInfo> = panes.iter().map(|p| (p.id.clone(), p)).collect();
    let mut messages = Vec::new();

    for agent in chosen {
        let name = format!("{} {}", agent.role, agent.index);
        let pane = pane_map.get(&agent.pane_id);
        let spec = pane.and_then(|p| pane_spec(p, Some(&coord_dir)));
        let spec = match spec {
            Some(s) => s,
            None => {
                messages.push(format!(
                    "{name}: not restarted (its launch details are not available)"
                ));
                continue;
            }
        };

        let coord_str = coord_dir.to_string_lossy();
        let launch = agent_launch(
            &spec,
            &coord_str,
            workspace,
            true,
            Some(config.claude_dir.as_path()),
        );

        match layout.restart_live(&agent.pane_id, &launch) {
            Ok(_) => messages.push(format!("{name}: restarted on {}", agent.installed)),
            Err(e) => messages.push(format!("{name}: not restarted ({e})")),
        }
    }

    messages
}

pub fn offer_restarts(
    layout: &Layout,
    workspace: &Path,
    config: &Config,
    interactive: bool,
    ask: &mut dyn FnMut(&str) -> Option<String>,
    out: &mut dyn FnMut(&str),
) {
    let stale = find_stale(layout, workspace, config);
    if stale.is_empty() {
        return;
    }
    if !interactive {
        out("\nThese agents still run an older CLI (restart them from tandem when convenient):");
        for agent in &stale {
            out(&format!("  {}", updater::describe_stale(agent)));
        }
        return;
    }
    let chosen = updater::choose_restarts(&stale, ask, out);
    if chosen.is_empty() {
        out("No agents restarted.");
        return;
    }
    let mut mut_layout = make_layout(workspace, config);
    for message in restart_stale(&mut mut_layout, workspace, config, &chosen) {
        out(&message);
    }
    autosave(&mut_layout, workspace);
}

pub fn first_run(layout: &Layout, config: &Config) {
    let self_cmd_refs: Option<Vec<&str>> = layout
        .self_cmd
        .as_ref()
        .map(|v| v.iter().map(|s| s.as_str()).collect());
    let launcher = keys::launcher_for(
        self_cmd_refs.as_deref(),
        config.workspace.as_ref().and_then(|w| w.to_str()),
    );

    if let Some(ref ws) = config.workspace
        && let Some(n) = notice::read_notice(ws)
        && !n.seen
    {
        let _ = layout.client.run_checked(&[
            "display-popup",
            "-E",
            "-w",
            "70%",
            "-h",
            "70%",
            &format!("{launcher} whats-new"),
        ]);
    }

    let marker = config.state_dir.join("first-run-done");
    if marker.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(&config.state_dir);
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let _ = std::fs::write(&marker, now.to_string());
    let _ = layout.client.run_checked(&[
        "display-popup",
        "-E",
        "-w",
        "70%",
        "-h",
        "70%",
        &format!("{launcher} help"),
    ]);
}

pub fn show_whats_new(workspace: &Path) {
    let notice = match notice::read_notice(workspace) {
        Some(n) => n,
        None => {
            println!("No update notice found.");
            return;
        }
    };

    let changelog_path = workspace
        .join(".agent-workspace")
        .join("tandem")
        .join("CHANGELOG.md");
    let changelog = std::fs::read_to_string(&changelog_path).unwrap_or_default();

    println!(
        "What's new in tandem {} (updated from {})",
        notice.to_version, notice.from_version
    );
    let bullets = notice::whats_new(&changelog, &notice.from_version, &notice.to_version, None);
    if !bullets.is_empty() {
        println!();
        for b in bullets {
            println!(" • {b}");
        }
    } else {
        println!("\n (release notes unavailable)");
    }

    println!("\nFull notes: {}", changelog_path.display());
    println!("Press Enter to close.");
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);
    let _ = notice::mark_seen(workspace);
}

pub fn click(layout: &mut Layout, config: &Config, action: &str) {
    if !CLICK_ACTIONS.contains(&action) {
        return;
    }

    let self_cmd_refs: Option<Vec<&str>> = layout
        .self_cmd
        .as_ref()
        .map(|v| v.iter().map(|s| s.as_str()).collect());
    let launcher = keys::launcher_for(self_cmd_refs.as_deref(), layout.workspace.as_deref());

    match action {
        "help" => {
            let _ = layout.client.run_checked(&[
                "display-popup",
                "-E",
                "-w",
                "70%",
                "-h",
                "70%",
                &format!("{launcher} help"),
            ]);
        }
        "add-agent" => {
            let _ = layout.client.run_checked(&[
                "display-popup",
                "-E",
                "-w",
                "90%",
                "-h",
                "90%",
                &format!("{launcher} add-agent"),
            ]);
        }
        "prev" => tab(layout, config, crate::cli::TabDirection::Prev, None),
        "next" => tab(layout, config, crate::cli::TabDirection::Next, None),
        "status" => focus(layout, config, crate::cli::FocusTarget::Status),
        "queries" => focus(layout, config, crate::cli::FocusTarget::Queries),
        "planner" => focus(layout, config, crate::cli::FocusTarget::Planner),
        "implementer" => focus(layout, config, crate::cli::FocusTarget::Implementer),
        "sidebar" => focus(layout, config, crate::cli::FocusTarget::Sidebar),
        "settings" => focus(layout, config, crate::cli::FocusTarget::Settings),
        "close-agent" => {
            let close_cmd = keys::run_shell(&launcher, "close-agent");
            let _ = layout.client.run_checked(&[
                "confirm-before",
                "-p",
                "Close this agent? (y/n)",
                &close_cmd,
            ]);
        }
        "quit" => quit_menu(layout, layout.workspace.as_deref()),
        "restart" => {
            let pane = if !config.tmux_pane.is_empty() {
                config.tmux_pane.clone()
            } else {
                layout
                    .client
                    .run_checked(&["display-message", "-p", "#{pane_id}"])
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            };
            if !pane.is_empty() {
                let ws_path = layout
                    .workspace
                    .as_ref()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                restart(layout, &pane, &ws_path, config);
            }
        }
        "resize" => {
            let _ = layout
                .client
                .run_checked(&["switch-client", "-T", RESIZE_TABLE]);
        }
        _ => {}
    }
}

pub fn click_tab(layout: &mut Layout, _config: &Config, pane_id: &str, select: &str) {
    let is_valid_pane = |s: &str| s.starts_with('%') && s[1..].chars().all(|c| c.is_ascii_digit());
    if !is_valid_pane(pane_id) || (!select.is_empty() && !is_valid_pane(select)) {
        return;
    }

    let role = layout
        .client
        .run(&["display-message", "-p", "-t", pane_id, "#{@tandem_role}"])
        .map(|r| r.out.trim().to_string())
        .unwrap_or_default();

    if !ROLES.contains(&role.as_str()) {
        return;
    }

    let panes = match layout.panes() {
        Ok(p) => p,
        Err(_) => return,
    };
    let agents = agents_in(&panes, &role);
    let self_cmd_refs: Option<Vec<&str>> = layout
        .self_cmd
        .as_ref()
        .map(|v| v.iter().map(|s| s.as_str()).collect());
    let launcher = keys::launcher_for(self_cmd_refs.as_deref(), layout.workspace.as_deref());

    if !select.is_empty() {
        if let Some(target) = agents.iter().find(|p| p.id == select) {
            let _ = layout.select(&role, target.index);
        }
        return;
    }

    if agents.is_empty() {
        let _ = layout.client.run_checked(&[
            "display-popup",
            "-E",
            "-w",
            "90%",
            "-h",
            "90%",
            &format!("{launcher} add-agent"),
        ]);
        return;
    }

    let shown = visible_pane(&panes, &role);
    let mut items = vec![
        "display-menu".to_string(),
        "-T".to_string(),
        "tandem".to_string(),
        "-t".to_string(),
        pane_id.to_string(),
        "-x".to_string(),
        "#{pane_left}".to_string(),
        "-y".to_string(),
        "#{pane_top}".to_string(),
    ];

    for p in &agents {
        let marker = if shown.as_ref().map(|s| s.id == p.id).unwrap_or(false) {
            "▶ "
        } else {
            "  "
        };
        let shortcut = if p.index < 10 {
            p.index.to_string()
        } else {
            String::new()
        };
        let select_cmd =
            keys::run_shell(&launcher, &format!("click-tab {} --select {}", p.id, p.id));
        items.push(format!("{marker}{}", tab_text(p)));
        items.push(shortcut);
        items.push(select_cmd);
    }

    items.push(String::new());
    items.push("New agent".to_string());
    items.push("n".to_string());
    items.push(keys::popup(&launcher, "add-agent", "90%", "90%"));

    let str_items: Vec<&str> = items.iter().map(|s| s.as_str()).collect();
    let _ = layout.client.run_checked(&str_items);
}

pub fn show_help() {
    for line in keys::help_lines(platform_name(), &keys::default_keymap()) {
        println!("{line}");
    }
    println!("\nPress Enter to close.");
    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);
}

pub fn placeholder(role: &str) {
    let add_key = keys::display_shortcut(
        &keys::key_for("add-agent", &keys::default_keymap()),
        platform_name(),
    )
    .unwrap_or_else(|_| "Ctrl+Alt+n".to_string());
    println!("No {role}s yet. Press {add_key} to add one.");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

pub fn quota_feed(config: &Config) {
    let mut input = String::new();
    let _ = io::stdin()
        .take(quota::MAX_INPUT as u64 + 1)
        .read_to_string(&mut input);
    let coord = config.coord_dir.as_deref();
    let out = if let Some(c) = coord {
        let now = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let cwd = std::env::current_dir().unwrap_or_default();
        let home = home_dir();
        let session = if config.session_id.is_empty() {
            ""
        } else {
            &config.session_id
        };
        quota::run_feed(&input, c, session, &cwd, &home, now)
    } else {
        String::new()
    };
    let _ = io::stdout().write_all(out.as_bytes());
}

pub fn quota_line(workspace: &Path, config: &Config) {
    let layout = inspect_layout(workspace, config);
    let panes = layout.panes().unwrap_or_default();

    let mut clis = Vec::new();
    for pane in &panes {
        if pane.kind != "agent" {
            continue;
        }
        if let Ok(raw) = serde_json::from_str::<serde_json::Value>(&pane.spec)
            && let Some(cli) = raw.get("cli").and_then(|v| v.as_str())
            && !clis.contains(&cli.to_string())
        {
            clis.push(cli.to_string());
        }
    }

    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let coord_dir = coord_dir_for(workspace, config);
    let claude = if clis.iter().any(|c| c == "claude") {
        quota::read_claude(&coord_dir)
    } else {
        None
    };

    let cli_refs: Vec<&str> = clis.iter().map(|s| s.as_str()).collect();
    let line = quota::format_line(&cli_refs, claude.as_ref(), now);
    let _ = io::stdout().write_all(line.as_bytes());
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WindowSize {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotPayload {
    pub workspace_id: String,
    pub workspace_path: String,
    pub snapshot: crate::snapshot::StatusSnapshot,
    pub panes: Vec<crate::tmux::tabs::PaneInfo>,
    pub selected: Vec<String>,
    pub quota_line: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowSize>,
}

pub fn collect_snapshot_json(
    workspace: &Path,
    config: &Config,
) -> Result<SnapshotPayload, anyhow::Error> {
    let resolved = crate::tmux::client::resolve_path(workspace);
    let workspace_id = crate::tmux::client::socket_for(&resolved);
    let workspace_path = resolved.to_string_lossy().to_string();

    let state_file = crate::state::state_path(&resolved);
    let workspace_name = if state_file.exists() {
        std::fs::read(&state_file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|val| {
                val.get("workspace_name")
                    .and_then(|v| v.as_str())
                    .filter(|s| crate::adapters::base::is_valid_custom_name(s))
                    .map(ToString::to_string)
            })
    } else {
        None
    };

    let coord_dir = coord_dir_for(workspace, config);
    let store = crate::coordination::store::CoordStore::new(coord_dir.clone());
    let snapshot =
        crate::snapshot::collect(workspace, &store, std::time::Duration::from_millis(500));

    let layout = inspect_layout(workspace, config);
    let panes = layout.panes().unwrap_or_default();
    let (selected, win_size) = if panes.is_empty() {
        (Vec::new(), None)
    } else {
        layout.selected_and_size(&panes)
    };
    let window = win_size.map(|(cols, rows)| WindowSize { cols, rows });

    let mut clis = Vec::new();
    for pane in &panes {
        if pane.kind != "agent" {
            continue;
        }
        if let Ok(raw) = serde_json::from_str::<serde_json::Value>(&pane.spec)
            && let Some(cli) = raw.get("cli").and_then(|v| v.as_str())
            && !clis.contains(&cli.to_string())
        {
            clis.push(cli.to_string());
        }
    }

    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let claude = if clis.iter().any(|c| c == "claude") {
        quota::read_claude(&coord_dir)
    } else {
        None
    };

    let cli_refs: Vec<&str> = clis.iter().map(|s| s.as_str()).collect();
    let quota_line = quota::format_line(&cli_refs, claude.as_ref(), now);

    Ok(SnapshotPayload {
        workspace_id,
        workspace_path,
        snapshot,
        panes,
        selected,
        quota_line,
        workspace_name,
        window,
    })
}

pub fn snapshot_fingerprint(payload: &SnapshotPayload) -> String {
    let mut stable = payload.clone();
    stable.snapshot.now = 0.0;
    serde_json::to_string(&stable).unwrap_or_default()
}

pub const WATCH_HEARTBEAT: std::time::Duration = std::time::Duration::from_secs(5);

pub fn snapshot_watch(
    workspace: &Path,
    config: &Config,
    interval: std::time::Duration,
    out: &mut dyn Write,
    max_ticks: Option<usize>,
) -> io::Result<()> {
    let mut last_fingerprint = String::new();
    let mut last_write = std::time::Instant::now();
    let mut ticks = 0usize;

    loop {
        match collect_snapshot_json(workspace, config) {
            Ok(payload) => {
                let fingerprint = snapshot_fingerprint(&payload);
                if fingerprint != last_fingerprint {
                    let line = serde_json::to_string(&payload).map_err(io::Error::other)?;
                    out.write_all(line.as_bytes())?;
                    out.write_all(b"\n")?;
                    out.flush()?;
                    last_fingerprint = fingerprint;
                    last_write = std::time::Instant::now();
                }
            }
            Err(err) => {
                let line = serde_json::json!({ "error": err.to_string() });
                writeln!(out, "{line}")?;
                out.flush()?;
                last_write = std::time::Instant::now();
            }
        }

        if last_write.elapsed() >= WATCH_HEARTBEAT {
            let now = SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0);
            writeln!(out, "{}", serde_json::json!({ "heartbeat": now }))?;
            out.flush()?;
            last_write = std::time::Instant::now();
        }

        ticks += 1;
        if max_ticks.is_some_and(|max| ticks >= max) {
            return Ok(());
        }
        std::thread::sleep(interval);
    }
}

pub fn snapshot_json(workspace: &Path, config: &Config, pretty: bool) {
    match collect_snapshot_json(workspace, config) {
        Ok(payload) => {
            let json = if pretty {
                serde_json::to_string_pretty(&payload)
            } else {
                serde_json::to_string(&payload)
            };
            if let Ok(json) = json {
                let _ = io::stdout().write_all(json.as_bytes());
                let _ = io::stdout().write_all(b"\n");
            }
        }
        Err(e) => {
            eprintln!("Error collecting snapshot: {e}");
            std::process::exit(1);
        }
    }
}

pub fn answer_query_inner(
    workspace: &Path,
    config: &Config,
    id: &str,
    answer: &str,
) -> Result<(), anyhow::Error> {
    if !crate::coordination::store::is_valid_query_id(id) {
        anyhow::bail!("Invalid query ID '{id}'");
    }
    let trimmed_answer = answer.trim();
    if trimmed_answer.is_empty() {
        anyhow::bail!("Answer text cannot be empty");
    }

    let coord_dir = coord_dir_for(workspace, config);
    let store = crate::coordination::store::CoordStore::new(coord_dir);
    store
        .answer(id, trimmed_answer, "cli")
        .map_err(|e| anyhow::anyhow!("{e}"))
}

pub fn answer_query_cmd(workspace: &Path, config: &Config, id: &str, answer: &str) {
    match answer_query_inner(workspace, config, id, answer) {
        Ok(()) => {
            let res = serde_json::json!({
                "ok": true,
                "id": id,
            });
            let _ =
                io::stdout().write_all(serde_json::to_string(&res).unwrap_or_default().as_bytes());
            let _ = io::stdout().write_all(b"\n");
        }
        Err(e) => {
            eprintln!("Error answering query: {e}");
            std::process::exit(1);
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ListIpsPayload {
    pub ips: Vec<crate::net_interfaces::IpCandidate>,
    pub workspaces: Vec<crate::registry::WorkspaceEntry>,
}

pub fn list_ips_payload(_workspace: &Path) -> ListIpsPayload {
    let ips = crate::net_interfaces::discover_ips();
    let state_dir = crate::registry::resolve_state_dir();
    let reg_path = crate::registry::default_registry_path(&state_dir);
    let workspaces = crate::registry::load_registry(&reg_path)
        .map(|r| r.workspaces)
        .unwrap_or_default();

    ListIpsPayload { ips, workspaces }
}

pub fn list_ips_json(workspace: &Path) {
    let payload = list_ips_payload(workspace);
    if let Ok(json) = serde_json::to_string_pretty(&payload) {
        let _ = io::stdout().write_all(json.as_bytes());
        let _ = io::stdout().write_all(b"\n");
    }
}

pub fn rename_workspace_cmd(workspace: &Path, name: Option<&str>) {
    let clean_name = name.map(str::trim).filter(|s| !s.is_empty());
    if clean_name.is_some_and(|n| !crate::adapters::base::is_valid_custom_name(n)) {
        eprintln!("Invalid workspace name: must be 1-32 alphanumeric characters");
        std::process::exit(1);
    }

    let state_dir = crate::registry::resolve_state_dir();
    let reg_path = crate::registry::default_registry_path(&state_dir);
    let resolved = crate::tmux::client::resolve_path(workspace);
    let target_id = crate::tmux::client::socket_for(&resolved);

    let default_name = resolved
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "workspace".to_string());

    let final_name = clean_name.unwrap_or(&default_name).to_string();

    if let Ok(mut reg) = crate::registry::load_registry(&reg_path) {
        let entry_opt = reg
            .workspaces
            .iter_mut()
            .find(|w| w.id == target_id || w.socket == target_id);
        if let Some(entry) = entry_opt {
            entry.name = final_name.clone();
            let _ = crate::registry::save_registry(&reg_path, &reg);
        }
    }

    let session_file = crate::state::state_path(&resolved);
    let mut file_val = std::fs::read(&session_file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    if let Some(obj) = file_val.as_mut().and_then(|v| v.as_object_mut()) {
        if let Some(n) = clean_name {
            obj.insert("workspace_name".to_string(), serde_json::json!(n));
        } else {
            obj.remove("workspace_name");
        }
        if let Ok(mut file) = std::fs::File::create(&session_file) {
            let _ = serde_json::to_writer_pretty(&mut file, &file_val);
        }
    }

    let res = serde_json::json!({
        "ok": true,
        "workspace_id": target_id,
        "name": final_name,
    });
    let _ = io::stdout().write_all(serde_json::to_string(&res).unwrap_or_default().as_bytes());
    let _ = io::stdout().write_all(b"\n");
}

pub fn rename_agent_cmd(workspace: &Path, config: &Config, target: &str, name: Option<&str>) {
    let clean_name = name.map(str::trim).filter(|s| !s.is_empty());
    if clean_name.is_some_and(|n| !crate::adapters::base::is_valid_custom_name(n)) {
        eprintln!("Invalid agent name: must be 1-32 alphanumeric characters");
        std::process::exit(1);
    }

    let resolved = crate::tmux::client::resolve_path(workspace);
    let layout = make_layout(workspace, config);
    let panes = if layout.exists() {
        layout.panes().unwrap_or_default()
    } else {
        Vec::new()
    };

    let target_pane = panes.iter().find(|p| {
        p.id == target || p.session == target || format!("{}-{}", p.role, p.index) == target
    });

    let pane_id = target_pane.map(|p| p.id.as_str());
    let session_id = target_pane
        .map(|p| {
            if !p.session.is_empty() {
                p.session.as_str()
            } else {
                ""
            }
        })
        .filter(|s| !s.is_empty())
        .unwrap_or(target);

    let name_val = clean_name.unwrap_or("");

    if let Some(pid) = pane_id {
        let _ =
            layout
                .client
                .run_checked(&["set-option", "-p", "-t", pid, "@tandem_name", name_val]);
        let _ = layout.refresh();
    }

    let session_file = crate::state::state_path(&resolved);
    let mut session_val = std::fs::read(&session_file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    if let Some(agents) = session_val
        .as_mut()
        .and_then(|v| v.get_mut("agents"))
        .and_then(|a| a.as_array_mut())
    {
        for a in agents {
            let role = a.get("role").and_then(|r| r.as_str()).unwrap_or("");
            let index = a.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
            let a_session = format!("{role}-{index}");
            let matches_target = a_session == target
                || (target_pane.is_some()
                    && target_pane.unwrap().role == role
                    && target_pane.unwrap().index == index as usize);
            if matches_target {
                if let Some(n) = clean_name {
                    a["name"] = serde_json::json!(n);
                } else if let Some(obj) = a.as_object_mut() {
                    obj.remove("name");
                }
            }
        }
        if let Ok(mut file) = std::fs::File::create(&session_file) {
            let _ = serde_json::to_writer_pretty(&mut file, &session_val);
        }
    }

    let coord_dir = coord_dir_for(workspace, config);
    let session_path = std::path::PathBuf::from(&coord_dir)
        .join("sessions")
        .join(format!("{session_id}.json"));
    let mut coord_val = std::fs::read(&session_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    if let Some(ref mut val) = coord_val {
        if let Some(n) = clean_name {
            val["name"] = serde_json::json!(n);
        } else if let Some(obj) = val.as_object_mut() {
            obj.remove("name");
        }
        if let Ok(mut file) = std::fs::File::create(&session_path) {
            let _ = serde_json::to_writer_pretty(&mut file, val);
        }
    }

    let res = serde_json::json!({
        "ok": true,
        "target": target,
        "name": name_val,
    });
    let _ = io::stdout().write_all(serde_json::to_string(&res).unwrap_or_default().as_bytes());
    let _ = io::stdout().write_all(b"\n");
}
