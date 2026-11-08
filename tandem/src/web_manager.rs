use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::net_interfaces::NetworkSource;

pub const DEFAULT_WEB_PORT: u16 = 3940;
pub const WEB_CONFIG_FILE_NAME: &str = "web-config.json";
pub const WEB_PID_FILE_NAME: &str = "tandem-web.pid";

fn generate_auth_token() -> String {
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

        let web_dir = workspace.join("tandem-web");
        if !web_dir.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "tandem-web directory not found",
            ));
        }

        let child = std::process::Command::new("bun")
            .arg("run")
            .arg("start")
            .current_dir(&web_dir)
            .env("PORT", config.port.to_string())
            .env("HOSTNAME", &config.bind_ip)
            .env("TANDEM_AUTH_TOKEN", &config.auth_token)
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
