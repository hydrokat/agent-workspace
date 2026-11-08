use std::collections::HashMap;
use std::path::PathBuf;

pub const DEFAULT_UPDATE_SPEC: &str = "hydrokat/agent-workspace";
pub const DEFAULT_UPDATE_INTERVAL: u64 = 14400;
pub const MIN_UPDATE_INTERVAL: u64 = 600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub coord_dir: Option<PathBuf>,
    pub color: bool,
    pub tmux_pane: String,
    pub workspace: Option<PathBuf>,
    pub state_dir: PathBuf,
    pub update_spec: String,
    pub session_id: String,
    pub claude_dir: PathBuf,
    pub update_interval: u64,
    pub update_check: bool,
}

pub trait EnvSource {
    fn get(&self, key: &str) -> Option<String>;
}

impl EnvSource for HashMap<String, String> {
    fn get(&self, key: &str) -> Option<String> {
        self.get(key).cloned()
    }
}

pub struct SystemEnv;

impl EnvSource for SystemEnv {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn is_valid_session_id(s: &str) -> bool {
    if let Some((prefix, num)) = s.split_once('-') {
        !prefix.is_empty()
            && prefix.chars().all(|c| c.is_ascii_lowercase())
            && (1..=2).contains(&num.len())
            && num.chars().all(|c| c.is_ascii_digit())
    } else {
        false
    }
}

fn is_valid_update_spec(spec: &str) -> bool {
    let (base, ref_part) = match spec.split_once('#') {
        Some((b, r)) => (b, Some(r)),
        None => (spec, None),
    };

    if let Some(r) = ref_part {
        let valid_chars = r
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '/' || c == '-');
        if r.is_empty() || !valid_chars {
            return false;
        }
    }

    if let Some((owner, repo)) = base.split_once('/') {
        if owner.is_empty()
            || repo.is_empty()
            || !owner.chars().next().unwrap().is_ascii_alphanumeric()
            || !owner
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
            || !repo
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
        {
            return false;
        }
        true
    } else {
        false
    }
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Self::from_env_source(&SystemEnv)
    }

    pub fn from_env_source<E: EnvSource>(env: &E) -> Result<Self, String> {
        let home = home_dir();

        let coord_dir = match env.get("TANDEM_COORD_DIR") {
            Some(raw) if !raw.is_empty() => {
                let p = PathBuf::from(&raw);
                if !p.is_absolute() {
                    return Err("TANDEM_COORD_DIR must be an absolute path".to_string());
                }
                Some(p)
            }
            _ => None,
        };

        let workspace = match env.get("TANDEM_WORKSPACE") {
            Some(raw) if !raw.is_empty() => {
                let p = PathBuf::from(&raw);
                if p.is_absolute() { Some(p) } else { None }
            }
            _ => None,
        };

        let state_dir = match env.get("XDG_STATE_HOME") {
            Some(raw) if !raw.is_empty() => {
                let p = PathBuf::from(&raw);
                if p.is_absolute() {
                    p.join("tandem")
                } else {
                    home.join(".local").join("state").join("tandem")
                }
            }
            _ => home.join(".local").join("state").join("tandem"),
        };

        let session_id = match env.get("TANDEM_SESSION_ID") {
            Some(raw) if is_valid_session_id(&raw) => raw,
            _ => String::new(),
        };

        let claude_dir = match env.get("CLAUDE_CONFIG_DIR") {
            Some(raw) if !raw.is_empty() => {
                let p = PathBuf::from(&raw);
                if p.is_absolute() {
                    p
                } else {
                    home.join(".claude")
                }
            }
            _ => home.join(".claude"),
        };

        let update_spec = match env.get("TANDEM_UPDATE_SPEC") {
            Some(raw) if !raw.is_empty() => raw,
            _ => DEFAULT_UPDATE_SPEC.to_string(),
        };
        if !is_valid_update_spec(&update_spec) {
            return Err(
                "TANDEM_UPDATE_SPEC must look like owner/repo or owner/repo#ref".to_string(),
            );
        }

        let update_interval = match env.get("TANDEM_UPDATE_INTERVAL") {
            Some(raw) if !raw.is_empty() => match raw.parse::<u64>() {
                Ok(val) => val.max(MIN_UPDATE_INTERVAL),
                Err(_) => DEFAULT_UPDATE_INTERVAL,
            },
            _ => DEFAULT_UPDATE_INTERVAL,
        };

        let mut update_check = env.get("TANDEM_UPDATE_CHECK").as_deref() != Some("0");
        if update_check {
            if let Some(ref ws) = workspace {
                update_check = ws.join(".agent-workspace").join("manifest.json").exists();
            } else {
                update_check = false;
            }
        }

        let color = env.get("NO_COLOR").unwrap_or_default().is_empty();
        let tmux_pane = env.get("TMUX_PANE").unwrap_or_default();

        Ok(Config {
            coord_dir,
            color,
            tmux_pane,
            workspace,
            state_dir,
            update_spec,
            session_id,
            claude_dir,
            update_interval,
            update_check,
        })
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::from_env_source(&HashMap::<String, String>::new())
            .expect("default config from empty env is always valid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_when_env_empty() {
        let env = HashMap::new();
        let cfg = Config::from_env_source(&env).unwrap();
        assert_eq!(cfg.coord_dir, None);
        assert!(cfg.color);
        assert_eq!(cfg.update_interval, 14400);
        assert!(!cfg.update_check);
    }

    #[test]
    fn test_reads_coord_dir() {
        let mut env = HashMap::new();
        env.insert("TANDEM_COORD_DIR".to_string(), "/tmp/coord".to_string());
        let cfg = Config::from_env_source(&env).unwrap();
        assert_eq!(cfg.coord_dir, Some(PathBuf::from("/tmp/coord")));
    }

    #[test]
    fn test_relative_coord_dir_rejected() {
        let mut env = HashMap::new();
        env.insert("TANDEM_COORD_DIR".to_string(), "relative/path".to_string());
        assert!(Config::from_env_source(&env).is_err());
    }

    #[test]
    fn test_no_color_disables_color() {
        let mut env = HashMap::new();
        env.insert("NO_COLOR".to_string(), "1".to_string());
        assert!(!Config::from_env_source(&env).unwrap().color);

        env.insert("NO_COLOR".to_string(), "".to_string());
        assert!(Config::from_env_source(&env).unwrap().color);
    }

    #[test]
    fn test_session_id_validation() {
        let mut env = HashMap::new();
        env.insert("TANDEM_SESSION_ID".to_string(), "planner-1".to_string());
        assert_eq!(
            Config::from_env_source(&env).unwrap().session_id,
            "planner-1"
        );

        env.insert("TANDEM_SESSION_ID".to_string(), "../../x".to_string());
        assert_eq!(Config::from_env_source(&env).unwrap().session_id, "");
    }

    #[test]
    fn test_update_interval() {
        let mut env = HashMap::new();
        env.insert("TANDEM_UPDATE_INTERVAL".to_string(), "3600".to_string());
        assert_eq!(Config::from_env_source(&env).unwrap().update_interval, 3600);

        env.insert("TANDEM_UPDATE_INTERVAL".to_string(), "10".to_string());
        assert_eq!(Config::from_env_source(&env).unwrap().update_interval, 600);

        env.insert(
            "TANDEM_UPDATE_INTERVAL".to_string(),
            "not-a-number".to_string(),
        );
        assert_eq!(
            Config::from_env_source(&env).unwrap().update_interval,
            14400
        );
    }
}
