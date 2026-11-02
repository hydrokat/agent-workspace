use crate::sanitize::clean;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

pub const MAX_BYTES: u64 = 1_000_000;
const STATUS_OPTIONS: &[&str] = &["Pending", "In Progress", "Blocked", "In Review", "Done"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub objectives_done: usize,
    #[serde(default)]
    pub objectives_total: usize,
}

impl Task {
    pub fn new(id: impl Into<String>, title: impl Into<String>, status: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            status: status.into(),
            objectives_done: 0,
            objectives_total: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Phase {
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub warning: String,
}

impl Phase {
    pub fn new(name: impl Into<String>, tasks: Vec<Task>) -> Self {
        Self {
            name: name.into(),
            title: String::new(),
            branch: String::new(),
            target: String::new(),
            tasks,
            warning: String::new(),
        }
    }

    pub fn with_warning(name: impl Into<String>, warning: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            title: String::new(),
            branch: String::new(),
            target: String::new(),
            tasks: Vec::new(),
            warning: warning.into(),
        }
    }

    pub fn total(&self) -> usize {
        self.tasks.len()
    }

    pub fn done(&self) -> usize {
        self.tasks.iter().filter(|t| t.status == "Done").count()
    }
}

pub(crate) fn read_file_capped(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    file.by_ref().take(MAX_BYTES).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn extract_task_status(text: Option<&str>) -> Option<String> {
    let text = text?;
    let mut in_status = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            in_status = trimmed == "## Status";
        } else if in_status
            && let Some(rest) = trimmed
                .strip_prefix("- [x] ")
                .or_else(|| trimmed.strip_prefix("- [X] "))
        {
            for &status in STATUS_OPTIONS {
                if let Some(after) = rest.strip_prefix(status)
                    && (after.is_empty()
                        || after
                            .chars()
                            .next()
                            .is_some_and(|c| !c.is_alphanumeric() && c != '_'))
                {
                    return Some(status.to_string());
                }
            }
        }
    }
    None
}

fn is_task_id(id: &str) -> bool {
    let digits = if let Some(rest) = id.strip_prefix("t-") {
        rest
    } else if let Some(rest) = id.strip_prefix("task-") {
        rest
    } else {
        return false;
    };
    digits.len() == 3 && digits.chars().all(|c| c.is_ascii_digit())
}

fn parse_timeline_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("- **")?;
    let colon_idx = rest.find("**:")?;
    let task_id = &rest[..colon_idx];
    if !is_task_id(task_id) {
        return None;
    }
    let after_colon = &rest[colon_idx + 3..];
    let close_paren_idx = after_colon.rfind(')')?;
    let before_close = &after_colon[..close_paren_idx];
    let open_paren_idx = before_close.rfind('(')?;
    let status_candidate = &before_close[open_paren_idx + 1..];
    if after_colon[close_paren_idx + 1..].trim().is_empty() {
        for &status in STATUS_OPTIONS {
            if status_candidate == status {
                return Some((task_id.to_string(), status.to_string()));
            }
        }
    }
    None
}

fn parse_task_line(line: &str) -> Option<(bool, String, String)> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("- [")?;
    let checked_char = rest.chars().next()?;
    let checked = checked_char == 'x' || checked_char == 'X';
    let after_box = &rest[checked_char.len_utf8()..];
    let after_bracket = after_box.strip_prefix("] ")?.trim_start();
    let colon_idx = after_bracket.find(':')?;
    let task_id = &after_bracket[..colon_idx];
    if !is_task_id(task_id) {
        return None;
    }
    let title = after_bracket[colon_idx + 1..].trim();
    Some((checked, task_id.to_string(), title.to_string()))
}

pub fn is_phase_dir(name: &str) -> bool {
    if let Some(rest) = name.strip_prefix("phase-") {
        return rest.len() == 3 && rest.chars().all(|c| c.is_ascii_digit());
    }
    if let Some(rest) = name.strip_prefix("p-") {
        if rest.len() < 5 {
            return false;
        }
        let (num, dash_and_slug) = rest.split_at(3);
        if !num.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        if !dash_and_slug.starts_with('-') {
            return false;
        }
        let slug = &dash_and_slug[1..];
        return !slug.is_empty()
            && slug
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    }
    false
}

fn extract_task_objectives(text: Option<&str>) -> (usize, usize) {
    let Some(text) = text else {
        return (0, 0);
    };
    let mut in_objectives = false;
    let mut done = 0;
    let mut total = 0;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            in_objectives = trimmed == "## Objectives";
        } else if in_objectives {
            if trimmed.starts_with("- [x] ") || trimmed.starts_with("- [X] ") {
                done += 1;
                total += 1;
            } else if trimmed.starts_with("- [ ] ") {
                total += 1;
            }
        }
    }
    (done, total)
}

fn load_phase_dir(directory: &Path) -> Phase {
    let dir_name = directory
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let impl_text = match read_file_capped(&directory.join("impl.md")) {
        Some(t) => t,
        None => {
            return Phase::with_warning(
                &dir_name,
                format!("{dir_name}/impl.md missing or unreadable"),
            );
        }
    };

    let mut phase_title = String::new();
    let mut branch = String::new();
    let mut target = String::new();
    let mut in_branch_strategy = false;

    let mut timeline = HashMap::new();
    for line in impl_text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("# Implementation Plan:") {
            phase_title = clean(rest).trim().to_string();
        } else if trimmed.starts_with("## Branch Strategy") {
            in_branch_strategy = true;
        } else if trimmed.starts_with("## ") {
            in_branch_strategy = false;
        } else if in_branch_strategy {
            if let Some(rest) = trimmed
                .strip_prefix("- **Working Branch**:")
                .or_else(|| trimmed.strip_prefix("- Working Branch:"))
            {
                branch = clean(rest).trim().to_string();
            } else if let Some(rest) = trimmed
                .strip_prefix("- **Merge Target**:")
                .or_else(|| trimmed.strip_prefix("- Merge Target:"))
            {
                target = clean(rest).trim().to_string();
            }
        }

        if let Some((task_id, status)) = parse_timeline_line(line) {
            timeline.insert(task_id, status);
        }
    }

    let mut tasks = Vec::new();
    for line in impl_text.lines() {
        let (checked, task_id, title) = match parse_task_line(line) {
            Some(t) => t,
            None => continue,
        };

        let task_file = directory.join(format!("{task_id}.md"));
        let task_content = read_file_capped(&task_file);
        let status = extract_task_status(task_content.as_deref())
            .or_else(|| timeline.get(&task_id).cloned())
            .unwrap_or_else(|| {
                if checked {
                    "Done".to_string()
                } else {
                    "Pending".to_string()
                }
            });

        let (objectives_done, objectives_total) = extract_task_objectives(task_content.as_deref());

        tasks.push(Task {
            id: task_id,
            title: clean(&title).trim().to_string(),
            status,
            objectives_done,
            objectives_total,
        });
    }

    Phase {
        name: dir_name,
        title: phase_title,
        branch,
        target,
        tasks,
        warning: String::new(),
    }
}

pub fn load_phases(workspace: &Path) -> Vec<Phase> {
    let specs = workspace.join("specs");
    let meta = match std::fs::symlink_metadata(&specs) {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    if !meta.is_dir() || meta.is_symlink() {
        return Vec::new();
    }

    let read_dir = match std::fs::read_dir(&specs) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut entries: Vec<std::fs::DirEntry> = read_dir.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());

    let mut phases = Vec::new();
    for entry in entries {
        let entry_meta = match entry.path().symlink_metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if entry_meta.is_dir() && !entry_meta.is_symlink() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if is_phase_dir(&name) {
                phases.push(load_phase_dir(&entry.path()));
            }
        }
    }

    phases
}
