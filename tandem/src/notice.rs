use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::sanitize::clean;
use crate::versions::{is_at_least, parse_version};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub from_version: String,
    pub to_version: String,
    pub at: u64,
    pub seen: bool,
}

pub fn notice_path(workspace: &Path) -> PathBuf {
    workspace.join(".agent-workspace").join("update-notice.env")
}

fn is_valid_notice_line(line: &str) -> bool {
    if let Some((key, val)) = line.split_once('=') {
        !key.is_empty()
            && key.chars().all(|c| c.is_ascii_uppercase() || c == '_')
            && !val.is_empty()
            && val
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    } else {
        false
    }
}

pub fn read_notice(workspace: &Path) -> Option<Notice> {
    let text = fs::read_to_string(notice_path(workspace)).ok()?;
    let mut data = BTreeMap::new();
    let known_keys: BTreeSet<&str> = [
        "TANDEM_UPDATE_FROM",
        "TANDEM_UPDATE_TO",
        "TANDEM_UPDATE_AT",
        "TANDEM_UPDATE_SEEN",
    ]
    .into_iter()
    .collect();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !is_valid_notice_line(trimmed) {
            return None;
        }
        let (key, val) = trimmed.split_once('=')?;
        if !known_keys.contains(key) {
            return None;
        }
        data.insert(key.to_string(), val.to_string());
    }

    let from_v = data.get("TANDEM_UPDATE_FROM")?;
    let to_v = data.get("TANDEM_UPDATE_TO")?;
    let at_str = data.get("TANDEM_UPDATE_AT")?;
    let at = at_str.parse::<u64>().ok()?;
    let seen = data.get("TANDEM_UPDATE_SEEN").map(|s| s.as_str()) == Some("1");

    Some(Notice {
        from_version: from_v.clone(),
        to_version: to_v.clone(),
        at,
        seen,
    })
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("env.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn write_notice(
    workspace: &Path,
    from_version: &str,
    to_version: &str,
    at: Option<u64>,
) -> std::io::Result<()> {
    let path = notice_path(workspace);
    let ts = at.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    });
    let content = format!(
        "TANDEM_UPDATE_FROM={from_version}\n\
         TANDEM_UPDATE_TO={to_version}\n\
         TANDEM_UPDATE_AT={ts}\n"
    );
    write_atomic(&path, &content)
}

pub fn mark_seen(workspace: &Path) -> std::io::Result<()> {
    let notice = match read_notice(workspace) {
        Some(n) => n,
        None => return Ok(()),
    };
    if notice.seen {
        return Ok(());
    }
    let path = notice_path(workspace);
    let content = format!(
        "TANDEM_UPDATE_FROM={}\n\
         TANDEM_UPDATE_TO={}\n\
         TANDEM_UPDATE_AT={}\n\
         TANDEM_UPDATE_SEEN=1\n",
        notice.from_version, notice.to_version, notice.at
    );
    write_atomic(&path, &content)
}

pub fn headline(bullet: &str) -> String {
    let trimmed = bullet.trim();
    let text = if let Some(rest) = trimmed.strip_prefix("- ") {
        rest.trim()
    } else if let Some(rest) = trimmed.strip_prefix("* ") {
        rest.trim()
    } else {
        trimmed
    };

    if let Some(rest) = text.strip_prefix("**")
        && let Some((bold, _)) = rest.split_once("**")
    {
        return clean(bold.trim());
    }

    if let Some(period_idx) = text.find('.') {
        return clean(&text[..=period_idx]);
    }

    clean(text)
}

pub fn whats_new(
    changelog_text: &str,
    from_v: &str,
    to_v: &str,
    limit: Option<usize>,
) -> Vec<String> {
    let from_ver = match parse_version(from_v) {
        Some(v) => v,
        None => return Vec::new(),
    };
    let to_ver = match parse_version(to_v) {
        Some(v) => v,
        None => return Vec::new(),
    };

    let max_count = limit.unwrap_or(8);
    let mut sections = Vec::new();
    let mut current_id: Option<String> = None;
    let mut current_bullets: Vec<String> = Vec::new();

    for line in changelog_text.lines() {
        if let Some(rest) = line.strip_prefix("## [")
            && let Some((sec_id, _)) = rest.split_once(']')
        {
            if let Some(id) = current_id.take() {
                sections.push((id, current_bullets));
                current_bullets = Vec::new();
            }
            current_id = Some(sec_id.to_string());
        } else if current_id.is_some() && (line.starts_with("- ") || line.starts_with("* ")) {
            current_bullets.push(line.to_string());
        }
    }
    if let Some(id) = current_id {
        sections.push((id, current_bullets));
    }

    let mut matching = Vec::new();
    let mut unreleased = Vec::new();

    for (sec_id, bullets) in sections {
        if sec_id.eq_ignore_ascii_case("unreleased") {
            unreleased = bullets;
            continue;
        }
        if let Some(sec_ver) = parse_version(&sec_id)
            && !is_at_least(&from_ver, &sec_ver)
            && is_at_least(&to_ver, &sec_ver)
        {
            matching.extend(bullets);
        }
    }

    let source = if !matching.is_empty() {
        matching
    } else {
        unreleased
    };

    let mut results = Vec::new();
    for b in source {
        let h = headline(&b);
        if !h.is_empty() {
            results.push(h);
            if results.len() >= max_count {
                break;
            }
        }
    }
    results
}
