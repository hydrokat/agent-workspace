use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub use super::FactoryError;
use super::machine::EventData;
use super::model::Actor;
use super::redact::{RedactContext, redact};
use crate::coordination::filelock::exclusive_lock;

pub const EVENT_VERSION: u32 = 1;
pub const MAX_EVENT_BYTES: usize = 64 * 1024;
pub const DEFAULT_SEGMENT_MAX_BYTES: u64 = 16 * 1024 * 1024;
const EVENT_INDEX_VERSION: u32 = 1;

thread_local! {
    static FULL_LOG_READ_COUNT: Cell<usize> = const { Cell::new(0) };
    static INDEXED_READ_COUNT: Cell<usize> = const { Cell::new(0) };
}

pub fn reset_event_log_read_counters() {
    FULL_LOG_READ_COUNT.with(|c| c.set(0));
    INDEXED_READ_COUNT.with(|c| c.set(0));
}

pub fn full_log_read_count() -> usize {
    FULL_LOG_READ_COUNT.with(|c| c.get())
}

pub fn indexed_read_count() -> usize {
    INDEXED_READ_COUNT.with(|c| c.get())
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev: Option<String>,
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
            prev: None,
        })
    }

    pub fn to_event_data(&self) -> Result<EventData, serde_json::Error> {
        serde_json::from_value(serde_json::json!({
            "type": self.r#type,
            "data": self.data,
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RedactedEvent(Event);

impl RedactedEvent {
    pub fn as_event(&self) -> &Event {
        &self.0
    }

    pub fn into_event(self) -> Event {
        self.0
    }
}

fn is_hash_json_key(key: &str) -> bool {
    matches!(
        key,
        "sha256" | "plan_sha256" | "workflow_sha256" | "base_commit" | "commit" | "commit_id"
    )
}

fn redact_json_value(val: &mut Value, context: &RedactContext) {
    match val {
        Value::String(s) => {
            *s = redact(s, context).into_string();
        }
        Value::Array(items) => {
            for item in items {
                redact_json_value(item, context);
            }
        }
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if is_hash_json_key(k) {
                    continue;
                }
                redact_json_value(v, context);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

pub fn redact_event(mut event: Event, context: &RedactContext) -> RedactedEvent {
    redact_json_value(&mut event.data, context);
    RedactedEvent(event)
}

pub fn redact_events(events: &[Event], context: &RedactContext) -> Vec<RedactedEvent> {
    events
        .iter()
        .cloned()
        .map(|ev| redact_event(ev, context))
        .collect()
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventIndex {
    pub version: u32,
    pub segments: Vec<String>,
    pub segment_sizes: BTreeMap<String, u64>,
    pub ideas: BTreeMap<String, Vec<(u16, u64, u32, u64)>>,
    pub runs: BTreeMap<String, Vec<(u16, u64, u32, u64)>>,
}

fn parse_segment_number(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("events-")?.strip_suffix(".jsonl")?;
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse::<u32>().ok()
}

pub fn segment_paths(log_path: &Path) -> Result<Vec<PathBuf>, FactoryError> {
    reject_if_symlink(log_path)?;
    let Some(dir) = log_path.parent() else {
        return Ok(if log_path.exists() {
            vec![log_path.to_path_buf()]
        } else {
            Vec::new()
        });
    };

    let mut numbered: Vec<(u32, PathBuf)> = Vec::new();
    match fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|s| s.to_str())
                    && let Some(num) = parse_segment_number(name)
                {
                    reject_if_symlink(&path)?;
                    if path.is_file() {
                        numbered.push((num, path));
                    }
                }
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(FactoryError::Io(err)),
    }

    numbered.sort_by_key(|(num, _)| *num);
    let mut out: Vec<PathBuf> = numbered.into_iter().map(|(_, p)| p).collect();
    if log_path.exists() {
        out.push(log_path.to_path_buf());
    }
    Ok(out)
}

fn index_path_for(log_path: &Path) -> PathBuf {
    log_path.with_file_name("events.index")
}

fn next_segment_path(log_path: &Path) -> Result<PathBuf, FactoryError> {
    let dir = log_path.parent().unwrap_or_else(|| Path::new("."));
    let mut max_num = 0u32;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && let Some(num) = parse_segment_number(name)
            {
                max_num = max_num.max(num);
            }
        }
    }
    Ok(dir.join(format!("events-{:04}.jsonl", max_num + 1)))
}

fn valid_line_slice(bytes: &[u8]) -> (&[u8], bool) {
    if bytes.is_empty() || bytes.ends_with(b"\n") {
        (bytes, false)
    } else {
        let cut = bytes
            .iter()
            .rposition(|&b| b == b'\n')
            .map(|idx| idx + 1)
            .unwrap_or(0);
        (&bytes[..cut], true)
    }
}

pub fn read_all(log_path: &Path) -> Result<ReadResult, FactoryError> {
    FULL_LOG_READ_COUNT.with(|c| c.set(c.get() + 1));
    let segments = segment_paths(log_path)?;
    if segments.is_empty() {
        return Ok(ReadResult {
            events: Vec::new(),
            truncated: false,
            skipped: 0,
        });
    }

    let mut all_events = Vec::new();
    let mut total_skipped = 0usize;
    let mut last_truncated = false;

    for (idx, seg_path) in segments.iter().enumerate() {
        let bytes = match fs::read(seg_path) {
            Ok(b) => b,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(FactoryError::Io(err)),
        };
        if bytes.is_empty() {
            continue;
        }
        let (valid_bytes, truncated) = valid_line_slice(&bytes);
        let (events, skipped) = parse_complete_lines(valid_bytes);
        all_events.extend(events);
        total_skipped += skipped;
        if idx + 1 == segments.len() {
            last_truncated = truncated;
        }
    }

    Ok(ReadResult {
        events: all_events,
        truncated: last_truncated,
        skipped: total_skipped,
    })
}

pub fn read_since(log_path: &Path, seq: u64) -> Result<ReadResult, FactoryError> {
    let mut res = read_all(log_path)?;
    res.events.retain(|ev| ev.seq > seq);
    Ok(res)
}

fn write_index_atomic(idx_path: &Path, index: &EventIndex) -> Result<(), FactoryError> {
    reject_if_symlink(idx_path)?;
    let tmp = idx_path.with_extension("index.tmp");
    reject_if_symlink(&tmp)?;
    let bytes = serde_json::to_vec(index)?;
    let mut f = File::create(&tmp)?;
    set_private_file_mode(&tmp)?;
    f.write_all(&bytes)?;
    f.flush()?;
    f.sync_all()?;
    fs::rename(&tmp, idx_path)?;
    Ok(())
}

pub fn rebuild_index(log_path: &Path) -> Result<EventIndex, FactoryError> {
    let segments = segment_paths(log_path)?;
    let mut index = EventIndex {
        version: EVENT_INDEX_VERSION,
        ..Default::default()
    };

    for seg_path in segments {
        let Some(seg_name) = seg_path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let bytes = match fs::read(&seg_path) {
            Ok(b) => b,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(FactoryError::Io(err)),
        };
        let seg_idx = index.segments.len() as u16;
        index.segments.push(seg_name.to_string());
        index
            .segment_sizes
            .insert(seg_name.to_string(), bytes.len() as u64);

        let (valid_bytes, _) = valid_line_slice(&bytes);
        let mut pos = 0usize;
        for raw_line in valid_bytes.split(|&b| b == b'\n') {
            let line_start = pos;
            pos += raw_line.len() + 1;
            let trimmed = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
            if trimmed.iter().all(|b| b.is_ascii_whitespace()) {
                continue;
            }
            if let Ok(ev) = serde_json::from_slice::<Event>(trimmed) {
                let entry = (seg_idx, line_start as u64, trimmed.len() as u32, ev.seq);
                index.ideas.entry(ev.idea).or_default().push(entry);
                if let Some(run_id) = ev.run {
                    index.runs.entry(run_id).or_default().push(entry);
                }
            }
        }
    }

    write_index_atomic(&index_path_for(log_path), &index)?;
    Ok(index)
}

fn load_or_rebuild_index(log_path: &Path) -> Result<EventIndex, FactoryError> {
    let idx_path = index_path_for(log_path);
    reject_if_symlink(&idx_path)?;

    if let Ok(raw) = fs::read(&idx_path)
        && let Ok(index) = serde_json::from_slice::<EventIndex>(&raw)
        && index.version == EVENT_INDEX_VERSION
    {
        let segments = segment_paths(log_path)?;
        let mut matches = segments.len() == index.segment_sizes.len();
        if matches {
            for seg in &segments {
                let Some(name) = seg.file_name().and_then(|s| s.to_str()) else {
                    matches = false;
                    break;
                };
                let disk_len = fs::metadata(seg).map(|m| m.len()).unwrap_or(0);
                if index.segment_sizes.get(name).copied() != Some(disk_len) {
                    matches = false;
                    break;
                }
            }
        }
        if matches {
            return Ok(index);
        }
    }

    rebuild_index(log_path)
}

fn read_indexed_entries(
    log_path: &Path,
    index: &EventIndex,
    entries: &[(u16, u64, u32, u64)],
) -> Result<Vec<Event>, FactoryError> {
    INDEXED_READ_COUNT.with(|c| c.set(c.get() + 1));
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let dir = log_path.parent().unwrap_or_else(|| Path::new("."));
    let mut open_files: BTreeMap<u16, File> = BTreeMap::new();
    let mut out = Vec::with_capacity(entries.len());

    for &(seg_idx, offset, len, _) in entries {
        let Some(seg_name) = index.segments.get(seg_idx as usize) else {
            continue;
        };
        let file = match open_files.entry(seg_idx) {
            std::collections::btree_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::btree_map::Entry::Vacant(e) => {
                let seg_path = dir.join(seg_name);
                reject_if_symlink(&seg_path)?;
                e.insert(File::open(&seg_path)?)
            }
        };
        file.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; len as usize];
        file.read_exact(&mut buf)?;
        if let Ok(ev) = serde_json::from_slice::<Event>(&buf) {
            out.push(ev);
        }
    }

    Ok(out)
}

pub fn read_idea_events(log_path: &Path, idea_id: &str) -> Result<Vec<Event>, FactoryError> {
    let index = load_or_rebuild_index(log_path)?;
    let Some(entries) = index.ideas.get(idea_id) else {
        INDEXED_READ_COUNT.with(|c| c.set(c.get() + 1));
        return Ok(Vec::new());
    };
    read_indexed_entries(log_path, &index, entries)
}

pub fn read_run_events(log_path: &Path, run_id: &str) -> Result<Vec<Event>, FactoryError> {
    let index = load_or_rebuild_index(log_path)?;
    let Some(entries) = index.runs.get(run_id) else {
        INDEXED_READ_COUNT.with(|c| c.set(c.get() + 1));
        return Ok(Vec::new());
    };
    read_indexed_entries(log_path, &index, entries)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn last_non_empty_line_hash(bytes: &[u8]) -> Option<String> {
    let mut last_line: Option<&[u8]> = None;
    for raw_line in bytes.split(|&b| b == b'\n') {
        let trimmed = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        if !trimmed.iter().all(|b| b.is_ascii_whitespace()) {
            last_line = Some(trimmed);
        }
    }
    last_line.map(sha256_hex)
}

fn tail_seq_and_hash_from_bytes(bytes: &[u8]) -> (usize, u64, Option<String>) {
    let (valid_bytes, _) = valid_line_slice(bytes);
    let (existing_events, _) = parse_complete_lines(valid_bytes);
    let last_seq = existing_events.iter().map(|ev| ev.seq).max().unwrap_or(0);
    let prev_hash = last_non_empty_line_hash(valid_bytes);
    (valid_bytes.len(), last_seq, prev_hash)
}

fn tail_seq_and_hash_across_segments(
    log_path: &Path,
) -> Result<(u64, Option<String>), FactoryError> {
    let segments = segment_paths(log_path)?;
    for seg in segments.into_iter().rev() {
        let bytes = match fs::read(&seg) {
            Ok(b) => b,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(FactoryError::Io(err)),
        };
        let (_, seq, hash) = tail_seq_and_hash_from_bytes(&bytes);
        if seq > 0 || hash.is_some() {
            return Ok((seq, hash));
        }
    }
    Ok((0, None))
}

fn rotate_active_segment(log_path: &Path) -> Result<PathBuf, FactoryError> {
    let archived = next_segment_path(log_path)?;
    reject_if_symlink(&archived)?;
    fs::rename(log_path, &archived)?;
    let mut fresh = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)?;
    set_private_file_mode(log_path)?;
    fresh.flush()?;
    fresh.sync_all()?;
    Ok(archived)
}

pub(crate) fn append_locked(
    log_path: &Path,
    events: &[RedactedEvent],
) -> Result<Vec<Event>, FactoryError> {
    append_locked_with_max_bytes(log_path, events, DEFAULT_SEGMENT_MAX_BYTES)
}

pub(crate) fn append_locked_with_max_bytes(
    log_path: &Path,
    events: &[RedactedEvent],
    max_bytes: u64,
) -> Result<Vec<Event>, FactoryError> {
    reject_if_symlink(log_path)?;
    if events.is_empty() {
        return Ok(Vec::new());
    }

    if let Ok(meta) = fs::metadata(log_path)
        && meta.len() >= max_bytes
        && meta.len() > 0
    {
        rotate_active_segment(log_path)?;
    }

    let (existing_len, valid_end, last_seq, mut prev_hash) = match fs::read(log_path) {
        Ok(bytes) => {
            let (valid_end, last_seq, prev_hash) = tail_seq_and_hash_from_bytes(&bytes);
            if last_seq == 0 && prev_hash.is_none() {
                let (seg_seq, seg_hash) = tail_seq_and_hash_across_segments(log_path)?;
                (bytes.len(), valid_end, seg_seq, seg_hash)
            } else {
                (bytes.len(), valid_end, last_seq, prev_hash)
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let (seg_seq, seg_hash) = tail_seq_and_hash_across_segments(log_path)?;
            (0, 0, seg_seq, seg_hash)
        }
        Err(err) => return Err(FactoryError::Io(err)),
    };

    let mut assigned = Vec::with_capacity(events.len());
    let mut buf = String::new();
    let mut pending_offsets: Vec<(u64, u32, u64, String, Option<String>)> =
        Vec::with_capacity(events.len());

    for (idx, redacted) in events.iter().enumerate() {
        let mut ev = redacted.0.clone();
        ev.v = EVENT_VERSION;
        ev.seq = last_seq + 1 + idx as u64;
        ev.prev = prev_hash.clone();

        let line = serde_json::to_string(&ev)?;
        let line_bytes = line.len() + 1;
        if line_bytes > MAX_EVENT_BYTES {
            return Err(FactoryError::EventTooLarge {
                bytes: line_bytes,
                max: MAX_EVENT_BYTES,
            });
        }
        let offset = (valid_end + buf.len()) as u64;
        pending_offsets.push((
            offset,
            line.len() as u32,
            ev.seq,
            ev.idea.clone(),
            ev.run.clone(),
        ));
        prev_hash = Some(sha256_hex(line.as_bytes()));
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
    drop(file);

    let new_len = (valid_end + buf.len()) as u64;
    let rotated_to = if new_len >= max_bytes {
        Some(rotate_active_segment(log_path)?)
    } else {
        None
    };

    update_index_after_append(log_path, new_len, rotated_to.as_deref(), &pending_offsets)?;

    Ok(assigned)
}

fn update_index_after_append(
    log_path: &Path,
    written_seg_len: u64,
    rotated_to: Option<&Path>,
    offsets: &[(u64, u32, u64, String, Option<String>)],
) -> Result<(), FactoryError> {
    let idx_path = index_path_for(log_path);
    let active_name = log_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("events.jsonl")
        .to_string();

    if let Ok(raw) = fs::read(&idx_path)
        && let Ok(mut index) = serde_json::from_slice::<EventIndex>(&raw)
        && index.version == EVENT_INDEX_VERSION
    {
        let seg_idx = if let Some(pos) = index.segments.iter().position(|s| s == &active_name) {
            pos as u16
        } else {
            let pos = index.segments.len() as u16;
            index.segments.push(active_name.clone());
            pos
        };
        for &(offset, len, seq, ref idea_id, ref run_id) in offsets {
            let entry = (seg_idx, offset, len, seq);
            index.ideas.entry(idea_id.clone()).or_default().push(entry);
            if let Some(rid) = run_id {
                index.runs.entry(rid.clone()).or_default().push(entry);
            }
        }
        if let Some(archived_path) = rotated_to
            && let Some(arch_name) = archived_path.file_name().and_then(|s| s.to_str())
        {
            if let Some(slot) = index.segments.get_mut(seg_idx as usize) {
                *slot = arch_name.to_string();
            }
            index.segment_sizes.remove(&active_name);
            index
                .segment_sizes
                .insert(arch_name.to_string(), written_seg_len);
            index.segment_sizes.insert(active_name, 0);
        } else {
            index.segment_sizes.insert(active_name, written_seg_len);
        }
        write_index_atomic(&idx_path, &index)?;
        return Ok(());
    }

    let _ = rebuild_index(log_path)?;
    Ok(())
}

pub fn append(
    log_path: &Path,
    lock_path: &Path,
    events: &[RedactedEvent],
) -> Result<Vec<Event>, FactoryError> {
    append_with_max_bytes(log_path, lock_path, events, DEFAULT_SEGMENT_MAX_BYTES)
}

pub fn append_with_max_bytes(
    log_path: &Path,
    lock_path: &Path,
    events: &[RedactedEvent],
    max_bytes: u64,
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
    append_locked_with_max_bytes(log_path, events, max_bytes)
}
