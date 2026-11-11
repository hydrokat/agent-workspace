use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::conversations::{is_valid_session_id, record as record_conv, recorded, valid_id};

pub const MAX_INPUT: usize = 1_000_000;
pub const DEFAULT_CHAIN_TIMEOUT: Duration = Duration::from_millis(5000);
pub const STALE_AFTER: f64 = 600.0;
pub const NOT_AVAILABLE: &str = "quota not available";
pub const FEED_COMMAND: &str = "quota-feed";

#[derive(Debug, thiserror::Error)]
pub enum QuotaError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub used_percentage: f64,
    pub resets_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaudeQuota {
    #[serde(default)]
    pub five_hour: Option<Window>,
    #[serde(default)]
    pub seven_day: Option<Window>,
    pub updated: f64,
}

pub fn quota_dir(coord_dir: &Path) -> PathBuf {
    coord_dir.join("quota")
}

fn parse_window(raw: Option<&serde_json::Value>) -> Option<Window> {
    let obj = raw?.as_object()?;
    let used_val = obj.get("used_percentage")?;
    if used_val.is_boolean() {
        return None;
    }
    let used = used_val.as_f64()?;
    if !(0.0..=1000.0).contains(&used) {
        return None;
    }

    let resets_val = obj.get("resets_at")?;
    if resets_val.is_boolean() {
        return None;
    }
    let resets = resets_val.as_i64()?;
    if !(1..4_102_444_800).contains(&resets) {
        return None;
    }

    Some(Window {
        used_percentage: used,
        resets_at: resets,
    })
}

pub fn parse_statusline(data: &serde_json::Value, now: f64) -> Option<ClaudeQuota> {
    let limits = data.as_object()?.get("rate_limits")?.as_object()?;
    let five_hour = parse_window(limits.get("five_hour"));
    let seven_day = parse_window(limits.get("seven_day"));

    if five_hour.is_none() && seven_day.is_none() {
        None
    } else {
        Some(ClaudeQuota {
            five_hour,
            seven_day,
            updated: now,
        })
    }
}

pub fn write_feed(
    coord_dir: &Path,
    session_id: &str,
    quota: &ClaudeQuota,
) -> Result<(), QuotaError> {
    if !is_valid_session_id(session_id) {
        return Err(QuotaError::Invalid("invalid session id".to_string()));
    }
    let folder = quota_dir(coord_dir);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        let _ = builder.create(&folder);
    }
    #[cfg(not(unix))]
    {
        let _ = fs::create_dir_all(&folder);
    }

    let tmp_path = folder.join(format!(".q-{}.tmp", uuid::Uuid::new_v4()));
    let target = folder.join(format!("claude-{session_id}.json"));

    let mut json_obj = serde_json::Map::new();
    json_obj.insert(
        "updated".to_string(),
        serde_json::Number::from_f64(quota.updated)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
    );

    if let Some(ref w) = quota.five_hour {
        json_obj.insert(
            "five_hour".to_string(),
            serde_json::json!({
                "used_percentage": w.used_percentage,
                "resets_at": w.resets_at,
            }),
        );
    }
    if let Some(ref w) = quota.seven_day {
        json_obj.insert(
            "seven_day".to_string(),
            serde_json::json!({
                "used_percentage": w.used_percentage,
                "resets_at": w.resets_at,
            }),
        );
    }

    let bytes = serde_json::to_vec(&serde_json::Value::Object(json_obj))?;
    let mut file = File::create(&tmp_path)?;
    file.write_all(&bytes)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));
    }

    fs::rename(&tmp_path, target)?;
    Ok(())
}

pub fn read_claude(coord_dir: &Path) -> Option<ClaudeQuota> {
    let folder = quota_dir(coord_dir);
    let entries = fs::read_dir(folder).ok()?;
    let mut best: Option<ClaudeQuota> = None;

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("claude-")
            && name.ends_with(".json")
            && let Ok(text) = fs::read_to_string(entry.path())
            && let Ok(raw) = serde_json::from_str::<serde_json::Value>(&text)
        {
            let updated = raw.get("updated").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let payload = serde_json::json!({ "rate_limits": raw });
            if let Some(found) = parse_statusline(&payload, updated)
                && best.as_ref().is_none_or(|b| found.updated > b.updated)
            {
                best = Some(found);
            }
        }
    }
    best
}

pub fn format_when(epoch: i64, now: f64) -> String {
    format_when_with_tz(epoch, now, &chrono::Local)
}

pub fn format_when_with_tz<Tz: chrono::TimeZone>(epoch: i64, now: f64, tz: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    if let Some(dt) = tz.timestamp_opt(epoch, 0).single() {
        if (epoch as f64) - now < 86400.0 {
            dt.format("%H:%M").to_string()
        } else {
            dt.format("%a %H:%M").to_string()
        }
    } else {
        String::new()
    }
}

pub fn format_claude_text(q: Option<&ClaudeQuota>, now: f64) -> String {
    format_claude_text_with_tz(q, now, &chrono::Local)
}

pub fn format_claude_text_with_tz<Tz: chrono::TimeZone>(
    q: Option<&ClaudeQuota>,
    now: f64,
    tz: &Tz,
) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let Some(q) = q else {
        return format!("claude: {NOT_AVAILABLE}");
    };

    let mut windows = Vec::new();
    if let Some(ref w) = q.five_hour
        && (w.resets_at as f64) > now
    {
        windows.push(("5h", w));
    }
    if let Some(ref w) = q.seven_day
        && (w.resets_at as f64) > now
    {
        windows.push(("7d", w));
    }

    if windows.is_empty() {
        return format!("claude: {NOT_AVAILABLE}");
    }

    let parts: Vec<String> = windows
        .iter()
        .map(|(label, w)| {
            format!(
                "{label} {:.0}% (resets {})",
                w.used_percentage,
                format_when_with_tz(w.resets_at, now, tz)
            )
        })
        .collect();

    let mut text = parts.join(" · ");
    if now - q.updated > STALE_AFTER
        && let Some(dt) = tz.timestamp_opt(q.updated as i64, 0).single()
    {
        text.push_str(&format!(" [as of {}]", dt.format("%H:%M")));
    }
    format!("claude: {text}")
}

pub fn format_line(clis: &[&str], claude: Option<&ClaudeQuota>, now: f64) -> String {
    format_line_with_tz(clis, claude, now, &chrono::Local)
}

pub fn format_line_with_tz<Tz: chrono::TimeZone>(
    clis: &[&str],
    claude: Option<&ClaudeQuota>,
    now: f64,
    tz: &Tz,
) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let mut parts = Vec::new();
    if clis.contains(&"claude") {
        parts.push(format_claude_text_with_tz(claude, now, tz));
    }
    if clis.contains(&"agy") {
        parts.push(format!("agy: {NOT_AVAILABLE}"));
    }
    if clis.contains(&"opencode") {
        parts.push(format!("opencode: {NOT_AVAILABLE}"));
    }

    let line = if !parts.is_empty() {
        format!("Quota  {}", parts.join("  │  "))
    } else {
        format!("Quota: {NOT_AVAILABLE} (no agents running)")
    };

    tmux_safe(&line)
}

pub fn tmux_safe(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .filter(|&c| c > '\x1f' && c != '\x7f')
        .collect();
    cleaned.replace('#', "##")
}

pub fn user_statusline(cwd: &Path, home: &Path) -> Option<String> {
    let candidates = [
        cwd.join(".claude").join("settings.local.json"),
        cwd.join(".claude").join("settings.json"),
        home.join(".claude").join("settings.json"),
    ];

    for path in candidates {
        if let Ok(text) = fs::read_to_string(path)
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&text)
            && let Some(line) = val.get("statusLine")
            && line.get("type").and_then(|t| t.as_str()) == Some("command")
            && let Some(cmd) = line.get("command").and_then(|c| c.as_str())
        {
            if cmd.contains(FEED_COMMAND) {
                return None;
            }
            return Some(cmd.to_string());
        }
    }
    None
}

pub fn chain(command: Option<&str>, stdin_text: &str) -> String {
    chain_with_timeout(command, stdin_text, DEFAULT_CHAIN_TIMEOUT)
}

pub fn chain_with_timeout(command: Option<&str>, stdin_text: &str, timeout: Duration) -> String {
    let Some(cmd) = command else {
        return String::new();
    };

    let mut child = match Command::new("/bin/sh")
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(stdin_text.as_bytes());
    }

    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                let mut stdout = Vec::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = std::io::Read::read_to_end(&mut out, &mut stdout);
                }
                return String::from_utf8_lossy(&stdout).into_owned();
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return String::new();
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return String::new(),
        }
    }
}

pub fn run_feed(
    stdin_text: &str,
    coord_dir: &Path,
    session_id: &str,
    cwd: &Path,
    home: &Path,
    now: f64,
) -> String {
    if stdin_text.len() <= MAX_INPUT
        && let Ok(data) = serde_json::from_str::<serde_json::Value>(stdin_text)
    {
        if let Some(quota) = parse_statusline(&data, now) {
            let _ = write_feed(coord_dir, session_id, &quota);
        }
        if let Some(session_id_val) = data.get("session_id").and_then(|v| v.as_str())
            && let Some(conv) = valid_id(session_id_val)
            && recorded(coord_dir, session_id).as_deref() != Some(&conv)
        {
            let _ = record_conv(coord_dir, session_id, &conv);
        }
    }
    let cmd = user_statusline(cwd, home);
    chain(cmd.as_deref(), stdin_text)
}
