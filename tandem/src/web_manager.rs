use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::net_interfaces::NetworkSource;

pub const DEFAULT_WEB_PORT: u16 = 3940;
pub const WEB_CONFIG_FILE_NAME: &str = "web-config.json";
pub const WEB_PID_FILE_NAME: &str = "tandem-web.pid";

pub fn generate_auth_token() -> String {
    format!(
        "tandem_{}",
        uuid::Uuid::new_v4().to_string().replace('-', "")
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebConfig {
    pub enabled: bool,
    pub first_time_acknowledged: bool,
    pub bind_ip: String,
    pub bind_source: NetworkSource,
    pub port: u16,
    pub auth_token: String,
    pub ttyd_enabled: bool,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            first_time_acknowledged: false,
            bind_ip: "127.0.0.1".to_string(),
            bind_source: NetworkSource::Localhost,
            port: DEFAULT_WEB_PORT,
            auth_token: generate_auth_token(),
            ttyd_enabled: false,
        }
    }
}

pub fn web_config_path(state_dir: &Path) -> PathBuf {
    state_dir.join(WEB_CONFIG_FILE_NAME)
}

pub fn web_pid_path(state_dir: &Path) -> PathBuf {
    state_dir.join(WEB_PID_FILE_NAME)
}

pub fn load_web_config(path: &Path) -> Result<WebConfig, std::io::Error> {
    if !path.exists() {
        return Ok(WebConfig::default());
    }

    let bytes = fs::read(path)?;
    if bytes.is_empty() {
        return Ok(WebConfig::default());
    }

    let config: WebConfig = serde_json::from_slice(&bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(config)
}

pub fn save_web_config(path: &Path, config: &WebConfig) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".web-config-{}.tmp", uuid::Uuid::new_v4()));

    let mut file = File::create(&tmp_path)?;
    serde_json::to_writer_pretty(&mut file, config)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    file.write_all(b"\n")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));
    }

    fs::rename(&tmp_path, path)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebStatus {
    Off,
    Running { url: String, pid: Option<u32> },
    Starting,
    Error(String),
}

pub fn access_url(config: &WebConfig, include_token: bool) -> String {
    if include_token {
        format!(
            "http://{}:{}/?token={}",
            config.bind_ip, config.port, config.auth_token
        )
    } else {
        format!("http://{}:{}/", config.bind_ip, config.port)
    }
}

pub fn web_server_args(config: &WebConfig) -> Result<Vec<String>, String> {
    if config.bind_ip == "0.0.0.0" || config.bind_ip == "::" {
        return Err("refusing to bind tandem-web to a wildcard address".to_string());
    }
    if config.auth_token.is_empty() {
        return Err("refusing to start tandem-web without an auth token".to_string());
    }
    Ok([
        "run",
        "start",
        "--",
        "-H",
        config.bind_ip.as_str(),
        "-p",
        config.port.to_string().as_str(),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect())
}

pub const WEB_VERSION_STAMP: &str = ".tandem-version";

pub fn resolve_web_dir(workspace: &Path, state_dir: &Path) -> Option<PathBuf> {
    let local = workspace.join("tandem-web");
    if local.join("package.json").exists() {
        return Some(local);
    }

    let installed = state_dir.join("tandem-web");
    let stamp = fs::read_to_string(installed.join(WEB_VERSION_STAMP)).unwrap_or_default();
    if installed.join("package.json").exists() && stamp.trim() == env!("CARGO_PKG_VERSION") {
        return Some(installed);
    }
    None
}

pub fn web_is_built(web_dir: &Path) -> bool {
    web_dir.join(".next").join("BUILD_ID").exists()
}

pub trait DaemonRunner {
    fn start_daemon(
        &self,
        config: &WebConfig,
        workspace: &Path,
        state_dir: &Path,
    ) -> Result<u32, std::io::Error>;
    fn stop_daemon(&self, state_dir: &Path) -> Result<(), std::io::Error>;
    fn status(&self, config: &WebConfig, state_dir: &Path) -> WebStatus;
}

pub struct SystemDaemonRunner;

impl SystemDaemonRunner {
    fn read_pid(state_dir: &Path) -> Option<u32> {
        let pid_path = web_pid_path(state_dir);
        if !pid_path.exists() {
            return None;
        }

        let content = fs::read_to_string(&pid_path).ok()?;
        let pid: u32 = content.trim().parse().ok()?;
        if is_pid_alive(pid) {
            Some(pid)
        } else {
            let _ = fs::remove_file(&pid_path);
            None
        }
    }
}

pub fn is_pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

impl DaemonRunner for SystemDaemonRunner {
    fn start_daemon(
        &self,
        config: &WebConfig,
        workspace: &Path,
        state_dir: &Path,
    ) -> Result<u32, std::io::Error> {
        if let Some(existing_pid) = Self::read_pid(state_dir) {
            return Ok(existing_pid);
        }

        let Some(web_dir) = resolve_web_dir(workspace, state_dir) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "tandem-web is not installed (or is an older version); run `agent-workspace web` once to install it",
            ));
        };

        if !web_is_built(&web_dir) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "tandem-web is not built; run `agent-workspace web` once to install and build it",
            ));
        }

        let args = web_server_args(config)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;

        let bun = crate::web_deps::locate_bun(&crate::web_deps::DepsEnv::from_process())
            .unwrap_or_else(|| PathBuf::from("bun"));
        let tandem_bin = crate::selfexec::self_argv()
            .into_iter()
            .next()
            .unwrap_or_else(|| "tandem".to_string());
        let child = std::process::Command::new(bun)
            .args(&args)
            .current_dir(&web_dir)
            .env("PORT", config.port.to_string())
            .env("HOSTNAME", &config.bind_ip)
            .env("TANDEM_AUTH_TOKEN", &config.auth_token)
            .env("TANDEM_BIN", &tandem_bin)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        let pid = child.id();
        let pid_path = web_pid_path(state_dir);
        let _ = fs::write(&pid_path, format!("{pid}\n"));

        Ok(pid)
    }

    fn stop_daemon(&self, state_dir: &Path) -> Result<(), std::io::Error> {
        let pid_path = web_pid_path(state_dir);
        if let Some(pid) = Self::read_pid(state_dir) {
            #[cfg(unix)]
            {
                let _ = std::process::Command::new("kill")
                    .args(["-TERM", &pid.to_string()])
                    .output();
            }
        }
        let _ = fs::remove_file(&pid_path);
        Ok(())
    }

    fn status(&self, config: &WebConfig, state_dir: &Path) -> WebStatus {
        if !config.enabled {
            return WebStatus::Off;
        }

        if let Some(pid) = Self::read_pid(state_dir) {
            let url = format!("http://{}:{}", config.bind_ip, config.port);
            WebStatus::Running {
                url,
                pid: Some(pid),
            }
        } else {
            WebStatus::Off
        }
    }
}
