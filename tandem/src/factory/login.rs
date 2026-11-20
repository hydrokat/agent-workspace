use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tmux::SESSION;
use crate::tmux::client::{TmuxClient, TmuxError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginSpec {
    pub cli: &'static str,
    pub status_argv: &'static [&'static str],
    pub login_argv: Option<&'static [&'static str]>,
    pub login_hint: Option<&'static str>,
}

// Verified 2026-11-20 against installed CLIs' `--help` output:
// - `claude auth --help` exposes `status --json` and `login`
// - `agy --help` has no auth/login subcommand; `agy models` probes auth and users sign in in agy's own TUI
// - `opencode auth --help` exposes `list` and `login`
pub const LOGIN_SPECS: &[LoginSpec] = &[
    LoginSpec {
        cli: "claude",
        status_argv: &["claude", "auth", "status", "--json"],
        login_argv: Some(&["claude", "auth", "login"]),
        login_hint: None,
    },
    LoginSpec {
        cli: "agy",
        status_argv: &["agy", "models"],
        login_argv: None,
        login_hint: Some("Sign in from agy's own TUI"),
    },
    LoginSpec {
        cli: "opencode",
        status_argv: &["opencode", "auth", "list"],
        login_argv: Some(&["opencode", "auth", "login"]),
        login_hint: None,
    },
];

pub trait LoginProbe {
    fn which(&self, bin: &str) -> Option<PathBuf>;
    fn version(&self, cli: &str) -> String;
    fn run(&self, argv: &[&str]) -> (i32, String, String);
}

pub struct HostLoginProbe;

impl LoginProbe for HostLoginProbe {
    fn which(&self, bin: &str) -> Option<PathBuf> {
        crate::versions::which_cli(bin)
    }

    fn version(&self, cli: &str) -> String {
        crate::versions::installed_version(cli)
    }

    fn run(&self, argv: &[&str]) -> (i32, String, String) {
        crate::versions::run_cli_probe(argv)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginStatusItem {
    pub cli: String,
    pub installed: bool,
    pub version: String,
    pub signed_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    pub ready: bool,
    pub has_login_command: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub login_hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginPaneResult {
    pub cli: String,
    pub pane_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LoginError {
    #[error("unknown CLI: {0}")]
    UnknownCli(String),
    #[error("{0} is not installed on PATH")]
    NotInstalled(String),
    #[error("{cli} has no login subcommand; {hint}")]
    NoLoginSubcommand { cli: String, hint: String },
    #[error(transparent)]
    Tmux(#[from] TmuxError),
}

pub fn spec_for(cli: &str) -> Option<&'static LoginSpec> {
    LOGIN_SPECS.iter().find(|s| s.cli == cli)
}

pub fn default_login_items() -> Vec<LoginStatusItem> {
    LOGIN_SPECS
        .iter()
        .map(|spec| LoginStatusItem {
            cli: spec.cli.to_string(),
            installed: false,
            version: String::new(),
            signed_in: false,
            method: None,
            ready: false,
            has_login_command: spec.login_argv.is_some(),
            login_hint: spec.login_hint.map(str::to_string),
            error: None,
        })
        .collect()
}

pub fn parse_status_output(
    cli: &str,
    code: i32,
    stdout: &str,
    stderr: &str,
) -> (bool, Option<String>, Option<String>) {
    match cli {
        "claude" => {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(stdout) {
                let logged_in = val
                    .get("loggedIn")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if logged_in {
                    let auth_method = val
                        .get("authMethod")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty());
                    let sub_type = val
                        .get("subscriptionType")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty());
                    let method = if sub_type.is_some() || auth_method == Some("claude.ai") {
                        Some("subscription".to_string())
                    } else {
                        auth_method.map(str::to_string)
                    };
                    (true, method, None)
                } else {
                    (false, None, None)
                }
            } else if code != 0 {
                let err = first_non_empty(stderr, stdout);
                (false, None, err)
            } else {
                (
                    false,
                    None,
                    Some("unrecognized status output from claude".to_string()),
                )
            }
        }
        "agy" => {
            if code == 0 && !stdout.trim().is_empty() {
                (true, None, None)
            } else if code == 0 {
                (false, None, None)
            } else {
                (false, None, first_non_empty(stderr, stdout))
            }
        }
        "opencode" => {
            if code != 0 {
                return (false, None, first_non_empty(stderr, stdout));
            }
            let cred_lines: Vec<&str> = stdout
                .lines()
                .map(str::trim)
                .filter(|line| {
                    !line.is_empty()
                        && !line.to_ascii_lowercase().contains("no credentials")
                        && !line.to_ascii_lowercase().contains("0 credentials")
                })
                .collect();
            if cred_lines.is_empty() {
                (false, None, None)
            } else {
                let joined = cred_lines.join(" ").to_ascii_lowercase();
                let method = if joined.contains("api key") {
                    Some("api-key".to_string())
                } else if joined.contains("oauth") || joined.contains("subscription") {
                    Some("subscription".to_string())
                } else {
                    None
                };
                (true, method, None)
            }
        }
        _ => (false, None, Some(format!("unknown CLI: {cli}"))),
    }
}

fn first_non_empty(primary: &str, fallback: &str) -> Option<String> {
    primary
        .lines()
        .chain(fallback.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|s| s.chars().take(120).collect())
}

pub fn check_status(cli: &str, probe: &dyn LoginProbe) -> LoginStatusItem {
    check_status_opts(cli, probe, true)
}

pub fn check_readiness(cli: &str, probe: &dyn LoginProbe) -> LoginStatusItem {
    check_status_opts(cli, probe, false)
}

fn check_status_opts(cli: &str, probe: &dyn LoginProbe, include_version: bool) -> LoginStatusItem {
    let Some(spec) = spec_for(cli) else {
        return LoginStatusItem {
            cli: cli.to_string(),
            installed: false,
            version: String::new(),
            signed_in: false,
            method: None,
            ready: false,
            has_login_command: false,
            login_hint: None,
            error: Some(format!("unknown CLI: {cli}")),
        };
    };

    let installed = probe.which(spec.cli).is_some();
    if !installed {
        return LoginStatusItem {
            cli: spec.cli.to_string(),
            installed: false,
            version: String::new(),
            signed_in: false,
            method: None,
            ready: false,
            has_login_command: spec.login_argv.is_some(),
            login_hint: spec.login_hint.map(str::to_string),
            error: None,
        };
    }

    let version = if include_version {
        probe.version(spec.cli)
    } else {
        String::new()
    };
    let (code, stdout, stderr) = probe.run(spec.status_argv);
    let (signed_in, method, error) = parse_status_output(spec.cli, code, &stdout, &stderr);

    LoginStatusItem {
        cli: spec.cli.to_string(),
        installed: true,
        version,
        signed_in,
        method,
        ready: installed && signed_in,
        has_login_command: spec.login_argv.is_some(),
        login_hint: spec.login_hint.map(str::to_string),
        error,
    }
}

pub fn all_statuses(probe: &dyn LoginProbe) -> Vec<LoginStatusItem> {
    LOGIN_SPECS
        .iter()
        .map(|spec| check_status(spec.cli, probe))
        .collect()
}

pub fn open_login_pane(
    client: &TmuxClient,
    workspace: &Path,
    cli: &str,
    probe: &dyn LoginProbe,
) -> Result<LoginPaneResult, LoginError> {
    let spec = spec_for(cli).ok_or_else(|| LoginError::UnknownCli(cli.to_string()))?;
    if probe.which(spec.cli).is_none() {
        return Err(LoginError::NotInstalled(spec.cli.to_string()));
    }
    let Some(login_argv) = spec.login_argv else {
        return Err(LoginError::NoLoginSubcommand {
            cli: spec.cli.to_string(),
            hint: spec
                .login_hint
                .unwrap_or("sign in from the CLI's own TUI")
                .to_string(),
        });
    };

    let win_name = format!("login-{cli}");
    let ws_str = workspace.to_string_lossy().into_owned();
    let mut args = vec![
        "new-window",
        "-P",
        "-F",
        "#{pane_id}",
        "-t",
        SESSION,
        "-n",
        &win_name,
        "-c",
        &ws_str,
    ];
    args.extend_from_slice(login_argv);
    let pane_id = client.run_checked(&args)?.trim().to_string();

    let label = format!("login·{cli}");
    let batch = [
        ("kind", "login"),
        ("role", "login"),
        ("index", "0"),
        ("label", label.as_str()),
        ("cli", cli),
    ]
    .into_iter()
    .map(|(k, v)| {
        vec![
            "set-option".to_string(),
            "-p".to_string(),
            "-t".to_string(),
            pane_id.clone(),
            format!("@tandem_{k}"),
            v.to_string(),
        ]
    })
    .collect::<Vec<_>>();
    client.run_batch_checked(&batch)?;

    Ok(LoginPaneResult {
        cli: cli.to_string(),
        pane_id,
    })
}
