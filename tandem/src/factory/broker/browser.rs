use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::factory::events::reject_if_symlink;

use super::{ChildWaitOutcome, MAX_BROKER_OUTPUT_BYTES, wait_child_with_timeout_and_drain};

pub const ALLOWED_BROWSER_BINARIES: &[&str] = &["chromium", "chromium-browser", "google-chrome"];
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenshotRequest {
    pub url: String,
    pub output_path: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum BrowserError {
    #[error("screenshot tool is not installed")]
    NotInstalled,
    #[error("invalid screenshot request: {0}")]
    InvalidInput(String),
    #[error("screenshot capture timed out")]
    Timeout,
    #[error("screenshot capture failed: {0}")]
    Failed(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn validate_local_url(url: &str) -> Result<(), BrowserError> {
    let trimmed = url.trim();
    if trimmed.is_empty()
        || trimmed != url
        || trimmed.starts_with('-')
        || trimmed.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(BrowserError::InvalidInput(
            "url must be a single local URL without whitespace or control characters".to_string(),
        ));
    }
    let rest = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
        .ok_or_else(|| {
            BrowserError::InvalidInput(format!(
                "only local loopback URLs are allowed for demo screenshots: {trimmed}"
            ))
        })?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.contains('@') {
        return Err(BrowserError::InvalidInput(format!(
            "userinfo is not allowed in demo screenshot URLs: {trimmed}"
        )));
    }
    let is_valid_port = |port: &str| {
        !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok()
    };
    let valid_authority = if authority.starts_with('[') {
        authority == "[::1]" || authority.strip_prefix("[::1]:").is_some_and(is_valid_port)
    } else if let Some((host, port)) = authority.split_once(':') {
        matches!(host, "127.0.0.1" | "localhost") && is_valid_port(port)
    } else {
        matches!(authority, "127.0.0.1" | "localhost")
    };
    if !valid_authority {
        return Err(BrowserError::InvalidInput(format!(
            "only local loopback URLs are allowed for demo screenshots: {trimmed}"
        )));
    }
    Ok(())
}

fn validate_output_path(path: &Path) -> Result<(), BrowserError> {
    let raw = path.to_string_lossy();
    if raw.trim().is_empty() || raw.chars().any(char::is_control) {
        return Err(BrowserError::InvalidInput(
            "output path must not be empty or contain control characters".to_string(),
        ));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(BrowserError::InvalidInput(
            "output path must not contain '..'".to_string(),
        ));
    }
    if path.extension().and_then(|e| e.to_str()) != Some("png") {
        return Err(BrowserError::InvalidInput(
            "output path must end with .png".to_string(),
        ));
    }
    reject_if_symlink(path).map_err(|e| BrowserError::InvalidInput(e.to_string()))?;
    Ok(())
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), BrowserError> {
    if !(320..=3840).contains(&width) || !(240..=2160).contains(&height) {
        return Err(BrowserError::InvalidInput(format!(
            "window size {width}x{height} out of allowed bounds (320..=3840 x 240..=2160)"
        )));
    }
    Ok(())
}

pub fn build_screenshot_argv(req: &ScreenshotRequest) -> Result<Vec<String>, BrowserError> {
    validate_local_url(&req.url)?;
    validate_output_path(&req.output_path)?;
    validate_dimensions(req.width, req.height)?;
    Ok(vec![
        "--headless".to_string(),
        "--disable-gpu".to_string(),
        "--no-sandbox".to_string(),
        "--hide-scrollbars".to_string(),
        format!("--window-size={},{}", req.width, req.height),
        format!("--screenshot={}", req.output_path.display()),
        req.url.clone(),
    ])
}

fn validate_binary(binary: &Path) -> Result<(), BrowserError> {
    let name = binary
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if !ALLOWED_BROWSER_BINARIES.contains(&name) {
        return Err(BrowserError::InvalidInput(format!(
            "disallowed screenshot binary: {name}"
        )));
    }
    Ok(())
}

pub fn capture_screenshot(
    binary: &Path,
    req: &ScreenshotRequest,
    cwd: &Path,
    timeout: Duration,
) -> Result<PathBuf, BrowserError> {
    validate_binary(binary)?;
    let argv = build_screenshot_argv(req)?;
    if let Some(parent) = req.output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let mut cmd = Command::new(binary);
    cmd.args(&argv)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(BrowserError::NotInstalled),
        Err(e) => return Err(BrowserError::Io(e)),
    };

    match wait_child_with_timeout_and_drain(&mut child, timeout, MAX_BROKER_OUTPUT_BYTES)? {
        ChildWaitOutcome::Exited { status, stderr, .. } => {
            if !status.success() {
                let stderr_text = String::from_utf8_lossy(&stderr).trim().to_string();
                return Err(BrowserError::Failed(stderr_text));
            }
            let bytes = fs::read(&req.output_path)?;
            if bytes.len() < PNG_SIGNATURE.len() || bytes[..8] != PNG_SIGNATURE {
                return Err(BrowserError::Failed(
                    "captured file is not a valid PNG".to_string(),
                ));
            }
            Ok(req.output_path.clone())
        }
        ChildWaitOutcome::TimedOut => Err(BrowserError::Timeout),
    }
}
