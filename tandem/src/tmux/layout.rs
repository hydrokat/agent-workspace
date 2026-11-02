use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::coordination::store::CoordStore;
use crate::tmux::client::{TmuxClient, TmuxError};
use crate::tmux::keys::{
    bind_commands, default_keymap, launcher_for, mouse_commands, resize_commands, run_shell,
    status_left,
};
use crate::tmux::tabs::{
    FORMAT, PaneInfo, agents_in, neighbor, parse_panes, render_title, visible_pane,
};

pub const SESSION: &str = "tandem";
pub const STATUS_ROWS: usize = 10;
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
    pub python: Option<String>,
    pub self_cmd: Option<Vec<String>>,
    pub coord: Option<CoordStore>,
    pub workspace: Option<String>,
}

impl Layout {
    pub fn new(client: &TmuxClient) -> Self {
        Self {
            client: client.clone(),
            python: None,
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
        for (name, val) in tags {
            self.client.run_checked(&[
                "set-option",
                "-p",
                "-t",
                pane_id,
                &format!("@tandem_{name}"),
                val,
            ])?;
        }
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
        left: &Launch,
        right: &Launch,
        status: &Launch,
    ) -> Result<HashMap<String, String>, TmuxError> {
        let w_str = width.to_string();
        let h_str = height.to_string();
        let s_rows_str = STATUS_ROWS.to_string();

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
        a_args.extend(spawn_args(left));
        let a_slices: Vec<&str> = a_args.iter().map(|s| s.as_str()).collect();
        let a = self.client.run_checked(&a_slices)?.trim().to_string();

        let mut b_args = vec![
            "split-window".to_string(),
            "-h".to_string(),
            "-t".to_string(),
            a.clone(),
        ];
        b_args.extend(spawn_args(right));
        let b_slices: Vec<&str> = b_args.iter().map(|s| s.as_str()).collect();
        let b = self.client.run_checked(&b_slices)?.trim().to_string();

        let mut s_args = vec![
            "split-window".to_string(),
            "-v".to_string(),
            "-f".to_string(),
            "-l".to_string(),
            s_rows_str,
            "-t".to_string(),
            format!("{SESSION}:main"),
        ];
        s_args.extend(spawn_args(status));
        let s_slices: Vec<&str> = s_args.iter().map(|s| s.as_str()).collect();
        let s = self.client.run_checked(&s_slices)?.trim().to_string();

        for (pane_id, launch) in [(&a, left), (&b, right), (&s, status)] {
            self.tag(pane_id, launch)?;
            self.register(launch, pane_id);
        }

        self.apply_chrome()?;
        self.refresh()?;

        let mut map = HashMap::new();
        map.insert("left".to_string(), a);
        map.insert("right".to_string(), b);
        map.insert("status".to_string(), s);
        Ok(map)
    }

    pub fn apply_chrome(&self) -> Result<(), TmuxError> {
        let platform = if cfg!(target_os = "macos") {
            "darwin"
        } else {
            "linux"
        };
        self.client
            .run_checked(&["set-option", "-g", "status-left-length", "400"])?;
        let sl = status_left(platform, &default_keymap());
        self.client
            .run_checked(&["set-option", "-g", "status-left", &sl])?;
        self.client
            .run_checked(&["set-option", "-g", "status-right", ""])?;

        let self_cmd_slices: Option<Vec<&str>> = self
            .self_cmd
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect());
        let self_cmd_ref = self_cmd_slices.as_deref();
        let launcher_for_quota = launcher_for(
            self_cmd_ref,
            self.workspace.as_deref(),
            self.python.as_deref(),
        );

        self.client
            .run_checked(&["set-option", "-g", "status", "2"])?;
        self.client
            .run_checked(&["set-option", "-g", "status-interval", "10"])?;
        let quota_fmt = format!(" #({launcher_for_quota} quota-line)");
        self.client
            .run_checked(&["set-option", "-g", "status-format[1]", &quota_fmt])?;
        self.client
            .run_checked(&["set-option", "-g", "window-status-format", ""])?;
        self.client
            .run_checked(&["set-option", "-g", "window-status-current-format", ""])?;

        let km = default_keymap();
        for argv in bind_commands(
            self_cmd_ref,
            self.workspace.as_deref(),
            platform,
            self.python.as_deref(),
            &km,
        ) {
            let slices: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
            self.client.run_checked(&slices)?;
        }
        for argv in resize_commands(
            self_cmd_ref,
            self.workspace.as_deref(),
            self.python.as_deref(),
        ) {
            let slices: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
            self.client.run_checked(&slices)?;
        }
        for argv in mouse_commands(
            self_cmd_ref,
            self.workspace.as_deref(),
            platform,
            self.python.as_deref(),
            &km,
        ) {
            let slices: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
            self.client.run_checked(&slices)?;
        }

        let launcher = launcher_for(
            self_cmd_ref,
            self.workspace.as_deref(),
            self.python.as_deref(),
        );
        let hook_resized = run_shell(&launcher, "reflow");
        self.client
            .run_checked(&["set-hook", "-g", "client-resized", &hook_resized])?;
        let hook_attached = run_shell(&launcher, "first-run");
        self.client
            .run_checked(&["set-hook", "-g", "client-attached", &hook_attached])?;
        let hook_died = run_shell(&launcher, "refresh");
        self.client
            .run_checked(&["set-hook", "-g", "pane-died", &hook_died])?;
        Ok(())
    }

    pub fn refresh(&self) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        for pane in &panes {
            if pane.role == "status" {
                self.client.run_checked(&[
                    "set-option",
                    "-p",
                    "-t",
                    pane.id.as_str(),
                    "pane-border-format",
                    "─ #{?pane_active,▶ ,}STATUS ─",
                ])?;
            }
        }
        for role in ROLES {
            if let Some(shown) = visible_pane(&panes, role) {
                let title = render_title(&panes, role);
                self.client.run_checked(&[
                    "set-option",
                    "-p",
                    "-t",
                    shown.id.as_str(),
                    "pane-border-format",
                    &title,
                ])?;
            }
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
        if let Some(shown) = visible_pane(&panes, &launch.role)
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
                self.client
                    .run_checked(&["kill-pane", "-t", shown.id.as_str()])?;
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
        self.show(&panes, role, target)
    }

    pub fn step(&self, role: &str, delta: isize) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        self.show(&panes, role, neighbor(&panes, role, delta))
    }

    fn show(
        &self,
        panes: &[PaneInfo],
        role: &str,
        target: Option<&PaneInfo>,
    ) -> Result<(), TmuxError> {
        let shown = visible_pane(panes, role);
        if let (Some(t), Some(s)) = (target, shown) {
            if t.id == s.id {
                return Ok(());
            }
            self.client.run_checked(&[
                "swap-pane",
                "-d",
                "-s",
                t.id.as_str(),
                "-t",
                s.id.as_str(),
            ])?;
            self.client
                .run_checked(&["select-pane", "-t", t.id.as_str()])?;
            self.refresh()?;
        }
        Ok(())
    }

    pub fn close_agent(&self, pane_id: &str, placeholder: &Launch) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        let pane = match panes.iter().find(|p| p.id == pane_id && p.kind == "agent") {
            Some(p) => p,
            None => return Ok(()),
        };
        let others: Vec<&PaneInfo> = agents_in(&panes, &pane.role)
            .into_iter()
            .filter(|p| p.id != pane.id)
            .collect();

        if pane.window == "main" && others.is_empty() {
            let mut cmd = vec!["respawn-pane", "-k", "-t", pane.id.as_str()];
            for a in &placeholder.argv {
                cmd.push(a.as_str());
            }
            self.client.run_checked(&cmd)?;
            self.tag(&pane.id, placeholder)?;
        } else {
            if pane.window == "main" {
                self.client.run_checked(&[
                    "swap-pane",
                    "-d",
                    "-s",
                    others[0].id.as_str(),
                    "-t",
                    pane.id.as_str(),
                ])?;
            }
            self.client
                .run_checked(&["kill-pane", "-t", pane.id.as_str()])?;
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
        if let Some(target) = visible_pane(&panes, role) {
            self.client
                .run_checked(&["select-pane", "-t", target.id.as_str()])?;
        }
        Ok(())
    }

    pub fn reflow(&self, width: usize, height: usize) -> Result<(), TmuxError> {
        self.restack(width)?;
        if height > 0 {
            self.clamp_status(height)?;
        }
        Ok(())
    }

    fn clamp_status(&self, height: usize) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        let status = match panes.iter().find(|p| p.role == "status") {
            Some(s) => s,
            None => return Ok(()),
        };
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
            self.client
                .run_checked(&["resize-pane", "-t", status.id.as_str(), "-y", &l_str])?;
        }
        Ok(())
    }

    pub fn reset(&self) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        let left = visible_pane(&panes, "planner");
        let right = visible_pane(&panes, "implementer");
        let status = panes.iter().find(|p| p.role == "status");

        let target_window = format!("{SESSION}:main");
        let size_res = self.client.run_checked(&[
            "display-message",
            "-p",
            "-t",
            &target_window,
            "#{window_width} #{window_height}",
        ])?;
        let size: Vec<&str> = size_res.split_whitespace().collect();
        if size.len() != 2 || left.is_none() || right.is_none() {
            return Ok(());
        }
        let width = size[0].parse::<usize>().unwrap_or(0);
        let height = size[1].parse::<usize>().unwrap_or(0);
        if width == 0 || height == 0 {
            return Ok(());
        }
        let left = left.unwrap();
        let right = right.unwrap();

        if let Some(s) = status {
            let status_limit =
                STATUS_ROWS.min(MIN_STATUS_ROWS.max((height as f64 * MAX_STATUS_SHARE) as usize));
            let s_str = status_limit.to_string();
            self.client
                .run_checked(&["resize-pane", "-t", s.id.as_str(), "-y", &s_str])?;
        }

        let out = self.client.run_checked(&[
            "list-panes",
            "-t",
            &target_window,
            "-F",
            "#{pane_id} #{pane_left}",
        ])?;
        let mut lefts: HashMap<String, String> = HashMap::new();
        for line in out.lines() {
            if let Some((pid, pleft)) = line.split_once(' ') {
                lefts.insert(pid.to_string(), pleft.to_string());
            }
        }

        if lefts.get(&left.id) == lefts.get(&right.id) {
            let avail = height.saturating_sub(if status.is_some() { STATUS_ROWS } else { 0 });
            let half = (avail / 2).max(3).to_string();
            self.client
                .run_checked(&["resize-pane", "-t", left.id.as_str(), "-y", &half])?;
        } else {
            let half = (width / 2).to_string();
            self.client
                .run_checked(&["resize-pane", "-t", left.id.as_str(), "-x", &half])?;
        }
        Ok(())
    }

    fn restack(&self, width: usize) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        let left = visible_pane(&panes, "planner");
        let right = visible_pane(&panes, "implementer");
        if let (Some(l), Some(r)) = (left, right) {
            let target_window = format!("{SESSION}:main");
            let out = self.client.run_checked(&[
                "list-panes",
                "-t",
                &target_window,
                "-F",
                "#{pane_id} #{pane_left}",
            ])?;
            let mut lefts: HashMap<String, String> = HashMap::new();
            for line in out.lines() {
                if let Some((pid, pleft)) = line.split_once(' ') {
                    lefts.insert(pid.to_string(), pleft.to_string());
                }
            }
            let stacked = lefts.get(&l.id) == lefts.get(&r.id);
            if width < STACK_BELOW_COLS && !stacked {
                self.client.run_checked(&[
                    "move-pane",
                    "-v",
                    "-s",
                    r.id.as_str(),
                    "-t",
                    l.id.as_str(),
                ])?;
            } else if width >= STACK_BELOW_COLS && stacked {
                self.client.run_checked(&[
                    "move-pane",
                    "-h",
                    "-s",
                    r.id.as_str(),
                    "-t",
                    l.id.as_str(),
                ])?;
            }
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
        let left = visible_pane(&panes, "planner");
        let right = visible_pane(&panes, "implementer");
        let status = panes.iter().find(|p| p.role == "status");

        let target_window = format!("{SESSION}:main");
        let out = self.client.run_checked(&[
            "list-panes",
            "-t",
            &target_window,
            "-F",
            "#{pane_id} #{pane_left} #{pane_width} #{pane_height} #{window_width}",
        ])?;
        let mut rows: HashMap<String, Vec<usize>> = HashMap::new();
        for line in out.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 5 {
                let nums: Result<Vec<usize>, _> =
                    parts[1..].iter().map(|s| s.parse::<usize>()).collect();
                if let Ok(ns) = nums {
                    rows.insert(parts[0].to_string(), ns);
                }
            }
        }

        let mut percent = None;
        if let (Some(l), Some(r)) = (left, right)
            && let (Some(l_row), Some(r_row)) = (rows.get(&l.id), rows.get(&r.id))
            && l_row[0] != r_row[0]
        {
            let w_width = l_row[3].max(1);
            percent = Some(((100.0 * l_row[1] as f64) / w_width as f64).round() as usize);
        }
        let status_rows = status.and_then(|s| rows.get(&s.id).map(|r| r[2]));
        Ok((percent, status_rows))
    }

    pub fn apply_sizes(
        &self,
        columns_percent: Option<usize>,
        status_rows: Option<usize>,
    ) -> Result<(), TmuxError> {
        let panes = self.panes()?;
        let left = visible_pane(&panes, "planner");
        let right = visible_pane(&panes, "implementer");
        let status = panes.iter().find(|p| p.role == "status");

        if let (Some(s), Some(sr)) = (status, status_rows) {
            let sr_str = sr.to_string();
            let _ = self
                .client
                .run_checked(&["resize-pane", "-t", s.id.as_str(), "-y", &sr_str]);
        }

        if let (Some(cp), Some(l), Some(r)) = (columns_percent, left, right) {
            let target_window = format!("{SESSION}:main");
            let out = self.client.run_checked(&[
                "list-panes",
                "-t",
                &target_window,
                "-F",
                "#{pane_id} #{pane_left}",
            ])?;
            let mut lefts: HashMap<String, String> = HashMap::new();
            for line in out.lines() {
                if let Some((pid, pleft)) = line.split_once(' ') {
                    lefts.insert(pid.to_string(), pleft.to_string());
                }
            }
            if lefts.get(&l.id) != lefts.get(&r.id) {
                let val = format!("{cp}%");
                let _ = self
                    .client
                    .run_checked(&["resize-pane", "-t", l.id.as_str(), "-x", &val]);
            }
        }
        Ok(())
    }

    pub fn respawn_own(&self, launches: &HashMap<String, Launch>) -> Result<(), TmuxError> {
        for pane in self.panes()? {
            let key = if pane.role == "status" {
                "status".to_string()
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
