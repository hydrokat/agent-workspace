use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::coordination::store::CoordStore;
use crate::tmux::client::{TmuxClient, TmuxError};
use crate::tmux::keys::{
    bind_commands, default_keymap, launcher_for, mouse_commands, resize_commands, run_shell,
    status_left,
};
use crate::tmux::tabs::{FORMAT, PaneInfo, agents_in, parse_panes, render_title};

pub const SESSION: &str = "tandem";
pub const STATUS_ROWS: usize = 10;
pub const SIDEBAR_COLS: usize = 26;
pub const STACK_BELOW_COLS: usize = 100;
pub const MAX_STATUS_SHARE: f64 = 0.4;
pub const MIN_STATUS_ROWS: usize = 6;
pub const ROLES: [&str; 2] = ["planner", "implementer"];

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Launch {
    pub argv: Vec<String>,
    pub role: String,
    pub index: usize,
    pub label: String,
    pub kind: String,
    pub env: HashMap<String, String>,
    pub cwd: Option<String>,
    pub session_id: String,
    pub spec: String,
    pub cli_version: String,
}

impl Launch {
    pub fn new(argv: Vec<String>, role: &str, index: usize, label: &str) -> Self {
        Self {
            argv,
            role: role.to_string(),
            index,
            label: label.to_string(),
            kind: "agent".to_string(),
            env: HashMap::new(),
            cwd: None,
            session_id: String::new(),
            spec: String::new(),
            cli_version: String::new(),
        }
    }
}

pub fn conf_text(history: usize) -> String {
    format!(
        "set-option -g mouse on\n\
         set-option -g extended-keys on\n\
         set-environment -g -u TMUX_PANE\n\
         set-option -g prefix None\n\
         set-option -g prefix2 None\n\
         set-option -g escape-time 10\n\
         set-option -g history-limit {history}\n\
         set-option -g pane-border-status top\n\
         set-option -g status on\n\
         set-option -g status-position bottom\n\
         set-window-option -g remain-on-exit on\n"
    )
}

fn spawn_args(launch: &Launch) -> Vec<String> {
    let mut args = vec!["-P".to_string(), "-F".to_string(), "#{pane_id}".to_string()];
    if let Some(cwd) = &launch.cwd {
        args.push("-c".to_string());
        args.push(cwd.clone());
    }
    let mut env_keys: Vec<&String> = launch.env.keys().collect();
    env_keys.sort();
    for k in env_keys {
        args.push("-e".to_string());
        args.push(format!("{k}={}", launch.env[k]));
    }
    args.extend(launch.argv.clone());
    args
}

#[derive(Clone)]
pub struct Layout {
    pub client: TmuxClient,
    pub self_cmd: Option<Vec<String>>,
    pub coord: Option<CoordStore>,
    pub workspace: Option<String>,
}

impl Layout {
    pub fn new(client: &TmuxClient) -> Self {
        Self {
            client: client.clone(),
            self_cmd: None,
            coord: None,
            workspace: None,
        }
    }

    pub fn exists(&self) -> bool {
        self.client
            .run(&["has-session", "-t", SESSION])
            .map(|r| r.code == 0)
            .unwrap_or(false)
    }

    pub fn panes(&self) -> Result<Vec<PaneInfo>, TmuxError> {
        let res =
            self.client
                .run_checked(&["list-panes", "-a", "-s", "-t", SESSION, "-F", FORMAT])?;
        Ok(parse_panes(&res))
    }

    fn tag(&self, pane_id: &str, launch: &Launch) -> Result<(), TmuxError> {
        let argv_json = serde_json::to_string(&launch.argv).unwrap_or_default();
        let idx_str = launch.index.to_string();
        let tags = [
            ("kind", launch.kind.as_str()),
            ("role", launch.role.as_str()),
            ("index", idx_str.as_str()),
            ("label", launch.label.as_str()),
            ("session", launch.session_id.as_str()),
            ("argv", argv_json.as_str()),
            ("spec", launch.spec.as_str()),
            ("cli_version", launch.cli_version.as_str()),
        ];
        let mut batch = Vec::with_capacity(tags.len());
        for (name, val) in tags {
            batch.push(vec![
                "set-option".to_string(),
                "-p".to_string(),
                "-t".to_string(),
                pane_id.to_string(),
                format!("@tandem_{name}"),
                val.to_string(),
            ]);
        }
        self.client.run_batch_checked(&batch)?;
        Ok(())
    }

    fn register(&self, launch: &Launch, pane_id: &str) {
        if let Some(ref coord) = self.coord
            && launch.kind == "agent"
            && !launch.session_id.is_empty()
        {
            let model = self.model_of(launch);
            let cli = launch.argv.first().map(|s| s.as_str()).unwrap_or("");
            let cwd = launch.cwd.as_deref().unwrap_or("");
            let _ =
                coord.write_session(&launch.session_id, cli, &launch.role, pane_id, cwd, &model);
        }
    }

    fn model_of(&self, launch: &Launch) -> String {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&launch.spec)
            && let Some(model) = val.get("model").and_then(|m| m.as_str())
        {
            return model.to_string();
        }
        launch.label.clone()
    }

    pub fn build(
        &self,
        width: usize,
        height: usize,
        first: &Launch,
        second: &Launch,
        status: &Launch,
    ) -> Result<HashMap<String, String>, TmuxError> {
        let w_str = width.to_string();
        let h_str = height.to_string();
        let s_rows_str = STATUS_ROWS.to_string();
        let sb_cols_str = SIDEBAR_COLS.to_string();

        let (sb_launch, main_launch, stash_launch) = if first.role == "sidebar" {
            (first.clone(), second.clone(), None)
        } else {
            let sb = if let Some(ref self_cmd) = self.self_cmd {
                crate::commands::sidebar_launch(
                    Some(self_cmd),
                    self.workspace.as_deref().map(Path::new),
                )
            } else {
                Launch {
                    argv: vec!["sleep".to_string(), "60".to_string()],
                    role: "sidebar".to_string(),
                    index: 0,
                    label: "sidebar".to_string(),
                    kind: "own".to_string(),
                    env: HashMap::new(),
                    cwd: None,
                    session_id: String::new(),
                    spec: String::new(),
                    cli_version: String::new(),
                }
            };
            (sb, first.clone(), Some(second.clone()))
        };

        let mut a_args = vec![
            "new-session".to_string(),
            "-d".to_string(),
            "-s".to_string(),
            SESSION.to_string(),
            "-n".to_string(),
            "main".to_string(),
            "-x".to_string(),
            w_str,
            "-y".to_string(),
            h_str,
        ];
        a_args.extend(spawn_args(&sb_launch));
        let a_slices: Vec<&str> = a_args.iter().map(|s| s.as_str()).collect();
        let s_id = self.client.run_checked(&a_slices)?.trim().to_string();

        let mut b_args = vec![
            "split-window".to_string(),
            "-h".to_string(),
            "-t".to_string(),
            s_id.clone(),
        ];
        b_args.extend(spawn_args(&main_launch));
        let b_slices: Vec<&str> = b_args.iter().map(|s| s.as_str()).collect();
        let m_id = self.client.run_checked(&b_slices)?.trim().to_string();

        let _ = self
            .client
            .run_checked(&["resize-pane", "-t", s_id.as_str(), "-x", &sb_cols_str]);

        let mut st_args = vec![
            "split-window".to_string(),
            "-v".to_string(),
            "-f".to_string(),
            "-l".to_string(),
            s_rows_str,
            "-t".to_string(),
            format!("{SESSION}:main"),
        ];
        st_args.extend(spawn_args(status));
        let st_slices: Vec<&str> = st_args.iter().map(|s| s.as_str()).collect();
        let st_id = self.client.run_checked(&st_slices)?.trim().to_string();

        for (pane_id, launch) in [(&s_id, &sb_launch), (&m_id, &main_launch), (&st_id, status)] {
            self.tag(pane_id, launch)?;
            self.register(launch, pane_id);
        }

        let mut map = HashMap::new();
        map.insert("sidebar".to_string(), s_id.clone());
        map.insert("main".to_string(), m_id.clone());
        map.insert("status".to_string(), st_id.clone());

        if let Some(stash) = stash_launch {
            if stash.kind == "agent" {
                let sec_id = self.add_agent(&stash, false)?;
                map.insert("left".to_string(), m_id.clone());
                map.insert("right".to_string(), sec_id);
            } else {
                map.insert("left".to_string(), m_id.clone());
                map.insert("right".to_string(), m_id.clone());
            }
        } else {
            map.insert("left".to_string(), s_id.clone());
            map.insert("right".to_string(), m_id.clone());
        }

        self.apply_chrome()?;
        self.refresh()?;

        Ok(map)
    }

    pub fn apply_chrome(&self) -> Result<(), TmuxError> {
        let platform = if cfg!(target_os = "macos") {
            "darwin"
        } else {
            "linux"
        };
        let mut batch = Vec::new();
        batch.push(vec![
            "set-option".to_string(),
            "-g".to_string(),
            "status-left-length".to_string(),
            "400".to_string(),
        ]);
        let sl = status_left(platform, &default_keymap());
        batch.push(vec![
            "set-option".to_string(),
            "-g".to_string(),
            "status-left".to_string(),
            sl,
        ]);
        batch.push(vec![
            "set-option".to_string(),
            "-g".to_string(),
            "status-right".to_string(),
            "".to_string(),
        ]);

        let self_cmd_slices: Option<Vec<&str>> = self
            .self_cmd
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect());
        let self_cmd_ref = self_cmd_slices.as_deref();
        let launcher_for_quota = launcher_for(self_cmd_ref, self.workspace.as_deref());

        batch.push(vec![
            "set-option".to_string(),
            "-g".to_string(),
            "status".to_string(),
            "2".to_string(),
        ]);
        batch.push(vec![
            "set-option".to_string(),
            "-g".to_string(),
            "status-interval".to_string(),
            "10".to_string(),
        ]);
        let quota_fmt = format!(" #({launcher_for_quota} quota-line)");
        batch.push(vec![
            "set-option".to_string(),
            "-g".to_string(),
            "status-format[1]".to_string(),
            quota_fmt,
        ]);
        batch.push(vec![
            "set-option".to_string(),
            "-g".to_string(),
            "window-status-format".to_string(),
            "".to_string(),
        ]);
        batch.push(vec![
            "set-option".to_string(),
            "-g".to_string(),
            "window-status-current-format".to_string(),
            "".to_string(),
        ]);

        let km = default_keymap();
        for argv in bind_commands(self_cmd_ref, self.workspace.as_deref(), platform, &km) {
            batch.push(argv);
        }
        for argv in resize_commands(self_cmd_ref, self.workspace.as_deref()) {
            batch.push(argv);
        }
        for argv in mouse_commands(self_cmd_ref, self.workspace.as_deref(), platform, &km) {
            batch.push(argv);
        }

        let launcher = launcher_for(self_cmd_ref, self.workspace.as_deref());
        let hook_resized = run_shell(&launcher, "reflow");
        batch.push(vec![
            "set-hook".to_string(),
            "-g".to_string(),
            "client-resized".to_string(),
            hook_resized,
        ]);
        let hook_attached = run_shell(&launcher, "first-run");
        batch.push(vec![
            "set-hook".to_string(),
            "-g".to_string(),
            "client-attached".to_string(),
            hook_attached,
        ]);
        let hook_died = run_shell(&launcher, "refresh");
        batch.push(vec![
            "set-hook".to_string(),
            "-g".to_string(),
            "pane-died".to_string(),
            hook_died,
        ]);

        self.client.run_batch_checked(&batch)?;
        Ok(())
    }

    pub fn refresh(&self) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        let mut batch = Vec::new();
        for pane in &panes {
            if pane.window == "main" {
                if pane.role == "status" {
                    batch.push(vec![
                        "set-option".to_string(),
                        "-p".to_string(),
                        "-t".to_string(),
                        pane.id.clone(),
                        "pane-border-format".to_string(),
                        "─ #{?pane_active,▶ ,}STATUS ─".to_string(),
                    ]);
                } else if pane.role == "sidebar" {
                    batch.push(vec![
                        "set-option".to_string(),
                        "-p".to_string(),
                        "-t".to_string(),
                        pane.id.clone(),
                        "pane-border-format".to_string(),
                        format!(
                            "─ #{{?pane_active,▶ ,}}tandem {} ↗ ─",
                            env!("CARGO_PKG_VERSION")
                        ),
                    ]);
                } else if pane.kind == "agent" {
                    let title = crate::tmux::tabs::render_agent_title(pane);
                    batch.push(vec![
                        "set-option".to_string(),
                        "-p".to_string(),
                        "-t".to_string(),
                        pane.id.clone(),
                        "pane-border-format".to_string(),
                        title,
                    ]);
                } else if pane.role == "gridbar" {
                    batch.push(vec![
                        "set-option".to_string(),
                        "-p".to_string(),
                        "-t".to_string(),
                        pane.id.clone(),
                        "pane-border-format".to_string(),
                        "".to_string(),
                    ]);
                } else if pane.kind == "placeholder" || pane.kind == "own" {
                    let title = render_title(&panes, &pane.role);
                    batch.push(vec![
                        "set-option".to_string(),
                        "-p".to_string(),
                        "-t".to_string(),
                        pane.id.clone(),
                        "pane-border-format".to_string(),
                        title,
                    ]);
                }
            }
        }
        if !batch.is_empty() {
            self.client.run_batch_checked(&batch)?;
        }
        Ok(())
    }

    pub fn add_agent(&self, launch: &Launch, activate: bool) -> Result<String, TmuxError> {
        let stash_name = format!("stash-{}-{}", launch.role, launch.index);
        let mut args = vec![
            "new-window".to_string(),
            "-d".to_string(),
            "-t".to_string(),
            SESSION.to_string(),
            "-n".to_string(),
            stash_name,
        ];
        args.extend(spawn_args(launch));
        let slices: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let new_id = self.client.run_checked(&slices)?.trim().to_string();

        self.tag(&new_id, launch)?;
        self.register(launch, &new_id);

        let panes = self.panes()?;
        let main_slot = panes
            .iter()
            .find(|p| p.window == "main" && p.role != "sidebar" && p.role != "status");
        if let Some(shown) = main_slot
            && (activate || shown.kind != "agent")
        {
            self.client.run_checked(&[
                "swap-pane",
                "-d",
                "-s",
                new_id.as_str(),
                "-t",
                shown.id.as_str(),
            ])?;
            if shown.kind != "agent" {
                let _ = self
                    .client
                    .run_checked(&["kill-pane", "-t", shown.id.as_str()]);
            }
        }
        self.refresh()?;
        Ok(new_id)
    }

    pub fn select(&self, role: &str, index: usize) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        let target = agents_in(&panes, role)
            .into_iter()
            .find(|p| p.index == index);
        if let Some(t) = target {
            self.show_pane(&t.id)?;
        }
        Ok(())
    }

    pub fn step(&self, role: &str, delta: isize) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        let role_agents = agents_in(&panes, role);
        let mut agents: Vec<&PaneInfo> = if !role_agents.is_empty() {
            role_agents
        } else {
            panes.iter().filter(|p| p.kind == "agent").collect()
        };
        if agents.is_empty() {
            return Ok(());
        }
        agents.sort_by(|a, b| {
            let role_order = |r: &str| match r {
                "planner" => 0,
                "implementer" => 1,
                _ => 2,
            };
            role_order(&a.role)
                .cmp(&role_order(&b.role))
                .then(a.index.cmp(&b.index))
        });
        let current_vis = panes
            .iter()
            .find(|p| p.window == "main" && p.kind == "agent");
        let current_idx = current_vis
            .and_then(|c| agents.iter().position(|a| a.id == c.id))
            .unwrap_or(0);
        let len = agents.len() as isize;
        let next_idx = ((current_idx as isize + delta) % len + len) % len;
        let next_target = &agents[next_idx as usize];
        if let Some(c) = current_vis
            && c.id == next_target.id
        {
            return Ok(());
        }
        self.show_pane_with(&panes, &next_target.id)
    }

    pub fn show_pane(&self, pane_id: &str) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        self.show_pane_with(&panes, pane_id)
    }

    pub fn show_pane_with(&self, panes: &[PaneInfo], pane_id: &str) -> Result<(), TmuxError> {
        let target = match panes.iter().find(|p| p.id == pane_id && p.kind == "agent") {
            Some(t) => t,
            None => return Ok(()),
        };
        let shown = panes
            .iter()
            .find(|p| p.window == "main" && p.role != "sidebar" && p.role != "status");
        if let Some(s) = shown {
            if target.id == s.id {
                self.client
                    .run_checked(&["select-pane", "-t", target.id.as_str()])?;
                return Ok(());
            }
            let mut batch = vec![
                vec![
                    "swap-pane".to_string(),
                    "-d".to_string(),
                    "-s".to_string(),
                    target.id.clone(),
                    "-t".to_string(),
                    s.id.clone(),
                ],
                vec![
                    "select-pane".to_string(),
                    "-t".to_string(),
                    target.id.clone(),
                ],
            ];
            if s.kind == "placeholder" || s.kind == "own" {
                batch.push(vec![
                    "kill-pane".to_string(),
                    "-t".to_string(),
                    s.id.clone(),
                ]);
            }
            self.client.run_batch_checked(&batch)?;
            self.refresh()?;
        }
        Ok(())
    }

    pub fn get_selected(&self) -> Vec<String> {
        let panes = self.panes().unwrap_or_default();
        self.get_selected_from(&panes)
    }

    pub fn get_selected_from(&self, panes: &[PaneInfo]) -> Vec<String> {
        let out = self
            .client
            .run(&[
                "display-message",
                "-p",
                "-t",
                SESSION,
                "#{@tandem_selected}",
            ])
            .map(|r| r.out.trim().to_string())
            .unwrap_or_default();
        if out.is_empty() {
            if let Some(vis) = panes
                .iter()
                .find(|p| p.window == "main" && p.kind == "agent" && !p.session.is_empty())
            {
                return vec![vis.session.clone()];
            }
            Vec::new()
        } else {
            out.split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim().to_string())
                .collect()
        }
    }

    pub fn set_selected(&self, selected: &[String]) -> Result<(), TmuxError> {
        let val = selected.join(",");
        self.client
            .run_checked(&["set-option", "-t", SESSION, "@tandem_selected", &val])
            .map(|_| ())
    }

    pub fn get_grid_offset(&self) -> usize {
        let out = self
            .client
            .run(&[
                "display-message",
                "-p",
                "-t",
                SESSION,
                "#{@tandem_grid_offset}",
            ])
            .map(|r| r.out.trim().to_string())
            .unwrap_or_default();
        out.parse::<usize>().unwrap_or(0)
    }

    pub fn set_grid_offset(&self, offset: usize) -> Result<(), TmuxError> {
        let val = offset.to_string();
        self.client
            .run_checked(&["set-option", "-t", SESSION, "@tandem_grid_offset", &val])
            .map(|_| ())
    }

    pub fn select_agent(&self, target: &str, toggle: bool) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        let target_pane = match panes
            .iter()
            .find(|p| (p.id == target || p.session == target) && p.kind == "agent")
        {
            Some(p) => p,
            None => return Ok(()),
        };

        let sess = if !target_pane.session.is_empty() {
            target_pane.session.clone()
        } else {
            target_pane.id.clone()
        };

        let mut selected = self.get_selected();
        selected.retain(|s| panes.iter().any(|p| p.session == *s || p.id == *s));

        if toggle {
            if selected.contains(&sess) {
                if selected.len() > 1 {
                    selected.retain(|s| s != &sess);
                } else {
                    return Ok(());
                }
            } else {
                selected.push(sess);
            }
        } else {
            selected = vec![sess];
        }

        self.set_selected(&selected)?;

        if selected.len() == 1 {
            let only_pane = panes
                .iter()
                .find(|p| p.session == selected[0] || p.id == selected[0]);
            if let Some(p) = only_pane {
                return self.show_pane_with(&panes, &p.id);
            }
        }

        self.apply_grid_selection(&panes, &selected)
    }

    pub fn scroll_grid(&self, delta: isize) -> Result<(), TmuxError> {
        let current_offset = self.get_grid_offset();
        let new_offset = if delta < 0 {
            current_offset.saturating_sub((-delta) as usize)
        } else {
            current_offset.saturating_add(delta as usize)
        };
        let selected = self.get_selected();
        let prefs = crate::tmux::grid::GridPrefs::default();
        let plan = crate::tmux::grid::plan_grid(selected.len(), 134, 30, &prefs, 0);
        let clamped =
            crate::tmux::grid::clamp_offset(new_offset, plan.total_rows, plan.rows_per_page);
        self.set_grid_offset(clamped)?;
        let panes = self.panes()?;
        self.apply_grid_selection(&panes, &selected)
    }

    pub fn apply_grid_selection(
        &self,
        panes: &[PaneInfo],
        selected: &[String],
    ) -> Result<(), TmuxError> {
        let offset = self.get_grid_offset();
        let (sb_cols, st_rows) = self.sizes().unwrap_or((Some(26), Some(10)));
        let term_cols: usize = 160;
        let term_rows: usize = 40;
        let main_cols = term_cols.saturating_sub(sb_cols.unwrap_or(26));
        let main_rows = term_rows.saturating_sub(st_rows.unwrap_or(10));

        let prefs = crate::tmux::grid::GridPrefs::default();
        let plan =
            crate::tmux::grid::plan_grid(selected.len(), main_cols, main_rows, &prefs, offset);

        let visible_sessions: Vec<&str> = if selected.is_empty() {
            Vec::new()
        } else {
            selected[plan.visible_range.clone()]
                .iter()
                .map(|s| s.as_str())
                .collect()
        };

        if visible_sessions.len() <= 1 {
            if let Some(only_sess) = visible_sessions.first()
                && let Some(target) = panes
                    .iter()
                    .find(|p| p.session == *only_sess || p.id == *only_sess)
            {
                if target.window != "main" {
                    self.show_pane_with(panes, &target.id)?;
                }
                for p in panes
                    .iter()
                    .filter(|p| p.window == "main" && p.kind == "agent" && p.id != target.id)
                {
                    let _ = self.client.run_checked(&[
                        "break-pane",
                        "-d",
                        "-s",
                        p.id.as_str(),
                        "-n",
                        &format!("stash-{}-{}", p.role, p.index),
                    ]);
                }
            }
            if let Some(gb) = panes
                .iter()
                .find(|p| p.role == "gridbar" && p.window == "main")
            {
                let _ = self
                    .client
                    .run_checked(&["kill-pane", "-t", gb.id.as_str()]);
            }
            return self.refresh();
        }

        let mut main_anchor = panes
            .iter()
            .find(|p| p.window == "main" && p.role != "sidebar" && p.role != "status")
            .map(|p| p.id.clone());

        for sess in &visible_sessions {
            if let Some(p) = panes
                .iter()
                .find(|p| (p.session == **sess || p.id == **sess) && p.kind == "agent")
            {
                if p.window != "main" {
                    if let Some(ref anchor) = main_anchor {
                        let _ = self.client.run_checked(&[
                            "join-pane",
                            "-d",
                            "-s",
                            p.id.as_str(),
                            "-t",
                            anchor.as_str(),
                        ]);
                    }
                } else if main_anchor.is_none() {
                    main_anchor = Some(p.id.clone());
                }
            }
        }

        let updated_panes = self.panes()?;
        for p in updated_panes
            .iter()
            .filter(|p| p.window == "main" && p.kind == "agent")
        {
            let matches_visible = visible_sessions
                .iter()
                .any(|s| p.session == *s || p.id == *s);
            if !matches_visible {
                let _ = self.client.run_checked(&[
                    "break-pane",
                    "-d",
                    "-s",
                    p.id.as_str(),
                    "-n",
                    &format!("stash-{}-{}", p.role, p.index),
                ]);
            }
        }

        if plan.has_overflow {
            let has_gb = updated_panes
                .iter()
                .any(|p| p.role == "gridbar" && p.window == "main");
            if !has_gb
                && let Some(anchor) = updated_panes
                    .iter()
                    .find(|p| p.window == "main" && p.kind == "agent")
            {
                let self_cmd_slices: Option<Vec<&str>> = self
                    .self_cmd
                    .as_ref()
                    .map(|v| v.iter().map(|s| s.as_str()).collect());
                let gb_launch = crate::gridbar::gridbar_launch(
                    self_cmd_slices.as_deref(),
                    self.workspace.as_deref().map(Path::new),
                );
                let mut cmd = vec![
                    "split-window".to_string(),
                    "-d".to_string(),
                    "-h".to_string(),
                    "-t".to_string(),
                    anchor.id.clone(),
                    "-l".to_string(),
                    "1".to_string(),
                ];
                for a in &gb_launch.argv {
                    cmd.push(a.clone());
                }
                let slices: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
                if let Ok(gb_id) = self.client.run_checked(&slices) {
                    let _ = self.tag(gb_id.trim(), &gb_launch);
                }
            }
        } else if let Some(gb) = updated_panes
            .iter()
            .find(|p| p.role == "gridbar" && p.window == "main")
        {
            let _ = self
                .client
                .run_checked(&["kill-pane", "-t", gb.id.as_str()]);
        }

        let _ =
            self.client
                .run_checked(&["select-layout", "-t", &format!("{SESSION}:main"), "tiled"]);
        self.refresh()
    }

    pub fn close_agent(&self, pane_id: &str, placeholder: &Launch) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        let pane = match panes.iter().find(|p| p.id == pane_id && p.kind == "agent") {
            Some(p) => p,
            None => return Ok(()),
        };
        let mut same_role: Vec<&PaneInfo> = agents_in(&panes, &pane.role)
            .into_iter()
            .filter(|p| p.id != pane.id)
            .collect();
        let other_role: Vec<&PaneInfo> = panes
            .iter()
            .filter(|p| p.id != pane.id && p.kind == "agent" && p.role != pane.role)
            .collect();
        same_role.extend(other_role);

        let mut selected = self.get_selected_from(&panes);
        if !pane.session.is_empty() {
            selected.retain(|s| s != &pane.session);
        }
        selected.retain(|s| s != &pane.id);
        if selected.is_empty() && !same_role.is_empty() {
            let next_sess = if !same_role[0].session.is_empty() {
                same_role[0].session.clone()
            } else {
                same_role[0].id.clone()
            };
            selected.push(next_sess);
        }

        let mut batch = Vec::new();
        batch.push(vec![
            "set-option".to_string(),
            "-t".to_string(),
            SESSION.to_string(),
            "@tandem_selected".to_string(),
            selected.join(","),
        ]);

        if pane.window == "main" && same_role.is_empty() {
            let mut cmd = vec![
                "respawn-pane".to_string(),
                "-k".to_string(),
                "-t".to_string(),
                pane.id.clone(),
            ];
            for a in &placeholder.argv {
                cmd.push(a.clone());
            }
            batch.push(cmd);
            let argv_json = serde_json::to_string(&placeholder.argv).unwrap_or_default();
            let idx_str = placeholder.index.to_string();
            let tags = [
                ("kind", placeholder.kind.as_str()),
                ("role", placeholder.role.as_str()),
                ("index", idx_str.as_str()),
                ("label", placeholder.label.as_str()),
                ("session", placeholder.session_id.as_str()),
                ("argv", argv_json.as_str()),
                ("spec", placeholder.spec.as_str()),
                ("cli_version", placeholder.cli_version.as_str()),
            ];
            for (name, val) in tags {
                batch.push(vec![
                    "set-option".to_string(),
                    "-p".to_string(),
                    "-t".to_string(),
                    pane.id.clone(),
                    format!("@tandem_{name}"),
                    val.to_string(),
                ]);
            }
            self.client.run_batch_checked(&batch)?;
        } else {
            if pane.window == "main" {
                batch.push(vec![
                    "swap-pane".to_string(),
                    "-d".to_string(),
                    "-s".to_string(),
                    same_role[0].id.clone(),
                    "-t".to_string(),
                    pane.id.clone(),
                ]);
            }
            batch.push(vec![
                "kill-pane".to_string(),
                "-t".to_string(),
                pane.id.clone(),
            ]);
            self.client.run_batch_checked(&batch)?;
        }

        if let Some(ref coord) = self.coord
            && !pane.session.is_empty()
        {
            coord.remove_session(&pane.session);
        }
        self.refresh()
    }

    pub fn restart(
        &self,
        pane_id: &str,
        coord_dir: &str,
        launch: Option<&Launch>,
    ) -> Result<(), TmuxError> {
        let res = self.client.run_checked(&[
            "display-message",
            "-p",
            "-t",
            pane_id,
            "#{pane_dead}\t#{@tandem_argv}\t#{@tandem_session}\t#{@tandem_role}\t#{@tandem_cli}",
        ])?;
        let out = res.trim_end_matches('\n');
        let parts: Vec<&str> = out.split('\t').collect();
        let dead = parts.first().copied().unwrap_or("");
        if dead != "1" {
            return Ok(());
        }
        if let Some(l) = launch {
            self.respawn(pane_id, l)?;
            return Ok(());
        }

        let raw = parts.get(1).copied().unwrap_or("");
        let session = parts.get(2).copied().unwrap_or("");
        let role = parts.get(3).copied().unwrap_or("");

        let argv: Vec<String> = serde_json::from_str(raw).map_err(|_| TmuxError::Failed {
            cmd: "restart".to_string(),
            err: "pane has no restartable command".to_string(),
        })?;
        if argv.is_empty() {
            return Err(TmuxError::Failed {
                cmd: "restart".to_string(),
                err: "pane has no restartable command".to_string(),
            });
        }

        let e_sess = format!("TANDEM_SESSION_ID={session}");
        let e_role = format!("TANDEM_ROLE={role}");
        let e_cli = format!("TANDEM_CLI={}", argv[0]);
        let e_coord = format!("TANDEM_COORD_DIR={coord_dir}");

        let mut cmd = vec![
            "respawn-pane",
            "-k",
            "-t",
            pane_id,
            "-e",
            &e_sess,
            "-e",
            &e_role,
            "-e",
            &e_cli,
            "-e",
            &e_coord,
        ];
        for a in &argv {
            cmd.push(a.as_str());
        }
        self.client.run_checked(&cmd)?;
        self.refresh()
    }

    pub fn restart_live(&self, pane_id: &str, launch: &Launch) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        let agent = panes.iter().find(|p| p.id == pane_id && p.kind == "agent");
        if agent.is_none() {
            return Err(TmuxError::Failed {
                cmd: "restart-live".to_string(),
                err: format!("{pane_id} is not an agent pane"),
            });
        }
        self.respawn(pane_id, launch)
    }

    pub fn respawn(&self, pane_id: &str, launch: &Launch) -> Result<(), TmuxError> {
        let mut env_strings = Vec::new();
        let mut keys: Vec<&String> = launch.env.keys().collect();
        keys.sort();
        for k in keys {
            env_strings.push("-e".to_string());
            env_strings.push(format!("{k}={}", launch.env[k]));
        }

        let mut cmd: Vec<&str> = vec!["respawn-pane", "-k", "-t", pane_id];
        for s in &env_strings {
            cmd.push(s.as_str());
        }
        for a in &launch.argv {
            cmd.push(a.as_str());
        }
        self.client.run_checked(&cmd)?;
        self.tag(pane_id, launch)?;
        self.register(launch, pane_id);
        self.refresh()
    }

    pub fn focus(&self, role: &str) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        if role == "sidebar" {
            if let Some(sb) = panes
                .iter()
                .find(|p| p.role == "sidebar" && p.window == "main")
            {
                self.client
                    .run_checked(&["select-pane", "-t", sb.id.as_str()])?;
            }
            return Ok(());
        }
        if role == "status" {
            if let Some(st) = panes
                .iter()
                .find(|p| p.role == "status" && p.window == "main")
            {
                self.client
                    .run_checked(&["select-pane", "-t", st.id.as_str()])?;
            }
            return Ok(());
        }
        let agents = agents_in(&panes, role);
        if let Some(most_recent) = agents.last() {
            self.show_pane(&most_recent.id)?;
            self.client
                .run_checked(&["select-pane", "-t", most_recent.id.as_str()])?;
        } else if let Some(vis) = panes
            .iter()
            .find(|p| p.window == "main" && p.role != "sidebar" && p.role != "status")
        {
            self.client
                .run_checked(&["select-pane", "-t", vis.id.as_str()])?;
        }
        Ok(())
    }

    pub fn reflow(&self, width: usize, height: usize) -> Result<(), TmuxError> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        let bp = crate::tmux::grid::breakpoint(width, height);
        let meta = self
            .client
            .run(&[
                "display-message",
                "-p",
                "-t",
                SESSION,
                "#{@tandem_reflow_size} #{@tandem_breakpoint}",
            ])
            .map(|r| r.out.trim().to_string())
            .unwrap_or_default();
        let parts: Vec<&str> = meta.split_whitespace().collect();
        let current_size = format!("{width}x{height}");
        let current_bp = format!("{bp:?}");

        if parts.len() == 2 && parts[0] == current_size && parts[1] == current_bp {
            return Ok(());
        }

        let bp_changed = parts.len() < 2 || parts[1] != current_bp;
        let panes = self.panes()?;

        let mut batch = Vec::new();
        batch.push(vec![
            "set-option".to_string(),
            "-t".to_string(),
            SESSION.to_string(),
            "@tandem_reflow_size".to_string(),
            current_size,
        ]);

        if bp_changed {
            batch.push(vec![
                "set-option".to_string(),
                "-t".to_string(),
                SESSION.to_string(),
                "@tandem_breakpoint".to_string(),
                current_bp,
            ]);

            let sidebar_in_main = panes
                .iter()
                .find(|p| p.role == "sidebar" && p.window == "main");
            let sidebar_stashed = panes
                .iter()
                .find(|p| p.role == "sidebar" && p.window != "main");

            match bp {
                crate::tmux::grid::Breakpoint::Narrow => {
                    if let Some(sb) = sidebar_in_main {
                        batch.push(vec![
                            "break-pane".to_string(),
                            "-d".to_string(),
                            "-s".to_string(),
                            sb.id.clone(),
                            "-n".to_string(),
                            "stash-sidebar-0".to_string(),
                        ]);
                    }
                }
                crate::tmux::grid::Breakpoint::Medium | crate::tmux::grid::Breakpoint::Wide => {
                    if let Some(sb) = sidebar_stashed
                        && let Some(anchor) = panes
                            .iter()
                            .find(|p| p.window == "main" && p.role != "status")
                    {
                        batch.push(vec![
                            "join-pane".to_string(),
                            "-d".to_string(),
                            "-h".to_string(),
                            "-b".to_string(),
                            "-s".to_string(),
                            sb.id.clone(),
                            "-t".to_string(),
                            anchor.id.clone(),
                        ]);
                    }
                    if let Some(sb) = sidebar_in_main.or(sidebar_stashed)
                        && let Some(target_cols) = bp.default_sidebar_cols()
                    {
                        batch.push(vec![
                            "resize-pane".to_string(),
                            "-t".to_string(),
                            sb.id.clone(),
                            "-x".to_string(),
                            target_cols.to_string(),
                        ]);
                    }
                }
            }
        }

        if height > 0
            && let Some(status) = panes
                .iter()
                .find(|p| p.role == "status" && p.window == "main")
            && let Ok(Some(clamp_cmd)) = self.clamp_status_cmd(height, status)
        {
            batch.push(clamp_cmd);
        }

        self.client.run_batch_checked(&batch)?;
        Ok(())
    }

    fn clamp_status_cmd(
        &self,
        height: usize,
        status: &PaneInfo,
    ) -> Result<Option<Vec<String>>, TmuxError> {
        let res = self.client.run_checked(&[
            "display-message",
            "-p",
            "-t",
            status.id.as_str(),
            "#{pane_height}",
        ])?;
        let rows = res.trim().parse::<usize>().unwrap_or(0);
        let limit = MIN_STATUS_ROWS.max((height as f64 * MAX_STATUS_SHARE) as usize);
        if rows > limit {
            let l_str = limit.to_string();
            Ok(Some(vec![
                "resize-pane".to_string(),
                "-t".to_string(),
                status.id.clone(),
                "-y".to_string(),
                l_str,
            ]))
        } else {
            Ok(None)
        }
    }

    pub fn equalize(&self, pane_id: &str) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        let target = if pane_id.is_empty() {
            panes
                .iter()
                .find(|p| p.window == "main" && p.kind == "agent")
        } else {
            panes
                .iter()
                .find(|p| (p.id == pane_id || p.session == pane_id) && p.window == "main")
        };

        let (term_w, term_h) = self
            .client
            .run(&[
                "display-message",
                "-p",
                "-t",
                SESSION,
                "#{window_width} #{window_height}",
            ])
            .map(|r| {
                let parts: Vec<&str> = r.out.split_whitespace().collect();
                if parts.len() == 2 {
                    (
                        parts[0].parse().unwrap_or(160),
                        parts[1].parse().unwrap_or(40),
                    )
                } else {
                    (160, 40)
                }
            })
            .unwrap_or((160, 40));
        let bp = crate::tmux::grid::breakpoint(term_w, term_h);

        match target {
            Some(p) if p.role == "sidebar" => {
                let def_cols = bp
                    .default_sidebar_cols()
                    .unwrap_or(SIDEBAR_COLS)
                    .to_string();
                let _ =
                    self.client
                        .run_checked(&["resize-pane", "-t", p.id.as_str(), "-x", &def_cols]);
            }
            Some(p) if p.role == "status" => {
                let s_str = STATUS_ROWS.to_string();
                let _ =
                    self.client
                        .run_checked(&["resize-pane", "-t", p.id.as_str(), "-y", &s_str]);
            }
            _ => {
                let _ = self.client.run_checked(&[
                    "select-layout",
                    "-t",
                    &format!("{SESSION}:main"),
                    "-E",
                ]);
                if let Some(sb) = panes
                    .iter()
                    .find(|p| p.role == "sidebar" && p.window == "main")
                {
                    let def_cols = bp
                        .default_sidebar_cols()
                        .unwrap_or(SIDEBAR_COLS)
                        .to_string();
                    let _ = self.client.run_checked(&[
                        "resize-pane",
                        "-t",
                        sb.id.as_str(),
                        "-x",
                        &def_cols,
                    ]);
                }
                if let Some(st) = panes
                    .iter()
                    .find(|p| p.role == "status" && p.window == "main")
                {
                    let s_str = STATUS_ROWS.to_string();
                    let _ = self.client.run_checked(&[
                        "resize-pane",
                        "-t",
                        st.id.as_str(),
                        "-y",
                        &s_str,
                    ]);
                }
            }
        }
        self.refresh()
    }

    pub fn save_grid_prefs(
        &self,
        prefs: &crate::tmux::grid::GridPrefs,
        mode: crate::tmux::grid::SidebarMode,
    ) -> Result<(), TmuxError> {
        let cols_val = match prefs.cols {
            crate::tmux::grid::GridCols::Auto => "auto".to_string(),
            crate::tmux::grid::GridCols::Fixed(n) => n.to_string(),
        };
        let rows_val = match prefs.rows {
            crate::tmux::grid::GridRows::Auto => "auto".to_string(),
            crate::tmux::grid::GridRows::Fixed(n) => n.to_string(),
        };
        let fill_val = match prefs.fill {
            crate::tmux::grid::FillOrder::RowFirst => "row",
            crate::tmux::grid::FillOrder::ColumnFirst => "column",
        };
        let mode_val = match mode {
            crate::tmux::grid::SidebarMode::Auto => "auto",
            crate::tmux::grid::SidebarMode::Wide => "wide",
            crate::tmux::grid::SidebarMode::Compact => "compact",
            crate::tmux::grid::SidebarMode::Hidden => "hidden",
        };

        let batch = [
            vec![
                "set-option".into(),
                "-t".into(),
                SESSION.into(),
                "@tandem_grid_cols".into(),
                cols_val,
            ],
            vec![
                "set-option".into(),
                "-t".into(),
                SESSION.into(),
                "@tandem_grid_rows".into(),
                rows_val,
            ],
            vec![
                "set-option".into(),
                "-t".into(),
                SESSION.into(),
                "@tandem_grid_fill".into(),
                fill_val.into(),
            ],
            vec![
                "set-option".into(),
                "-t".into(),
                SESSION.into(),
                "@tandem_sidebar_mode".into(),
                mode_val.into(),
            ],
        ];
        self.client.run_batch_checked(&batch)?;
        Ok(())
    }

    pub fn reset_ui(&self) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        let active_agent = panes
            .iter()
            .find(|p| p.window == "main" && p.kind == "agent")
            .map(|p| {
                if !p.session.is_empty() {
                    p.session.clone()
                } else {
                    p.id.clone()
                }
            })
            .unwrap_or_default();

        let sidebar = panes
            .iter()
            .find(|p| p.role == "sidebar" && p.window == "main");
        let status = panes
            .iter()
            .find(|p| p.role == "status" && p.window == "main");

        let mut batch: Vec<Vec<String>> = vec![
            vec![
                "set-option".into(),
                "-t".into(),
                SESSION.into(),
                "@tandem_grid_cols".into(),
                "auto".into(),
            ],
            vec![
                "set-option".into(),
                "-t".into(),
                SESSION.into(),
                "@tandem_grid_rows".into(),
                "auto".into(),
            ],
            vec![
                "set-option".into(),
                "-t".into(),
                SESSION.into(),
                "@tandem_grid_fill".into(),
                "row".into(),
            ],
            vec![
                "set-option".into(),
                "-t".into(),
                SESSION.into(),
                "@tandem_sidebar_mode".into(),
                "auto".into(),
            ],
            vec![
                "set-option".into(),
                "-t".into(),
                SESSION.into(),
                "@tandem_grid_offset".into(),
                "0".into(),
            ],
            vec![
                "set-option".into(),
                "-t".into(),
                SESSION.into(),
                "@tandem_selected".into(),
                active_agent,
            ],
        ];

        let s_str = STATUS_ROWS.to_string();
        if let Some(s) = status {
            batch.push(vec![
                "resize-pane".into(),
                "-t".into(),
                s.id.clone(),
                "-y".into(),
                s_str,
            ]);
        }
        let sb_str = SIDEBAR_COLS.to_string();
        if let Some(sb) = sidebar {
            batch.push(vec![
                "resize-pane".into(),
                "-t".into(),
                sb.id.clone(),
                "-x".into(),
                sb_str,
            ]);
        }

        self.client.run_batch_checked(&batch)?;

        self.reflow(0, 0)?;
        Ok(())
    }

    pub fn reset(&self) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        let sidebar = panes
            .iter()
            .find(|p| p.role == "sidebar" && p.window == "main");
        let status = panes
            .iter()
            .find(|p| p.role == "status" && p.window == "main");

        if let Some(s) = status {
            let s_str = STATUS_ROWS.to_string();
            let _ = self
                .client
                .run_checked(&["resize-pane", "-t", s.id.as_str(), "-y", &s_str]);
        }
        if let Some(sb) = sidebar {
            let sb_str = SIDEBAR_COLS.to_string();
            let _ = self
                .client
                .run_checked(&["resize-pane", "-t", sb.id.as_str(), "-x", &sb_str]);
        }
        Ok(())
    }

    pub fn write_conf(&self, path: &Path) -> Result<PathBuf, std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, conf_text(50000))?;
        Ok(path.to_path_buf())
    }

    pub fn sizes(&self) -> Result<(Option<usize>, Option<usize>), TmuxError> {
        let panes = self.panes()?;
        let sidebar = panes
            .iter()
            .find(|p| p.role == "sidebar" && p.window == "main");
        let status = panes
            .iter()
            .find(|p| p.role == "status" && p.window == "main");

        let target_window = format!("{SESSION}:main");
        let out = self.client.run_checked(&[
            "list-panes",
            "-t",
            &target_window,
            "-F",
            "#{pane_id} #{pane_width} #{pane_height}",
        ])?;
        let mut map: HashMap<String, (usize, usize)> = HashMap::new();
        for line in out.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 3
                && let (Ok(w), Ok(h)) = (parts[1].parse::<usize>(), parts[2].parse::<usize>())
            {
                map.insert(parts[0].to_string(), (w, h));
            }
        }

        let sidebar_cols = sidebar.and_then(|s| map.get(&s.id).map(|(w, _)| *w));
        let status_rows = status.and_then(|s| map.get(&s.id).map(|(_, h)| *h));
        Ok((sidebar_cols, status_rows))
    }

    pub fn apply_sizes(
        &self,
        sidebar_cols_or_percent: Option<usize>,
        status_rows: Option<usize>,
    ) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        let sidebar = panes
            .iter()
            .find(|p| p.role == "sidebar" && p.window == "main");
        let status = panes
            .iter()
            .find(|p| p.role == "status" && p.window == "main");

        if let (Some(s), Some(sr)) = (status, status_rows) {
            let sr_str = sr.to_string();
            let _ = self
                .client
                .run_checked(&["resize-pane", "-t", s.id.as_str(), "-y", &sr_str]);
        }

        if let (Some(cols), Some(sb)) = (sidebar_cols_or_percent, sidebar) {
            let val = cols.to_string();
            let _ = self
                .client
                .run_checked(&["resize-pane", "-t", sb.id.as_str(), "-x", &val]);
        }
        Ok(())
    }

    pub fn respawn_own(&self, launches: &HashMap<String, Launch>) -> Result<(), TmuxError> {
        for pane in self.panes()? {
            let key = if pane.role == "status" {
                "status".to_string()
            } else if pane.role == "sidebar" {
                "sidebar".to_string()
            } else {
                format!("placeholder-{}", pane.role)
            };
            if let Some(launch) = launches.get(&key)
                && pane.kind == "own"
            {
                let mut cmd = vec!["respawn-pane", "-k", "-t", pane.id.as_str()];
                for a in &launch.argv {
                    cmd.push(a.as_str());
                }
                self.client.run_checked(&cmd)?;
            }
        }
        Ok(())
    }
}
