use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const LOG_TAIL: u64 = 1_000_000;

#[derive(Debug, thiserror::Error)]
pub enum ConversationError {
    #[error("{0}")]
    InvalidInput(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub fn valid_id(value: &str) -> Option<String> {
    if value.len() != 36 {
        return None;
    }
    let bytes = value.as_bytes();
    if bytes[8] != b'-' || bytes[13] != b'-' || bytes[18] != b'-' || bytes[23] != b'-' {
        return None;
    }
    for (i, &b) in bytes.iter().enumerate() {
        if i == 8 || i == 13 || i == 18 || i == 23 {
            continue;
        }
        if !b.is_ascii_hexdigit() {
            return None;
        }
    }
    Some(value.to_ascii_lowercase())
}

pub fn valid_opencode_session_id(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let ok = (8..=64).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    ok.then(|| value.to_string())
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn is_valid_session_id(s: &str) -> bool {
    if let Some((prefix, suffix)) = s.split_once('-') {
        !prefix.is_empty()
            && prefix.chars().all(|c| c.is_ascii_lowercase())
            && (1..=2).contains(&suffix.len())
            && suffix.chars().all(|c| c.is_ascii_digit())
    } else {
        false
    }
}

pub fn folder(coord_dir: &Path) -> PathBuf {
    coord_dir.join("conversations")
}

pub fn agy_log_path(coord_dir: &Path, session_id: &str) -> Result<PathBuf, ConversationError> {
    if !is_valid_session_id(session_id) {
        return Err(ConversationError::InvalidInput(
            "invalid session id".to_string(),
        ));
    }
    Ok(folder(coord_dir).join(format!("agy-{session_id}.log")))
}

pub fn prepare(coord_dir: &Path) -> std::io::Result<()> {
    let dir = folder(coord_dir);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        let _ = builder.create(&dir);
    }
    #[cfg(not(unix))]
    {
        let _ = fs::create_dir_all(&dir);
    }
    Ok(())
}

pub fn record(
    coord_dir: &Path,
    session_id: &str,
    conversation: &str,
) -> Result<(), ConversationError> {
    let checked_conv = valid_id(conversation)
        .ok_or_else(|| ConversationError::InvalidInput("invalid conversation id".to_string()))?;
    if !is_valid_session_id(session_id) {
        return Err(ConversationError::InvalidInput(
            "invalid session id".to_string(),
        ));
    }
    prepare(coord_dir)?;

    let target = folder(coord_dir).join(format!("{session_id}.json"));
    let tmp_path = folder(coord_dir).join(format!(".c-{}.tmp", uuid::Uuid::new_v4()));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    let data = serde_json::json!({
        "id": checked_conv,
        "updated": now,
    });
    let json_bytes = serde_json::to_vec(&data)?;
    fs::write(&tmp_path, json_bytes)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));
    }

    fs::rename(&tmp_path, &target)?;
    Ok(())
}

pub fn recorded(coord_dir: &Path, session_id: &str) -> Option<String> {
    if !is_valid_session_id(session_id) {
        return None;
    }
    let path = folder(coord_dir).join(format!("{session_id}.json"));
    let text = fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("id").and_then(|v| v.as_str()).and_then(valid_id)
}

pub fn from_agy_log(path: &Path) -> Option<String> {
    from_agy_log_with_tail(path, LOG_TAIL)
}

pub fn from_agy_log_with_tail(path: &Path, tail_limit: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();
    let read_start = file_len.saturating_sub(tail_limit);
    file.seek(SeekFrom::Start(read_start)).ok()?;

    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);

    let mut latest = None;
    let prefixes = ["Created conversation ", "Streaming conversation "];

    for (idx, _) in text.match_indices(" conversation ") {
        for prefix in &prefixes {
            let prefix_len = prefix.len() - " conversation ".len();
            if idx >= prefix_len {
                let start = idx - prefix_len;
                if text[start..].starts_with(prefix) {
                    let conv_start = start + prefix.len();
                    if conv_start + 36 <= text.len() {
                        let candidate = &text[conv_start..conv_start + 36];
                        if let Some(id) = valid_id(candidate) {
                            latest = Some(id);
                        }
                    }
                }
            }
        }
    }
    latest
}

pub fn current(coord_dir: &Path, session_id: &str, cli: &str) -> Option<String> {
    if !is_valid_session_id(session_id) {
        return None;
    }
    match cli {
        "claude" => recorded(coord_dir, session_id),
        "agy" => {
            let log = agy_log_path(coord_dir, session_id).ok()?;
            from_agy_log(&log)
        }
        _ => None,
    }
}

pub fn forget(coord_dir: &Path, session_id: &str) {
    if !is_valid_session_id(session_id) {
        return;
    }
    let record_path = folder(coord_dir).join(format!("{session_id}.json"));
    let _ = fs::remove_file(record_path);
    if let Ok(log_path) = agy_log_path(coord_dir, session_id) {
        let _ = fs::remove_file(log_path);
    }
}

pub fn claude_transcript_exists(claude_dir: &Path, conversation: &str) -> bool {
    let checked = match valid_id(conversation) {
        Some(id) => id,
        None => return false,
    };
    let projects_dir = claude_dir.join("projects");
    if let Ok(entries) = fs::read_dir(projects_dir) {
        let filename = format!("{checked}.jsonl");
        for entry in entries.flatten() {
            if entry.path().is_dir() && entry.path().join(&filename).is_file() {
                return true;
            }
        }
    }
    false
}

pub fn can_resume(cli: &str, conversation: Option<&str>, claude_dir: &Path) -> bool {
    let conv = match conversation {
        Some(c) => c,
        None => return false,
    };
    match cli {
        "claude" => claude_transcript_exists(claude_dir, conv),
        "agy" => valid_id(conv).is_some(),
        _ => false,
    }
}
