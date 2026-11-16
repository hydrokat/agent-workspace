use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use super::FactoryError;
use super::machine::EventData;
use super::model::Actor;
use crate::coordination::filelock::exclusive_lock;

pub const EVENT_VERSION: u32 = 1;
pub const MAX_EVENT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub v: u32,
    pub seq: u64,
    pub ts: f64,
    pub actor: Actor,
    pub r#type: String,
    pub idea: String,
    #[serde(default)]
    pub run: Option<String>,
    pub data: Value,
}

impl Event {
    pub fn from_data(
        ts: f64,
        actor: Actor,
        idea: String,
        run: Option<String>,
        event_data: &EventData,
    ) -> Result<Self, FactoryError> {
        let mut obj = match serde_json::to_value(event_data)? {
            Value::Object(map) => map,
            _ => {
                return Err(FactoryError::InvalidInput(
                    "event payload must serialize to an object".to_string(),
                ));
            }
        };
        let event_type = obj
            .remove("type")
            .and_then(|v| v.as_str().map(str::to_string))
            .ok_or_else(|| FactoryError::InvalidInput("missing event type".to_string()))?;
        let data = obj.remove("data").unwrap_or_else(|| serde_json::json!({}));

        Ok(Self {
            v: EVENT_VERSION,
            seq: 0,
            ts,
            actor,
            r#type: event_type,
            idea,
            run,
            data,
        })
    }

    pub fn to_event_data(&self) -> Result<EventData, serde_json::Error> {
        serde_json::from_value(serde_json::json!({
            "type": self.r#type,
            "data": self.data,
        }))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReadResult {
    pub events: Vec<Event>,
    pub truncated: bool,
    pub skipped: usize,
}

pub(crate) fn reject_if_symlink(path: &Path) -> Result<(), FactoryError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(FactoryError::Symlink(path.to_path_buf())),
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(FactoryError::Io(err)),
    }
}

pub(crate) fn set_private_file_mode(path: &Path) -> Result<(), FactoryError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(crate) fn set_private_dir_mode(path: &Path) -> Result<(), FactoryError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn parse_complete_lines(bytes: &[u8]) -> (Vec<Event>, usize) {
    let mut events = Vec::new();
    let mut skipped = 0usize;

    for raw_line in bytes.split(|&b| b == b'\n') {
        let trimmed = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        if trimmed.iter().all(|b| b.is_ascii_whitespace()) {
            continue;
        }
        match serde_json::from_slice::<Event>(trimmed) {
            Ok(ev) => events.push(ev),
            Err(_) => skipped += 1,
        }
    }

    (events, skipped)
}

pub fn read_all(log_path: &Path) -> Result<ReadResult, FactoryError> {
    reject_if_symlink(log_path)?;

    let bytes = match fs::read(log_path) {
        Ok(b) => b,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ReadResult {
                events: Vec::new(),
                truncated: false,
                skipped: 0,
            });
        }
        Err(err) => return Err(FactoryError::Io(err)),
    };

    if bytes.is_empty() {
        return Ok(ReadResult {
            events: Vec::new(),
            truncated: false,
            skipped: 0,
        });
    }

    let (valid_bytes, truncated) = if bytes.ends_with(b"\n") {
        (bytes.as_slice(), false)
    } else {
        let cut = bytes
            .iter()
            .rposition(|&b| b == b'\n')
            .map(|idx| idx + 1)
            .unwrap_or(0);
        (&bytes[..cut], true)
    };

    let (events, skipped) = parse_complete_lines(valid_bytes);
    Ok(ReadResult {
        events,
        truncated,
        skipped,
    })
}

pub fn read_since(log_path: &Path, seq: u64) -> Result<ReadResult, FactoryError> {
    let mut res = read_all(log_path)?;
    res.events.retain(|ev| ev.seq > seq);
    Ok(res)
}

pub(crate) fn append_locked(log_path: &Path, events: &[Event]) -> Result<Vec<Event>, FactoryError> {
    reject_if_symlink(log_path)?;
    if events.is_empty() {
        return Ok(Vec::new());
    }

    let (existing_len, valid_end, last_seq) = match fs::read(log_path) {
        Ok(bytes) => {
            let valid_end = if bytes.is_empty() || bytes.ends_with(b"\n") {
                bytes.len()
            } else {
                bytes
                    .iter()
                    .rposition(|&b| b == b'\n')
                    .map(|idx| idx + 1)
                    .unwrap_or(0)
            };
            let (existing_events, _) = parse_complete_lines(&bytes[..valid_end]);
            let last_seq = existing_events.iter().map(|ev| ev.seq).max().unwrap_or(0);
            (bytes.len(), valid_end, last_seq)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (0, 0, 0),
        Err(err) => return Err(FactoryError::Io(err)),
    };

    let mut assigned = Vec::with_capacity(events.len());
    let mut buf = String::new();

    for (idx, event) in events.iter().enumerate() {
        let mut ev = event.clone();
        ev.v = EVENT_VERSION;
        ev.seq = last_seq + 1 + idx as u64;

        let line = serde_json::to_string(&ev)?;
        let line_bytes = line.len() + 1;
        if line_bytes > MAX_EVENT_BYTES {
            return Err(FactoryError::EventTooLarge {
                bytes: line_bytes,
                max: MAX_EVENT_BYTES,
            });
        }
        buf.push_str(&line);
        buf.push('\n');
        assigned.push(ev);
    }

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(log_path)?;
    set_private_file_mode(log_path)?;

    if valid_end < existing_len {
        file.set_len(valid_end as u64)?;
    }
    file.seek(SeekFrom::End(0))?;
    file.write_all(buf.as_bytes())?;
    file.flush()?;
    file.sync_all()?;

    Ok(assigned)
}

pub fn append(
    log_path: &Path,
    lock_path: &Path,
    events: &[Event],
) -> Result<Vec<Event>, FactoryError> {
    reject_if_symlink(lock_path)?;
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path)?;
    set_private_file_mode(lock_path)?;
    let _guard = exclusive_lock(&lock_file)?;
    append_locked(log_path, events)
}
