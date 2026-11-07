use std::io;

pub const REPO_URL: &str = "https://github.com/hydrokat/agent-workspace";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    DisallowedUrl(String),
    UnsupportedPlatform(String),
    SpawnFailed(String),
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DisallowedUrl(u) => write!(f, "url not in allow-list: {u}"),
            Self::UnsupportedPlatform(p) => write!(f, "unsupported platform: {p}"),
            Self::SpawnFailed(err) => write!(f, "failed to spawn opener: {err}"),
        }
    }
}

impl std::error::Error for LinkError {}

pub trait UrlSpawner: Send + Sync {
    fn spawn(&self, program: &str, args: &[&str]) -> io::Result<()>;
}

pub struct ProcessUrlSpawner;

impl UrlSpawner for ProcessUrlSpawner {
    fn spawn(&self, program: &str, args: &[&str]) -> io::Result<()> {
        std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        Ok(())
    }
}

pub fn open_url(url: &str, platform: &str, spawner: &dyn UrlSpawner) -> Result<(), LinkError> {
    if url != REPO_URL {
        return Err(LinkError::DisallowedUrl(url.to_string()));
    }

    let program = match platform.to_ascii_lowercase().as_str() {
        "macos" | "darwin" => "open",
        "linux" => "xdg-open",
        _ => return Err(LinkError::UnsupportedPlatform(platform.to_string())),
    };

    spawner
        .spawn(program, &[url])
        .map_err(|e| LinkError::SpawnFailed(e.to_string()))
}

pub fn osc8(text: &str, url: &str) -> String {
    format!("\x1b]8;;{url}\x1b\\{text}\x1b]8;;\x1b\\")
}
