use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct InvalidMinVersionsError(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct Version {
    pub parts: Vec<u32>,
    pub suffix: String,
}

impl Version {
    pub fn new(parts: Vec<u32>, suffix: impl Into<String>) -> Self {
        Self {
            parts,
            suffix: suffix.into(),
        }
    }

    pub fn text(&self) -> String {
        let joined = self
            .parts
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(".");
        format!("{}{}", joined, self.suffix)
    }
}

pub fn parse_version(raw: &str) -> Option<Version> {
    let chars: Vec<char> = raw.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len && !chars[i].is_ascii_digit() {
        i += 1;
    }
    if i >= len {
        return None;
    }

    let mut parts = Vec::new();
    while i < len && chars[i].is_ascii_digit() {
        let mut num = 0u32;
        while i < len && chars[i].is_ascii_digit() {
            num = num
                .saturating_mul(10)
                .saturating_add(chars[i].to_digit(10).unwrap());
            i += 1;
        }
        parts.push(num);

        if i + 1 < len && chars[i] == '.' && chars[i + 1].is_ascii_digit() {
            i += 1;
        } else {
            break;
        }
    }

    let suffix = if i < len && chars[i].is_ascii_lowercase() {
        chars[i].to_string()
    } else {
        String::new()
    };

    Some(Version { parts, suffix })
}

pub fn is_at_least(actual: &Version, minimum: &Version) -> bool {
    let max_len = actual.parts.len().max(minimum.parts.len());
    let mut a_padded = actual.parts.clone();
    a_padded.resize(max_len, 0);
    let mut m_padded = minimum.parts.clone();
    m_padded.resize(max_len, 0);

    (&a_padded, &actual.suffix) >= (&m_padded, &minimum.suffix)
}

pub fn parse_min_versions(path: &Path) -> Result<HashMap<String, String>, InvalidMinVersionsError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| InvalidMinVersionsError(format!("{}: {}", path.display(), e)))?;
    let filename = path.file_name().unwrap_or_default().to_string_lossy();
    parse_min_versions_str(&content, &filename)
}

pub fn parse_min_versions_str(
    content: &str,
    filename: &str,
) -> Result<HashMap<String, String>, InvalidMinVersionsError> {
    let mut result = HashMap::new();
    for (num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            let key_ok = !key.is_empty() && key.chars().all(|c| c.is_ascii_uppercase() || c == '_');
            let val_ok = !val.is_empty()
                && val
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
            if key_ok && val_ok {
                result.insert(key.to_string(), val.to_string());
                continue;
            }
        }
        return Err(InvalidMinVersionsError(format!(
            "{}:{}: invalid line",
            filename,
            num + 1
        )));
    }
    Ok(result)
}

pub fn default_which(cli: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(cli);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn default_runner(argv: &[&str]) -> (i32, String) {
    if argv.is_empty() {
        return (1, String::new());
    }
    let mut cmd = Command::new(argv[0]);
    if argv.len() > 1 {
        cmd.args(&argv[1..]);
    }
    match cmd.output() {
        Ok(out) => {
            let code = out.status.code().unwrap_or(1);
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            (code, text)
        }
        Err(_) => (1, String::new()),
    }
}

pub fn installed_version(cli: &str) -> String {
    installed_version_with(cli, default_which, default_runner)
}

pub fn installed_version_with<W, R>(cli: &str, which: W, runner: R) -> String
where
    W: Fn(&str) -> Option<PathBuf>,
    R: Fn(&[&str]) -> (i32, String),
{
    let path = match which(cli) {
        Some(p) => p,
        None => return String::new(),
    };
    let path_str = match path.to_str() {
        Some(s) => s,
        None => return String::new(),
    };
    let (code, out) = runner(&[path_str, "--version"]);
    if code == 0 {
        parse_version(&out).map(|v| v.text()).unwrap_or_default()
    } else {
        String::new()
    }
}

pub fn verify_release_versions(tag: &str, repo_root: &Path) -> (bool, String) {
    if !tag.starts_with('v') {
        return (false, format!("tag '{tag}' does not start with 'v'"));
    }
    let expected = &tag[1..];

    let pkg_json = repo_root.join("package.json");
    if !pkg_json.is_file() {
        return (
            false,
            format!("package.json not found at {}", pkg_json.display()),
        );
    }
    let node_version = match std::fs::read_to_string(&pkg_json)
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .and_then(|v| v.get("version").and_then(|s| s.as_str().map(String::from)))
    {
        Some(v) => v,
        None => return (false, "failed to read package.json version".to_string()),
    };
    if node_version != expected {
        return (
            false,
            format!("package.json version '{node_version}' != tag version '{expected}'"),
        );
    }

    let cargo_candidates = [
        repo_root.join("tandem-rs").join("Cargo.toml"),
        repo_root.join("tandem").join("Cargo.toml"),
        repo_root.join("Cargo.toml"),
    ];
    let mut cargo_version = None;
    for candidate in cargo_candidates {
        if candidate.is_file() {
            if let Ok(content) = std::fs::read_to_string(&candidate) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    let version_opt = trimmed
                        .strip_prefix("version")
                        .and_then(|rest| rest.split_once('='))
                        .map(|(_, val)| val.trim().trim_matches('"').trim_matches('\''));
                    if let Some(clean_v) = version_opt {
                        cargo_version = Some(clean_v.to_string());
                        break;
                    }
                }
            }
            if cargo_version.is_some() {
                break;
            }
        }
    }

    if let Some(cv) = cargo_version {
        if cv != expected {
            return (
                false,
                format!("Cargo.toml version '{cv}' != tag version '{expected}'"),
            );
        }
    } else {
        return (false, "Cargo.toml not found".to_string());
    }

    (true, format!("all versions match {expected}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() {
        assert_eq!(
            parse_version("1.5.0"),
            Some(Version::new(vec![1, 5, 0], ""))
        );
        assert_eq!(
            parse_version("v2.0.1"),
            Some(Version::new(vec![2, 0, 1], ""))
        );
        assert_eq!(parse_version("3.3a"), Some(Version::new(vec![3, 3], "a")));
        assert_eq!(parse_version("invalid"), None);
    }

    #[test]
    fn test_is_at_least() {
        let v150 = parse_version("1.5.0").unwrap();
        let v140 = parse_version("1.4.0").unwrap();
        let v150a = parse_version("1.5.0a").unwrap();

        assert!(is_at_least(&v150, &v140));
        assert!(is_at_least(&v150, &v150));
        assert!(!is_at_least(&v140, &v150));
        assert!(is_at_least(&v150a, &v150));
    }

    #[test]
    fn test_parse_min_versions_str() {
        let valid = "TANDEM_MIN_NODE=18.0.0\nTANDEM_MIN_TMUX=3.2\n";
        let map = parse_min_versions_str(valid, "min.env").unwrap();
        assert_eq!(map.get("TANDEM_MIN_NODE"), Some(&"18.0.0".to_string()));
        assert_eq!(map.get("TANDEM_MIN_TMUX"), Some(&"3.2".to_string()));

        let invalid = "bad_key=18.0.0\n";
        assert!(parse_min_versions_str(invalid, "min.env").is_err());
    }

    #[test]
    fn test_installed_version_with() {
        let which = |name: &str| {
            if name == "claude" {
                Some(PathBuf::from("/usr/bin/claude"))
            } else {
                None
            }
        };
        let runner = |argv: &[&str]| {
            if argv == ["/usr/bin/claude", "--version"] {
                (0, "claude 1.2.3".to_string())
            } else {
                (1, String::new())
            }
        };

        assert_eq!(installed_version_with("claude", which, runner), "1.2.3");
        assert_eq!(installed_version_with("missing", which, runner), "");
    }
}
