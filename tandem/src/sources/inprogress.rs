use crate::sanitize::clean;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

const MAX_BYTES: u64 = 1_000_000;
const EXAMPLE_IDS: &[&str] = &["abc-1234", "def-5678"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InProgressEntry {
    pub spec: String,
    pub title: String,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
}

pub fn parse_in_progress(path: &Path) -> Vec<InProgressEntry> {
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let mut buf = Vec::new();
    if file.by_ref().take(MAX_BYTES).read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&buf);

    let mut entries = Vec::new();
    let mut current: Option<(String, String, BTreeMap<String, String>)> = None;

    for line in text.lines() {
        if let Some((spec, title)) = parse_heading(line) {
            if let Some((s, t, f)) = current.take() {
                entries.push(InProgressEntry {
                    spec: s,
                    title: t,
                    fields: f,
                });
            }
            current = Some((clean(&spec), clean(&title), BTreeMap::new()));
            continue;
        }

        if line.starts_with('#') {
            if let Some((s, t, f)) = current.take() {
                entries.push(InProgressEntry {
                    spec: s,
                    title: t,
                    fields: f,
                });
            }
            continue;
        }

        if let Some((key, val)) = parse_field(line)
            && let Some((_, _, ref mut fields)) = current
        {
            fields.insert(key, val);
        }
    }

    if let Some((s, t, f)) = current {
        entries.push(InProgressEntry {
            spec: s,
            title: t,
            fields: f,
        });
    }

    entries.retain(|e| {
        !EXAMPLE_IDS
            .iter()
            .any(|id| e.fields.values().any(|val| val.contains(id)))
    });

    entries
}

fn parse_heading(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("###")?;
    let trimmed = rest.trim_start();
    let dash_pos = trimmed
        .find(" — ")
        .or_else(|| trimmed.find(" – "))
        .or_else(|| trimmed.find(" - "))?;
    let spec = trimmed[..dash_pos].trim();
    let title_start = if trimmed[dash_pos..].starts_with(" — ") {
        dash_pos + " — ".len()
    } else if trimmed[dash_pos..].starts_with(" – ") {
        dash_pos + " – ".len()
    } else {
        dash_pos + " - ".len()
    };
    let title = trimmed[title_start..].trim();
    if spec.is_empty() || title.is_empty() {
        None
    } else {
        Some((spec.to_string(), title.to_string()))
    }
}

fn parse_field(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("- **")?;
    let colon_idx = rest.find(":**")?;
    let key = rest[..colon_idx].trim().to_string();
    let val_raw = &rest[colon_idx + 3..];
    let val = clean(val_raw.trim()).trim().trim_matches('`').to_string();
    Some((key, val))
}
