use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::FactoryError;
use super::approval::{ApprovalError, ensure_user_caller};
use super::events::set_private_file_mode;
use super::model::FactorySettings;
use super::store::{FactoryStore, current_time};
use crate::coordination::filelock::exclusive_lock;

pub const DEFAULT_CHECK_TIMEOUT_SECS: u64 = 1800;
pub const MAX_CHECK_COMMANDS_PER_KIND: usize = 8;
pub const MAX_CHECK_ARGV_LEN: usize = 32;

const SHELL_BINARIES: &[&str] = &["sh", "bash", "zsh", "cmd", "cmd.exe"];

fn default_timeout_secs() -> u64 {
    DEFAULT_CHECK_TIMEOUT_SECS
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChecksConfig {
    #[serde(default)]
    pub test: Vec<Vec<String>>,
    #[serde(default)]
    pub lint: Vec<Vec<String>>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub ui_paths: Vec<String>,
    #[serde(default)]
    pub confirmed_by: Option<String>,
    #[serde(default)]
    pub confirmed_at: Option<f64>,
}

impl Default for ChecksConfig {
    fn default() -> Self {
        Self {
            test: Vec::new(),
            lint: Vec::new(),
            timeout_secs: DEFAULT_CHECK_TIMEOUT_SECS,
            ui_paths: Vec::new(),
            confirmed_by: None,
            confirmed_at: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedChecks {
    target: String,
    test: Vec<Vec<String>>,
    lint: Vec<Vec<String>>,
    timeout_secs: u64,
    ui_paths: Vec<String>,
    confirmed_by: String,
    confirmed_at: String,
}

impl ConfirmedChecks {
    /// Construct a sealed `ConfirmedChecks` only from a validated, user-confirmed `ChecksConfig`.
    pub fn from_config(target: &str, cfg: &ChecksConfig) -> Result<Self, ChecksError> {
        validate_target_name(target)?;
        validate_checks_config(cfg)?;
        let confirmed_by = cfg
            .confirmed_by
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or(ChecksError::NotConfirmed)?
            .to_string();
        let confirmed_at = cfg
            .confirmed_at
            .map(|ts| format!("{ts:.3}"))
            .ok_or(ChecksError::NotConfirmed)?;

        Ok(Self {
            target: target.to_string(),
            test: cfg.test.clone(),
            lint: cfg.lint.clone(),
            timeout_secs: cfg.timeout_secs,
            ui_paths: cfg.ui_paths.clone(),
            confirmed_by,
            confirmed_at,
        })
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn test(&self) -> &[Vec<String>] {
        &self.test
    }

    pub fn lint(&self) -> &[Vec<String>] {
        &self.lint
    }

    pub fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    pub fn ui_paths(&self) -> &[String] {
        &self.ui_paths
    }

    pub fn confirmed_by(&self) -> &str {
        &self.confirmed_by
    }

    pub fn confirmed_at(&self) -> &str {
        &self.confirmed_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    Test,
    Lint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckProposal {
    pub kind: CheckKind,
    pub argv: Vec<String>,
    pub source_file: String,
    pub source_line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoverReport {
    pub target: String,
    pub target_path: String,
    pub proposals: Vec<CheckProposal>,
    pub confirmed: Option<ChecksConfig>,
    pub confirm_command: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ChecksError {
    #[error("only the user can confirm or change check commands")]
    AgentCaller,
    #[error("test and lint commands are not confirmed")]
    NotConfirmed,
    #[error("{0}")]
    InvalidInput(String),
    #[error(transparent)]
    Factory(#[from] FactoryError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl ChecksError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::AgentCaller | Self::NotConfirmed => 3,
            Self::InvalidInput(_) | Self::Factory(_) | Self::Io(_) => 1,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::AgentCaller => "agent_caller_forbidden",
            Self::NotConfirmed => "checks_not_confirmed",
            Self::InvalidInput(_) => "invalid_input",
            Self::Factory(_) | Self::Io(_) => "internal_error",
        }
    }
}

impl From<ApprovalError> for ChecksError {
    fn from(err: ApprovalError) -> Self {
        match err {
            ApprovalError::AgentCaller => Self::AgentCaller,
            ApprovalError::InvalidInput(m) | ApprovalError::Gate(m) => Self::InvalidInput(m),
            ApprovalError::Factory(f) => Self::Factory(f),
            ApprovalError::Io(e) => Self::Io(e),
        }
    }
}

pub fn validate_target_name(target: &str) -> Result<(), ChecksError> {
    let trimmed = target.trim();
    if trimmed.is_empty()
        || trimmed != target
        || trimmed.chars().any(char::is_control)
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed == "."
        || trimmed == ".."
    {
        return Err(ChecksError::InvalidInput(format!(
            "invalid target name: {target}"
        )));
    }
    Ok(())
}

fn validate_command_list(cmds: &[Vec<String>], label: &str) -> Result<(), ChecksError> {
    if cmds.len() > MAX_CHECK_COMMANDS_PER_KIND {
        return Err(ChecksError::InvalidInput(format!(
            "too many {label} commands: {} (max {MAX_CHECK_COMMANDS_PER_KIND})",
            cmds.len()
        )));
    }
    for (idx, argv) in cmds.iter().enumerate() {
        if argv.is_empty() {
            return Err(ChecksError::InvalidInput(format!(
                "{label} command #{idx} must be a non-empty argv list"
            )));
        }
        if argv.len() > MAX_CHECK_ARGV_LEN {
            return Err(ChecksError::InvalidInput(format!(
                "{label} command #{idx} has {} items (max {MAX_CHECK_ARGV_LEN})",
                argv.len()
            )));
        }
        if argv[0].trim().is_empty() {
            return Err(ChecksError::InvalidInput(format!(
                "{label} command #{idx} has an empty executable"
            )));
        }
        for arg in argv {
            if arg.chars().any(char::is_control) {
                return Err(ChecksError::InvalidInput(format!(
                    "{label} command #{idx} contains control characters"
                )));
            }
        }
        let bin_name = Path::new(&argv[0])
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&argv[0])
            .to_ascii_lowercase();
        if SHELL_BINARIES.contains(&bin_name.as_str())
            && argv[1..]
                .iter()
                .any(|flag| flag == "-c" || flag.eq_ignore_ascii_case("/c"))
        {
            return Err(ChecksError::InvalidInput(
                "Refused: a shell with an inline script is not allowed. Give the command as a list of arguments."
                    .to_string(),
            ));
        }
    }
    Ok(())
}

pub fn validate_checks_config(cfg: &ChecksConfig) -> Result<(), ChecksError> {
    validate_command_list(&cfg.test, "test")?;
    validate_command_list(&cfg.lint, "lint")?;
    if cfg.timeout_secs == 0 {
        return Err(ChecksError::InvalidInput(
            "timeout_secs must be greater than 0".to_string(),
        ));
    }
    for pat in &cfg.ui_paths {
        if pat.trim().is_empty() || pat.chars().any(char::is_control) {
            return Err(ChecksError::InvalidInput(
                "ui_paths entries must be non-empty without control characters".to_string(),
            ));
        }
    }
    Ok(())
}

pub fn require_confirmed_checks(
    settings: &FactorySettings,
    target: &str,
) -> Result<ConfirmedChecks, ChecksError> {
    let cfg = settings
        .checks
        .get(target)
        .ok_or(ChecksError::NotConfirmed)?;
    ConfirmedChecks::from_config(target, cfg)
}

#[allow(clippy::too_many_arguments)]
pub fn set_checks(
    store: &FactoryStore,
    target: &str,
    test: Vec<Vec<String>>,
    lint: Vec<Vec<String>>,
    timeout_secs: Option<u64>,
    ui_paths: Vec<String>,
    confirm: &str,
    confirmed_by: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<ChecksConfig, ChecksError> {
    ensure_user_caller(env)?;
    validate_target_name(target)?;
    if confirm != target {
        return Err(ChecksError::InvalidInput(format!(
            "confirmation mismatch: pass --confirm {target}"
        )));
    }
    if confirmed_by.trim().is_empty() {
        return Err(ChecksError::InvalidInput(
            "confirmed_by must not be empty".to_string(),
        ));
    }

    let cfg = ChecksConfig {
        test,
        lint,
        timeout_secs: timeout_secs.unwrap_or(DEFAULT_CHECK_TIMEOUT_SECS),
        ui_paths,
        confirmed_by: Some(confirmed_by.to_string()),
        confirmed_at: Some(current_time()),
    };
    validate_checks_config(&cfg)?;

    store.ensure()?;
    let lock_path = store.lock_path();
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(FactoryError::from)?;
    set_private_file_mode(&lock_path)?;
    let _guard = exclusive_lock(&lock_file).map_err(FactoryError::from)?;

    let mut settings = store.settings()?;
    settings.checks.insert(target.to_string(), cfg.clone());
    store.save_settings(&settings)?;
    Ok(cfg)
}

pub fn show_checks(
    store: &FactoryStore,
    target: Option<&str>,
) -> Result<BTreeMap<String, ChecksConfig>, ChecksError> {
    let settings = if store.exists() {
        store.settings()?
    } else {
        FactorySettings::default()
    };
    if let Some(t) = target {
        validate_target_name(t)?;
        let mut out = BTreeMap::new();
        if let Some(cfg) = settings.checks.get(t) {
            out.insert(t.to_string(), cfg.clone());
        }
        Ok(out)
    } else {
        Ok(settings.checks)
    }
}

pub fn discover_checks(
    target_name: &str,
    target_path: &Path,
    confirmed: Option<ChecksConfig>,
) -> Result<DiscoverReport, ChecksError> {
    validate_target_name(target_name)?;
    if !target_path.is_dir() {
        return Err(ChecksError::InvalidInput(format!(
            "target path does not exist: {}",
            target_path.display()
        )));
    }

    let mut proposals = Vec::new();
    let mut seen: HashSet<(u8, Vec<String>)> = HashSet::new();
    let mut push_unique = |p: CheckProposal| {
        let k = match p.kind {
            CheckKind::Test => 0,
            CheckKind::Lint => 1,
        };
        if seen.insert((k, p.argv.clone())) {
            proposals.push(p);
        }
    };

    for p in discover_from_check_all_sh(target_path) {
        push_unique(p);
    }
    for p in discover_from_cargo_and_ci(target_path) {
        push_unique(p);
    }
    for p in discover_from_package_json(target_path) {
        push_unique(p);
    }
    for p in discover_from_docs(target_path) {
        push_unique(p);
    }

    let confirm_command = build_confirm_command(target_name, &proposals);
    Ok(DiscoverReport {
        target: target_name.to_string(),
        target_path: target_path.display().to_string(),
        proposals,
        confirmed,
        confirm_command,
    })
}

fn read_regular_text(path: &Path) -> Option<String> {
    let meta = fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn strip_quotes(tok: &str) -> &str {
    tok.trim_matches(|c| c == '"' || c == '\'')
}

fn normalize_shell_token(tok: &str) -> Option<String> {
    let s = strip_quotes(tok);
    if s.is_empty() {
        return None;
    }
    if let Some(rest) = s.strip_prefix("$ROOT/") {
        return Some(rest.to_string());
    }
    if s == "$ROOT" {
        return Some(".".to_string());
    }
    Some(s.to_string())
}

fn classify_argv(argv: &[String]) -> Option<CheckKind> {
    let head = argv.first()?.as_str();
    match head {
        "cargo" => match argv.get(1).map(String::as_str) {
            Some("test") => Some(CheckKind::Test),
            Some("fmt" | "clippy") => Some(CheckKind::Lint),
            _ => None,
        },
        "npm" | "pnpm" | "yarn" | "bun" => {
            if argv.iter().any(|a| a == "test") {
                Some(CheckKind::Test)
            } else if argv.iter().any(|a| a == "lint") {
                Some(CheckKind::Lint)
            } else {
                None
            }
        }
        "pytest" => Some(CheckKind::Test),
        "python3" | "python" => {
            if argv
                .iter()
                .any(|a| a.contains("run_tests.py") || a == "pytest")
            {
                Some(CheckKind::Test)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn parse_check_all_tokens(raw_tokens: &[&str]) -> Option<(CheckKind, Vec<String>)> {
    let mut argv = Vec::new();
    let mut i = 0;
    while i < raw_tokens.len() {
        let tok = raw_tokens[i];
        if tok == "--prefix"
            && i + 1 < raw_tokens.len()
            && strip_quotes(raw_tokens[i + 1]) == "$ROOT"
        {
            i += 2;
            continue;
        }
        if let Some(norm) = normalize_shell_token(tok) {
            argv.push(norm);
        }
        i += 1;
    }
    let kind = classify_argv(&argv)?;
    Some((kind, argv))
}

fn discover_from_check_all_sh(target_path: &Path) -> Vec<CheckProposal> {
    let rel = "scripts/check-all.sh";
    let Some(content) = read_regular_text(&target_path.join(rel)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        let cmd_tokens: &[&str] = if tokens.first() == Some(&"run_cmd") {
            let mut start = 1;
            if let Some(t) = tokens.get(start)
                && t.starts_with('"')
            {
                while start < tokens.len() && !tokens[start].ends_with('"') {
                    start += 1;
                }
                start += 1;
            }
            if start < tokens.len() {
                &tokens[start..]
            } else {
                continue;
            }
        } else {
            &tokens
        };

        if let Some((kind, argv)) = parse_check_all_tokens(cmd_tokens) {
            out.push(CheckProposal {
                kind,
                argv,
                source_file: rel.to_string(),
                source_line: idx + 1,
                note: None,
            });
        }
    }
    out
}

fn find_line_containing(content: &str, needle: &str) -> usize {
    content
        .lines()
        .position(|l| l.contains(needle))
        .map(|i| i + 1)
        .unwrap_or(1)
}

fn discover_from_cargo_and_ci(target_path: &Path) -> Vec<CheckProposal> {
    let mut out = Vec::new();
    for rel in ["Cargo.toml", "tandem/Cargo.toml"] {
        let Some(content) = read_regular_text(&target_path.join(rel)) else {
            continue;
        };
        let line = find_line_containing(&content, "[package]");
        let argv = if rel == "Cargo.toml" {
            vec!["cargo".to_string(), "test".to_string()]
        } else {
            vec![
                "cargo".to_string(),
                "test".to_string(),
                "--manifest-path".to_string(),
                rel.to_string(),
            ]
        };
        out.push(CheckProposal {
            kind: CheckKind::Test,
            argv,
            source_file: rel.to_string(),
            source_line: line,
            note: None,
        });
    }

    let wf_dir = target_path.join(".github/workflows");
    if wf_dir.is_dir()
        && let Ok(entries) = fs::read_dir(&wf_dir)
    {
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        files.sort();
        for file in files {
            let ext = file.extension().and_then(|s| s.to_str());
            if !matches!(ext, Some("yml" | "yaml")) {
                continue;
            }
            let Some(content) = read_regular_text(&file) else {
                continue;
            };
            if let Some(pos) = content.lines().position(|l| l.contains("clippy")) {
                let rel = file
                    .strip_prefix(target_path)
                    .unwrap_or(&file)
                    .to_string_lossy()
                    .to_string();
                out.push(CheckProposal {
                    kind: CheckKind::Lint,
                    argv: vec![
                        "cargo".to_string(),
                        "clippy".to_string(),
                        "--all-targets".to_string(),
                        "--".to_string(),
                        "-D".to_string(),
                        "warnings".to_string(),
                    ],
                    source_file: rel,
                    source_line: pos + 1,
                    note: None,
                });
                break;
            }
        }
    }

    out
}

fn detect_package_manager(target_path: &Path) -> &'static str {
    if target_path.join("bun.lockb").exists() || target_path.join("bun.lock").exists() {
        "bun"
    } else if target_path.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if target_path.join("yarn.lock").exists() {
        "yarn"
    } else {
        "npm"
    }
}

fn discover_from_package_json(target_path: &Path) -> Vec<CheckProposal> {
    let rel = "package.json";
    let Some(content) = read_regular_text(&target_path.join(rel)) else {
        return Vec::new();
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    let Some(scripts) = val.get("scripts").and_then(|s| s.as_object()) else {
        return Vec::new();
    };

    let pm = detect_package_manager(target_path);
    let mut out = Vec::new();

    if let Some(test_script) = scripts.get("test").and_then(|v| v.as_str())
        && !test_script.trim().is_empty()
        && !test_script.contains("no test specified")
    {
        out.push(CheckProposal {
            kind: CheckKind::Test,
            argv: vec![pm.to_string(), "test".to_string()],
            source_file: rel.to_string(),
            source_line: find_line_containing(&content, "\"test\""),
            note: Some(format!("scripts.test ({pm})")),
        });
    }

    if let Some(lint_script) = scripts.get("lint").and_then(|v| v.as_str())
        && !lint_script.trim().is_empty()
    {
        out.push(CheckProposal {
            kind: CheckKind::Lint,
            argv: vec![pm.to_string(), "run".to_string(), "lint".to_string()],
            source_file: rel.to_string(),
            source_line: find_line_containing(&content, "\"lint\""),
            note: Some(format!("scripts.lint ({pm})")),
        });
    }

    out
}

fn extract_backticked_commands(line: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('`') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('`') else {
            break;
        };
        let snippet = after[..end].trim();
        let tokens: Vec<String> = snippet.split_whitespace().map(String::from).collect();
        if classify_argv(&tokens).is_some() && !tokens.iter().any(|t| t.starts_with('<')) {
            out.push(tokens);
        }
        rest = &after[end + 1..];
    }
    out
}

fn discover_from_docs(target_path: &Path) -> Vec<CheckProposal> {
    let mut out = Vec::new();
    for rel in ["WORKFLOW.md", "AGENTS.md"] {
        let Some(content) = read_regular_text(&target_path.join(rel)) else {
            continue;
        };
        for (idx, line) in content.lines().enumerate() {
            for argv in extract_backticked_commands(line) {
                if let Some(kind) = classify_argv(&argv) {
                    out.push(CheckProposal {
                        kind,
                        argv,
                        source_file: rel.to_string(),
                        source_line: idx + 1,
                        note: None,
                    });
                }
            }
        }
    }
    out
}

fn build_confirm_command(target: &str, proposals: &[CheckProposal]) -> String {
    let test_cmds: Vec<&Vec<String>> = proposals
        .iter()
        .filter(|p| p.kind == CheckKind::Test)
        .take(MAX_CHECK_COMMANDS_PER_KIND)
        .map(|p| &p.argv)
        .collect();
    let lint_cmds: Vec<&Vec<String>> = proposals
        .iter()
        .filter(|p| p.kind == CheckKind::Lint)
        .take(MAX_CHECK_COMMANDS_PER_KIND)
        .map(|p| &p.argv)
        .collect();
    let test_json = serde_json::to_string(&test_cmds).unwrap_or_else(|_| "[]".to_string());
    let lint_json = serde_json::to_string(&lint_cmds).unwrap_or_else(|_| "[]".to_string());
    format!(
        "tandem factory checks set --target {target} --test '{test_json}' --lint '{lint_json}' --confirm {target}"
    )
}
