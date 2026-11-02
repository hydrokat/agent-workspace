use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const STALE_SECONDS: u64 = 600;

pub fn extract_str(d: &Map<String, Value>, key: &str) -> String {
    match d.get(key) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

pub fn extract_num(d: &Map<String, Value>, key: &str) -> f64 {
    match d.get(key) {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

pub fn extract_strs(d: &Map<String, Value>, key: &str) -> Vec<String> {
    match d.get(key) {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub cli: String,
    pub role: String,
    pub model: String,
    pub pane: String,
    pub cwd: String,
    pub branch: String,
    pub worktree: String,
    pub task: String,
    pub heartbeat: f64,
}

impl Session {
    pub fn from_dict(d: &Map<String, Value>) -> Option<Self> {
        let sid = extract_str(d, "id");
        if sid.is_empty() {
            return None;
        }
        Some(Self {
            id: sid,
            cli: extract_str(d, "cli"),
            role: extract_str(d, "role"),
            model: extract_str(d, "model"),
            pane: extract_str(d, "pane"),
            cwd: extract_str(d, "cwd"),
            branch: extract_str(d, "branch"),
            worktree: extract_str(d, "worktree"),
            task: extract_str(d, "task"),
            heartbeat: extract_num(d, "heartbeat"),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub phase: String,
    pub task: String,
    pub session: String,
    pub branch: String,
    pub worktree: String,
    pub paths: Vec<String>,
    pub heartbeat: f64,
}

impl Claim {
    pub fn from_dict(d: &Map<String, Value>) -> Option<Self> {
        let phase = extract_str(d, "phase");
        let task = extract_str(d, "task");
        if phase.is_empty() || task.is_empty() {
            return None;
        }
        Some(Self {
            phase,
            task,
            session: extract_str(d, "session"),
            branch: extract_str(d, "branch"),
            worktree: extract_str(d, "worktree"),
            paths: extract_strs(d, "paths"),
            heartbeat: extract_num(d, "heartbeat"),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Query {
    pub id: String,
    pub status: String,
    pub asked_by: String,
    pub task: String,
    pub blocking_scope: String,
    pub created: f64,
    pub question: String,
    pub options: Vec<String>,
    pub answer: String,
    pub answered_via: String,
}

impl Query {
    pub fn from_dict(d: &Map<String, Value>) -> Option<Self> {
        let qid = extract_str(d, "id");
        let status = extract_str(d, "status");
        if qid.is_empty()
            || !matches!(
                status.as_str(),
                "open" | "answered" | "acknowledged" | "withdrawn"
            )
        {
            return None;
        }
        Some(Self {
            id: qid,
            status,
            asked_by: extract_str(d, "asked_by"),
            task: extract_str(d, "task"),
            blocking_scope: extract_str(d, "blocking_scope"),
            created: extract_num(d, "created"),
            question: extract_str(d, "question"),
            options: extract_strs(d, "options"),
            answer: extract_str(d, "answer"),
            answered_via: extract_str(d, "answered_via"),
        })
    }
}

pub fn decode_query_fields(text: &str) -> Option<Map<String, Value>> {
    let lines: Vec<&str> = text.split('\n').collect();
    if lines.is_empty() || lines[0] != "---" {
        return None;
    }
    let mut fields = Map::new();
    for line in lines.iter().skip(1) {
        if *line == "---" {
            return Some(fields);
        }
        if let Some((key, raw)) = line.split_once(": ") {
            if let Ok(val) = serde_json::from_str(raw) {
                fields.insert(key.to_string(), val);
            } else {
                return None;
            }
        } else {
            return None;
        }
    }
    None
}

pub fn encode_query_fields(fields: &Map<String, Value>) -> String {
    let mut lines = Vec::new();
    lines.push("---".to_string());
    for (k, v) in fields {
        lines.push(format!("{}: {}", k, serde_json::to_string(v).unwrap()));
    }
    lines.push("---".to_string());
    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_query_fields() {
        let mut fields = Map::new();
        fields.insert("id".to_string(), Value::String("q-1234".to_string()));
        fields.insert("status".to_string(), Value::String("open".to_string()));
        fields.insert(
            "blocking".to_string(),
            Value::Array(vec![Value::String("src/main.rs".to_string())]),
        );

        let encoded = encode_query_fields(&fields);
        let decoded = decode_query_fields(&encoded).unwrap();
        assert_eq!(decoded, fields);
    }

    #[test]
    fn test_claim_lenient_from_dict() {
        let mut d = Map::new();
        d.insert("phase".to_string(), Value::String("p-001".to_string()));
        d.insert("task".to_string(), Value::String("t-002".to_string()));
        d.insert(
            "paths".to_string(),
            Value::String("not an array".to_string()),
        );

        let c = Claim::from_dict(&d).unwrap();
        assert_eq!(c.phase, "p-001");
        assert_eq!(c.task, "t-002");
        assert_eq!(c.paths, Vec::<String>::new());
    }
}
