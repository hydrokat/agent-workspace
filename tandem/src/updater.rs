use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::selfexec::is_frozen;
use crate::tmux::tabs::PaneInfo;

pub type UpdateRunner<'a> = dyn Fn(&[&str], &Path, Duration) -> (i32, String) + 'a;
pub type WhichFn<'a> = dyn Fn(&str) -> Option<PathBuf> + 'a;

pub const UPDATE_TIMEOUT: Duration = Duration::from_secs(900);

pub fn default_streaming_runner(argv: &[&str], cwd: &Path, _timeout: Duration) -> (i32, String) {
    if argv.is_empty() {
        return (1, String::new());
    }
    let mut cmd = Command::new(argv[0]);
    if argv.len() > 1 {
        cmd.args(&argv[1..]);
    }
    cmd.current_dir(cwd);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    match cmd.spawn() {
        Ok(mut child) => {
            let mut out = String::new();
            if let Some(stdout) = child.stdout.take() {
                use std::io::{BufRead, BufReader};
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    let _ = writeln!(std::io::stdout(), "{line}");
                    let _ = std::io::stdout().flush();
                    out.push_str(&line);
                    out.push('\n');
                }
            }
            let status = child.wait().map(|s| s.code().unwrap_or(1)).unwrap_or(1);
            (status, out.trim().to_string())
        }
        Err(err) => (127, err.to_string()),
    }
}

pub fn default_quiet_runner(argv: &[&str], cwd: &Path, _timeout: Duration) -> (i32, String) {
    if argv.is_empty() {
        return (1, String::new());
    }
    let mut cmd = Command::new(argv[0]);
    if argv.len() > 1 {
        cmd.args(&argv[1..]);
    }
    cmd.current_dir(cwd);
    cmd.stdin(Stdio::null());
    match cmd.output() {
        Ok(out) => {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (out.status.code().unwrap_or(1), s)
        }
        Err(err) => (1, err.to_string()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateResult {
    pub ok: bool,
    pub message: String,
    pub steps: Vec<String>,
    pub reloaded: bool,
    pub old_version: String,
    pub new_version: String,
}

pub fn installed_dir(workspace: &Path) -> PathBuf {
    workspace.join(".agent-workspace").join("tandem")
}

pub fn running_from_workspace(workspace: &Path) -> bool {
    installed_dir(workspace).is_dir()
}

pub fn plan(workspace: &Path, spec: &str, frozen: Option<bool>) -> Vec<String> {
    let is_frz = frozen.unwrap_or_else(|| is_frozen() || running_from_workspace(workspace));
    let mut lines = vec![format!(
        "1. npx --yes {spec} update --yes --no-link   (refreshes skills, agents, run.sh and tandem, and updates the installed AI CLIs: claude update, agy update, codex through its package manager)"
    )];
    if is_frz {
        lines.push("2. run.sh --fetch-runtime   (refreshes the tandem binary runtime)".to_string());
        lines.push(
            "3. restart tandem's own windows (status, placeholders) on the new code".to_string(),
        );
    } else {
        lines.push(
            "2. tandem is running from a checkout, not the workspace copy: update that with git yourself"
                .to_string(),
        );
    }
    lines.push("Your agents keep running. Nothing is typed into their panes.".to_string());
    lines
}

#[allow(clippy::too_many_arguments)]
pub fn run_update(
    workspace: &Path,
    spec: &str,
    mut reload: impl FnMut() -> bool,
    runner: Option<&UpdateRunner>,
    quiet_runner: Option<&UpdateRunner>,
    which: Option<&WhichFn>,
    frozen: Option<bool>,
    state_dir_override: Option<&Path>,
) -> UpdateResult {
    let old_version = env!("CARGO_PKG_VERSION").to_string();
    let mut result = UpdateResult {
        ok: false,
        message: String::new(),
        steps: Vec::new(),
        reloaded: false,
        old_version: old_version.clone(),
        new_version: String::new(),
    };

    let check_which = |cmd: &str| -> Option<PathBuf> {
        if let Some(w) = which {
            w(cmd)
        } else {
            crate::versions::default_which(cmd)
        }
    };

    if check_which("npx").is_none() {
        result.message = format!(
            "npx not found. Install Node.js (>= 18) to update, or run `npx {spec} update` yourself."
        );
        return result;
    }

    let min_file = installed_dir(workspace).join("min-versions.env");
    let before = std::fs::read(&min_file).ok();

    let default_runner = default_streaming_runner;
    let run_cmd = runner.unwrap_or(&default_runner);
    let default_quiet = default_quiet_runner;
    let run_quiet = quiet_runner.unwrap_or(&default_quiet);

    let (code, note) = run_cmd(
        &["npx", "--yes", spec, "update", "--yes", "--no-link"],
        workspace,
        UPDATE_TIMEOUT,
    );
    if code != 0 {
        let note_suffix = if note.is_empty() {
            String::new()
        } else {
            format!(": {note}")
        };
        result.message = format!(
            "agent-workspace update failed (exit {code}{note_suffix}). Nothing was restarted."
        );
        return result;
    }
    result.steps.push("workspace updated".to_string());

    let is_frz = frozen.unwrap_or_else(|| is_frozen() || running_from_workspace(workspace));
    if !is_frz {
        result.ok = true;
        result.message =
            "Workspace updated. tandem runs from a checkout, so update that copy with git."
                .to_string();
        return result;
    }

    let run_sh = workspace.join("run.sh");
    let run_sh_str = run_sh.to_string_lossy();
    let (code, out) = run_cmd(&[&run_sh_str, "--fetch-runtime"], workspace, UPDATE_TIMEOUT);
    if code != 0 {
        let reason = if out.is_empty() {
            format!("exit {code}")
        } else {
            out
        };
        result.message = format!(
            "Workspace updated, but the new tandem binary could not be fetched ({reason}). Quit and run ./run.sh to finish (it will fall back to Python if needed)."
        );
        return result;
    }

    let mut new_self = String::new();
    for line in out.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("TANDEM_SELF=") {
            new_self = val.trim().to_string();
            break;
        }
    }

    if new_self.is_empty() {
        result.message =
            "Workspace updated, but the new tandem binary could not be fetched (TANDEM_SELF not reported). Quit and run ./run.sh to finish (it will fall back to Python if needed)."
                .to_string();
        return result;
    }

    let self_path = PathBuf::from(&new_self);
    let state_dir = if let Some(dir) = state_dir_override {
        dir.to_path_buf()
    } else {
        let state_dir_str = std::env::var("TANDEM_STATE_DIR").unwrap_or_else(|_| {
            std::env::var("XDG_STATE_HOME")
                .map(|s| format!("{s}/tandem"))
                .unwrap_or_else(|_| {
                    std::env::var("HOME")
                        .map(|h| format!("{h}/.local/state/tandem"))
                        .unwrap_or_else(|_| "/tmp/tandem".to_string())
                })
        });
        PathBuf::from(state_dir_str)
    };
    let expected_base = state_dir.join("bin");

    let canonical_self = self_path
        .canonicalize()
        .unwrap_or_else(|_| self_path.clone());
    let canonical_expected = expected_base.canonicalize().unwrap_or(expected_base);
    let in_expected_base = canonical_self.starts_with(&canonical_expected)
        || self_path.starts_with(&canonical_expected);

    if !in_expected_base || !self_path.is_file() {
        result.message =
            "Workspace updated, but the new tandem binary could not be fetched (invalid binary path). Quit and run ./run.sh to finish (it will fall back to Python if needed)."
                .to_string();
        return result;
    }

    let (_, version_out) = run_quiet(
        &[&new_self, "--version"],
        workspace,
        Duration::from_secs(30),
    );
    let v_str = version_out.trim();
    let parsed_ver = v_str.strip_prefix("tandem ").unwrap_or(v_str).trim();
    result.new_version = parsed_ver.to_string();

    if !result.new_version.is_empty()
        && !result.old_version.is_empty()
        && result.new_version != result.old_version
    {
        result.steps.push(format!(
            "binary runtime fetched ({} → {})",
            result.old_version, result.new_version
        ));
    } else {
        result.steps.push("binary runtime fetched".to_string());
    }

    crate::selfexec::set_self_binary(&new_self);
    result.reloaded = reload();
    if result.reloaded {
        result.steps.push("tandem windows restarted".to_string());
    } else {
        result
            .steps
            .push("no running session to restart".to_string());
    }

    let changed = std::fs::read(&min_file).ok() != before;
    result.ok = true;
    let mut msg = if !result.new_version.is_empty() {
        format!(
            "Updated tandem {} → {}.",
            result.old_version, result.new_version
        )
    } else {
        "Updated tandem.".to_string()
    };
    if changed {
        msg.push_str(
            " Minimum dependency versions changed: quit and start with ./run.sh to re-check them.",
        );
    }
    result.message = msg;
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleAgent {
    pub pane_id: String,
    pub role: String,
    pub index: usize,
    pub cli: String,
    pub model: String,
    pub launched: String,
    pub installed: String,
    pub resumable: bool,
    pub busy: bool,
    pub claim: String,
}

fn cli_and_model(pane: &PaneInfo) -> (String, String) {
    if let Ok(raw) = serde_json::from_str::<serde_json::Value>(&pane.spec) {
        let cli = raw
            .get("cli")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let model = raw
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        (cli, model)
    } else {
        (String::new(), String::new())
    }
}

pub fn stale_agents(
    panes: &[PaneInfo],
    mut installed: impl FnMut(&str) -> String,
    mut resumable: impl FnMut(&PaneInfo) -> bool,
    busy: &HashMap<String, String>,
) -> Vec<StaleAgent> {
    let mut versions: HashMap<String, String> = HashMap::new();
    let mut found = Vec::new();

    for pane in panes {
        if pane.kind != "agent" {
            continue;
        }
        let (cli, model) = cli_and_model(pane);
        if cli.is_empty() {
            continue;
        }
        if !versions.contains_key(&cli) {
            let v = installed(&cli);
            versions.insert(cli.clone(), v);
        }
        let inst_ver = versions.get(&cli).cloned().unwrap_or_default();
        if inst_ver.is_empty() || pane.cli_version == inst_ver {
            continue;
        }
        let is_busy = busy.contains_key(&pane.session);
        let claim_str = busy.get(&pane.session).cloned().unwrap_or_default();
        found.push(StaleAgent {
            pane_id: pane.id.clone(),
            role: pane.role.clone(),
            index: pane.index,
            cli,
            model,
            launched: pane.cli_version.clone(),
            installed: inst_ver,
            resumable: resumable(pane),
            busy: is_busy,
            claim: claim_str,
        });
    }

    found
}

pub fn default_selection(stale: &[StaleAgent]) -> HashSet<String> {
    stale
        .iter()
        .filter(|s| s.resumable && !s.busy)
        .map(|s| s.pane_id.clone())
        .collect()
}

pub fn describe_stale(agent: &StaleAgent) -> String {
    let note = if agent.busy {
        format!("busy (claim {})", agent.claim)
    } else if agent.resumable {
        "resumes conversation".to_string()
    } else {
        "would start a new chat".to_string()
    };
    let launched = if agent.launched.is_empty() {
        "unknown"
    } else {
        &agent.launched
    };
    format!(
        "{} {}  {}·{}  {} → {}  {}",
        agent.role, agent.index, agent.cli, agent.model, launched, agent.installed, note
    )
}

pub fn choose_restarts(
    stale: &[StaleAgent],
    ask: &mut dyn FnMut(&str) -> Option<String>,
    out: &mut dyn FnMut(&str),
) -> Vec<StaleAgent> {
    let mut selected = default_selection(stale);
    out("These agents still run an older CLI:");
    if stale.iter().any(|s| s.cli == "agy") {
        out(
            "(agy: the update may replace its binary under a running agent, so a restart is advised.)",
        );
    }

    loop {
        for (idx, agent) in stale.iter().enumerate() {
            let mark = if selected.contains(&agent.pane_id) {
                'x'
            } else {
                ' '
            };
            out(&format!("  [{mark}] {} {}", idx + 1, describe_stale(agent)));
        }
        out("Restart: a=all  numbers=toggle  Enter=restart selected  n=none");

        let answer = match ask("> ") {
            Some(s) => s.trim().to_lowercase(),
            None => return Vec::new(),
        };

        if answer.is_empty() {
            return stale
                .iter()
                .filter(|s| selected.contains(&s.pane_id))
                .cloned()
                .collect();
        }

        if answer == "n" {
            return Vec::new();
        }

        for token in answer.replace(',', " ").split_whitespace() {
            if token == "a" {
                selected = stale.iter().map(|s| s.pane_id.clone()).collect();
            } else if let Ok(num) = token.parse::<usize>()
                && num >= 1
                && num <= stale.len()
            {
                let pid = &stale[num - 1].pane_id;
                if selected.contains(pid) {
                    selected.remove(pid);
                } else {
                    selected.insert(pid.clone());
                }
            }
        }
    }
}
