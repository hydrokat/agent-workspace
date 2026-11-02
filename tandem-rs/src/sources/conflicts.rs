use crate::coordination::schema::Claim;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub first: String,
    pub path: String,
    pub second: String,
}

impl Conflict {
    pub fn new(
        first: impl Into<String>,
        second: impl Into<String>,
        path: impl Into<String>,
    ) -> Self {
        Self {
            first: first.into(),
            second: second.into(),
            path: path.into(),
        }
    }
}

fn normalize_path(path: &str) -> String {
    let replaced = path.replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    for seg in replaced.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            parts.pop();
        } else {
            parts.push(seg);
        }
    }
    parts.join("/")
}

fn overlap(a: &str, b: &str) -> Option<String> {
    if a.is_empty() || b.is_empty() || a == "." || b == "." {
        return None;
    }
    if a == b || b.starts_with(&format!("{a}/")) {
        return Some(a.to_string());
    }
    if a.starts_with(&format!("{b}/")) {
        return Some(b.to_string());
    }
    None
}

pub fn find_conflicts(claims: &[Claim]) -> Vec<Conflict> {
    let mut found = Vec::new();
    for (i, first) in claims.iter().enumerate() {
        for second in &claims[i + 1..] {
            let mut matched = false;
            for pa in &first.paths {
                let norm_a = normalize_path(pa);
                for pb in &second.paths {
                    let norm_b = normalize_path(pb);
                    if let Some(hit) = overlap(&norm_a, &norm_b) {
                        found.push(Conflict::new(&first.task, &second.task, hit));
                        matched = true;
                        break;
                    }
                }
                if matched {
                    break;
                }
            }
        }
    }
    found
}
