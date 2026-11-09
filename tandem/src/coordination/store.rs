use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};

use super::filelock::exclusive_lock;
use super::schema::{
    Claim, Query, STALE_SECONDS, Session, decode_query_fields, encode_query_fields,
};

#[derive(Debug, thiserror::Error)]
pub enum CoordError {
    #[error("query is already {0}")]
    AnswerConflict(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub fn is_valid_query_id(id: &str) -> bool {
    if let Some(rest) = id.strip_prefix("q-") {
        (4..=32).contains(&rest.len())
            && rest
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    } else {
        false
    }
}

pub fn is_valid_session_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn current_time() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub fn is_stale(heartbeat: f64, now: Option<f64>) -> bool {
    (now.unwrap_or_else(current_time) - heartbeat) > STALE_SECONDS as f64
}

#[derive(Debug, Clone)]
pub struct CoordStore {
    pub root: PathBuf,
}

impl CoordStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn exists(&self) -> bool {
        self.root.is_dir() && !self.root.is_symlink()
    }

    fn read_json(&self, path: &Path) -> Option<Map<String, Value>> {
        let content = fs::read_to_string(path).ok()?;
        match serde_json::from_str::<Value>(&content).ok()? {
            Value::Object(map) => Some(map),
            _ => None,
        }
    }

    fn list_files(&self, sub: &str, extension: &str) -> Vec<PathBuf> {
        let dir = self.root.join(sub);
        if dir.is_symlink() || !dir.is_dir() {
            return Vec::new();
        }
        let mut paths = Vec::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if !p.is_symlink()
                    && p.is_file()
                    && p.extension().and_then(|s| s.to_str()) == Some(extension)
                {
                    paths.push(p);
                }
            }
        }
        paths.sort();
        paths
    }

    pub fn sessions(&self) -> Vec<Session> {
        let mut results = Vec::new();
        for path in self.list_files("sessions", "json") {
            if let Some(s) = self.read_json(&path).and_then(|d| Session::from_dict(&d)) {
                results.push(s);
            }
        }
        results
    }

    pub fn claims(&self) -> Vec<Claim> {
        let mut results = Vec::new();
        for path in self.list_files("claims", "json") {
            if let Some(c) = self.read_json(&path).and_then(|d| Claim::from_dict(&d)) {
                results.push(c);
            }
        }
        results
    }

    pub fn queries(&self) -> Vec<Query> {
        let mut results = Vec::new();
        for path in self.list_files("queries", "md") {
            let file_name = path.file_name().unwrap_or_default().to_string_lossy();
            if !file_name.starts_with("q-") {
                continue;
            }
            if let Some(q) = fs::read_to_string(&path)
                .ok()
                .and_then(|text| decode_query_fields(&text))
                .and_then(|fields| Query::from_dict(&fields))
            {
                results.push(q);
            }
        }
        results
    }

    fn transition(
        &self,
        query_id: &str,
        allowed: &[&str],
        updates: &[(String, Value)],
    ) -> Result<(), CoordError> {
        if !is_valid_query_id(query_id) {
            return Err(CoordError::InvalidInput("invalid query id".to_string()));
        }

        let queries_dir = self.root.join("queries");
        let query_path = queries_dir.join(format!("{query_id}.md"));

        let lock_path = self.root.join(".lock");
        let lock_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&lock_path)?;
        let _guard = exclusive_lock(&lock_file)?;

        let text = fs::read_to_string(&query_path)?;
        let mut fields = decode_query_fields(&text)
            .ok_or_else(|| CoordError::InvalidInput("unreadable query".to_string()))?;

        let current_status = fields.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if !allowed.contains(&current_status) {
            return Err(CoordError::AnswerConflict(current_status.to_string()));
        }

        for (k, v) in updates {
            fields.insert(k.clone(), v.clone());
        }

        let encoded = encode_query_fields(&fields);

        let temp_path = queries_dir.join(format!(".tmp-{}", uuid::Uuid::new_v4()));
        {
            let mut file = File::create(&temp_path)?;
            file.write_all(encoded.as_bytes())?;
            file.flush()?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o600));
        }

        fs::rename(temp_path, query_path)?;
        Ok(())
    }

    pub fn answer(&self, query_id: &str, text: &str, via: &str) -> Result<(), CoordError> {
        if via != "tandem" && via != "cli" {
            return Err(CoordError::InvalidInput(
                "via must be tandem or cli".to_string(),
            ));
        }

        let now = current_time();
        let updates = [
            ("status".to_string(), Value::String("answered".to_string())),
            ("answer".to_string(), Value::String(text.to_string())),
            ("answered_via".to_string(), Value::String(via.to_string())),
            (
                "answered_at".to_string(),
                serde_json::Number::from_f64(now)
                    .map(Value::Number)
                    .unwrap_or(Value::Null),
            ),
        ];

        self.transition(query_id, &["open"], &updates)
    }

    pub fn withdraw(&self, query_id: &str) -> Result<(), CoordError> {
        let updates = [("status".to_string(), Value::String("withdrawn".to_string()))];
        self.transition(query_id, &["open"], &updates)
    }

    pub fn write_session(
        &self,
        session_id: &str,
        cli: &str,
        role: &str,
        pane: &str,
        cwd: &str,
        label: &str,
    ) -> Result<(), CoordError> {
        self.write_session_with_name(session_id, cli, role, pane, cwd, label, None)
    }

    pub fn write_session_with_name(
        &self,
        session_id: &str,
        cli: &str,
        role: &str,
        pane: &str,
        cwd: &str,
        label: &str,
        name: Option<&str>,
    ) -> Result<(), CoordError> {
        if !is_valid_session_id(session_id) {
            return Err(CoordError::InvalidInput("invalid session id".to_string()));
        }

        let dir = self.root.join("sessions");
        fs::create_dir_all(&dir)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
        }

        let now = current_time();
        let mut record = Map::new();
        record.insert("id".to_string(), Value::String(session_id.to_string()));
        record.insert("cli".to_string(), Value::String(cli.to_string()));
        record.insert("role".to_string(), Value::String(role.to_string()));
        record.insert("model".to_string(), Value::String(label.to_string()));
        record.insert("pane".to_string(), Value::String(pane.to_string()));
        record.insert("cwd".to_string(), Value::String(cwd.to_string()));
        record.insert("branch".to_string(), Value::String(String::new()));
        record.insert("worktree".to_string(), Value::String(String::new()));
        record.insert("task".to_string(), Value::String(String::new()));
        if let Some(n) = name.filter(|s| !s.is_empty()) {
            record.insert("name".to_string(), Value::String(n.to_string()));
        }
        if let Some(n) = serde_json::Number::from_f64(now) {
            record.insert("started".to_string(), Value::Number(n.clone()));
            record.insert("heartbeat".to_string(), Value::Number(n));
        }

        let serialized = serde_json::to_string(&Value::Object(record))?;
        let temp_path = dir.join(format!(".tmp-{}", uuid::Uuid::new_v4()));
        {
            let mut file = File::create(&temp_path)?;
            file.write_all(serialized.as_bytes())?;
            file.flush()?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o600));
        }

        fs::rename(temp_path, dir.join(format!("{session_id}.json")))?;
        Ok(())
    }

    pub fn remove_session(&self, session_id: &str) {
        if is_valid_session_id(session_id) {
            let path = self
                .root
                .join("sessions")
                .join(format!("{session_id}.json"));
            let _ = fs::remove_file(path);
        }
    }
}
