use std::fmt;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::FactoryError;
use super::broker::context::ContextItem;
use super::broker::job::{JobInput, jobs_dir};
use super::events::{reject_if_symlink, set_private_file_mode};
use super::model::{FactorySettings, Run};
use super::permissions::ToolGrant;
use super::store::{FactoryStore, write_atomic};

pub const DEFAULT_SECRETS_PATTERNS: &[&str] = &[
    ".env*",
    "*.pem",
    "*.key",
    "id_rsa*",
    "secrets/**",
    ".git/**",
];

pub const PROVIDER_LIMITATION_SENTENCE: &str =
    "The provider CLI can read any file in the readable scope. tandem does not see which.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataClass {
    Source,
    Tests,
    Docs,
    Config,
    Generated,
    Secrets,
}

impl DataClass {
    pub const ALL: [Self; 6] = [
        Self::Source,
        Self::Tests,
        Self::Docs,
        Self::Config,
        Self::Generated,
        Self::Secrets,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Tests => "tests",
            Self::Docs => "docs",
            Self::Config => "config",
            Self::Generated => "generated",
            Self::Secrets => "secrets",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "source" => Some(Self::Source),
            "tests" => Some(Self::Tests),
            "docs" => Some(Self::Docs),
            "config" => Some(Self::Config),
            "generated" => Some(Self::Generated),
            "secrets" => Some(Self::Secrets),
            _ => None,
        }
    }
}

impl fmt::Display for DataClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("data policy denied {path} ({data_class}): {rule}")]
pub struct PolicyDenial {
    pub path: String,
    pub data_class: DataClass,
    pub rule: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExcludedPathEntry {
    pub path: String,
    pub rule: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobDataView {
    pub correlation_id: String,
    pub stage: String,
    pub attempt: u32,
    pub provider: String,
    pub model: String,
    pub sent_files: Vec<ContextItem>,
    pub total_bytes: usize,
    pub redactions: usize,
    pub readable_scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_grant: Option<ToolGrant>,
    pub tool_grants: Vec<String>,
    pub excluded_paths: Vec<String>,
    pub limitation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunDataView {
    pub run: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    pub limitation: String,
    pub excluded_paths: Vec<String>,
    pub exclusions: Vec<ExcludedPathEntry>,
    pub jobs: Vec<JobDataView>,
}

pub fn normalize_rel_path(raw: &str) -> String {
    let mut s = raw.replace('\\', "/");
    while let Some(stripped) = s.strip_prefix("./") {
        s = stripped.to_string();
    }
    while let Some(stripped) = s.strip_prefix('/') {
        s = stripped.to_string();
    }
    s
}

fn match_segment_glob(pat: &str, text: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let mut dp = vec![vec![false; t.len() + 1]; p.len() + 1];
    dp[0][0] = true;
    for i in 1..=p.len() {
        if p[i - 1] == '*' {
            dp[i][0] = dp[i - 1][0];
        }
    }
    for i in 1..=p.len() {
        for j in 1..=t.len() {
            if p[i - 1] == '*' {
                dp[i][j] = dp[i - 1][j] || dp[i][j - 1];
            } else if p[i - 1] == '?' || p[i - 1] == t[j - 1] {
                dp[i][j] = dp[i - 1][j - 1];
            }
        }
    }
    dp[p.len()][t.len()]
}

fn match_path_segments(pat_segs: &[&str], path_segs: &[&str]) -> bool {
    let m = pat_segs.len();
    let n = path_segs.len();
    let mut dp = vec![vec![false; n + 1]; m + 1];
    dp[0][0] = true;
    for i in 1..=m {
        if pat_segs[i - 1] == "**" {
            dp[i][0] = dp[i - 1][0];
        }
    }
    for i in 1..=m {
        for j in 1..=n {
            if pat_segs[i - 1] == "**" {
                dp[i][j] = dp[i - 1][j] || dp[i][j - 1];
            } else if match_segment_glob(pat_segs[i - 1], path_segs[j - 1]) {
                dp[i][j] = dp[i - 1][j - 1];
            }
        }
    }
    dp[m][n]
}

pub fn glob_match(pattern: &str, path: &str) -> bool {
    let norm_pat = normalize_rel_path(pattern.trim());
    let norm_path = normalize_rel_path(path.trim());
    if norm_pat.is_empty() || norm_path.is_empty() {
        return false;
    }

    if !norm_pat.contains('/') {
        let file_name = norm_path.rsplit('/').next().unwrap_or(&norm_path);
        return match_segment_glob(&norm_pat, file_name)
            || match_segment_glob(&norm_pat, &norm_path);
    }

    let pat_segs: Vec<&str> = norm_pat.split('/').collect();
    let path_segs: Vec<&str> = norm_path.split('/').collect();
    match_path_segments(&pat_segs, &path_segs)
}

pub fn classify_path(raw_path: &str) -> DataClass {
    let norm = normalize_rel_path(raw_path);
    if DEFAULT_SECRETS_PATTERNS
        .iter()
        .any(|pat| glob_match(pat, &norm))
    {
        return DataClass::Secrets;
    }

    let lower = norm.to_ascii_lowercase();
    let file_name = lower.rsplit('/').next().unwrap_or(&lower);
    let segs: Vec<&str> = lower.split('/').collect();

    if segs
        .iter()
        .any(|s| matches!(*s, "tests" | "test" | "__tests__" | "spec"))
        || file_name.starts_with("test_")
        || file_name.contains("_test.")
        || file_name.contains(".test.")
        || file_name.contains("_spec.")
        || file_name.contains(".spec.")
    {
        return DataClass::Tests;
    }

    if segs.contains(&"docs")
        || file_name.ends_with(".md")
        || file_name.ends_with(".mdx")
        || file_name.ends_with(".txt")
        || file_name.ends_with(".rst")
        || file_name.ends_with(".adoc")
        || file_name.starts_with("readme")
        || file_name.starts_with("changelog")
        || file_name.starts_with("license")
    {
        return DataClass::Docs;
    }

    if segs.iter().any(|s| {
        matches!(
            *s,
            "target" | "dist" | "build" | "out" | ".next" | "node_modules" | "generated"
        )
    }) || file_name.ends_with(".lock")
        || file_name == "package-lock.json"
        || file_name.ends_with(".min.js")
        || file_name.ends_with(".min.css")
        || file_name.ends_with(".map")
    {
        return DataClass::Generated;
    }

    if file_name.ends_with(".json")
        || file_name.ends_with(".yaml")
        || file_name.ends_with(".yml")
        || file_name.ends_with(".toml")
        || file_name.ends_with(".ini")
        || file_name.ends_with(".cfg")
        || file_name.ends_with(".conf")
        || file_name.starts_with(".eslintrc")
        || file_name.starts_with(".prettierrc")
        || file_name.starts_with("dockerfile")
        || file_name == "makefile"
        || file_name.contains(".config.")
    {
        return DataClass::Config;
    }

    DataClass::Source
}

pub fn check_path(
    path: &str,
    provider: &str,
    settings: &FactorySettings,
) -> Result<DataClass, PolicyDenial> {
    let norm = normalize_rel_path(path);
    let data_class = classify_path(&norm);

    for pat in &settings.denied_path_patterns {
        if glob_match(pat, &norm) {
            return Err(PolicyDenial {
                path: path.to_string(),
                data_class,
                rule: format!("denied_path_patterns: {pat}"),
            });
        }
    }

    if data_class == DataClass::Secrets {
        return Err(PolicyDenial {
            path: path.to_string(),
            data_class,
            rule: "secrets class is never allowed".to_string(),
        });
    }

    if let Some(allowed) = settings.allowed_data_classes.get(provider)
        && !allowed.contains(&data_class)
    {
        return Err(PolicyDenial {
            path: path.to_string(),
            data_class,
            rule: format!(
                "data class '{}' is not allowed for provider '{}'",
                data_class.as_str(),
                provider
            ),
        });
    }

    Ok(data_class)
}

pub fn is_worktree_excluded(rel_path: &str, settings: &FactorySettings) -> Option<String> {
    let norm = normalize_rel_path(rel_path);
    if classify_path(&norm) == DataClass::Secrets {
        return Some("secrets".to_string());
    }
    for pat in &settings.denied_path_patterns {
        if glob_match(pat, &norm) {
            return Some(format!("denied_path_patterns: {pat}"));
        }
    }
    None
}

pub fn check_slices_against_exclusions(
    slice_paths: &[String],
    settings: &FactorySettings,
) -> Option<(String, String)> {
    for path in slice_paths {
        if let Some(rule) = is_worktree_excluded(path, settings) {
            return Some((path.clone(), rule));
        }
    }
    None
}

pub fn exclusions_path(store: &FactoryStore, run_id: &str) -> PathBuf {
    store.run_dir(run_id).join("exclusions.json")
}

pub fn save_exclusions(
    store: &FactoryStore,
    run_id: &str,
    entries: &[ExcludedPathEntry],
) -> Result<(), FactoryError> {
    let path = exclusions_path(store, run_id);
    reject_if_symlink(&path)?;
    let bytes = serde_json::to_vec_pretty(entries)?;
    write_atomic(&path, &bytes)?;
    set_private_file_mode(&path)?;
    Ok(())
}

pub fn load_exclusions(store: &FactoryStore, run_id: &str) -> Vec<ExcludedPathEntry> {
    let path = exclusions_path(store, run_id);
    if reject_if_symlink(&path).is_err() {
        return Vec::new();
    }
    fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn format_bytes(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.0} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

pub fn format_readable_scope(worktree_path: Option<&str>, cwd: &str) -> String {
    let scope_base = worktree_path.filter(|s| !s.is_empty()).unwrap_or(cwd);
    format!("{scope_base} (tracked files)")
}

pub fn build_run_data(store: &FactoryStore, run: &Run) -> Result<RunDataView, FactoryError> {
    let exclusions = load_exclusions(store, &run.id);
    let excluded_paths: Vec<String> = exclusions.iter().map(|e| e.path.clone()).collect();

    let base = jobs_dir(store, &run.id);
    let mut job_entries: Vec<(u32, String, JobInput, usize)> = Vec::new();
    if base.exists() && reject_if_symlink(&base).is_ok() {
        for entry in fs::read_dir(&base)? {
            let entry = entry?;
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let input_path = dir.join("input.json");
            if reject_if_symlink(&input_path).is_err() {
                continue;
            }
            let Ok(bytes) = fs::read(&input_path) else {
                continue;
            };
            let Ok(input) = serde_json::from_slice::<JobInput>(&bytes) else {
                continue;
            };
            let prompt_path = dir.join("prompt.md");
            let redactions = if reject_if_symlink(&prompt_path).is_ok() {
                fs::read_to_string(&prompt_path)
                    .map(|text| text.matches("«redacted:").count())
                    .unwrap_or(0)
            } else {
                0
            };
            job_entries.push((input.attempt, input.stage.clone(), input, redactions));
        }
    }

    let stage_order = |stage: &str| {
        run.stages
            .iter()
            .position(|s| s.name == stage)
            .unwrap_or(usize::MAX)
    };
    job_entries.sort_by(|a, b| {
        stage_order(&a.1)
            .cmp(&stage_order(&b.1))
            .then_with(|| a.0.cmp(&b.0))
    });

    let jobs = job_entries
        .into_iter()
        .map(|(_, _, input, redactions)| {
            let job_excluded = if input.context_manifest.excluded_paths.is_empty() {
                excluded_paths.clone()
            } else {
                input.context_manifest.excluded_paths.clone()
            };
            let readable_scope = input
                .context_manifest
                .readable_scope
                .clone()
                .unwrap_or_else(|| format_readable_scope(run.worktree_path.as_deref(), &input.cwd));
            JobDataView {
                correlation_id: input.correlation_id,
                stage: input.stage,
                attempt: input.attempt,
                provider: input.provider,
                model: input.model,
                sent_files: input.context_manifest.items,
                total_bytes: input.context_manifest.total_bytes,
                redactions,
                readable_scope,
                tool_grant: input.context_manifest.tool_grant,
                tool_grants: input.tool_grants,
                excluded_paths: job_excluded,
                limitation: PROVIDER_LIMITATION_SENTENCE.to_string(),
            }
        })
        .collect();

    Ok(RunDataView {
        run: run.id.clone(),
        worktree_path: run.worktree_path.clone(),
        limitation: PROVIDER_LIMITATION_SENTENCE.to_string(),
        excluded_paths,
        exclusions,
        jobs,
    })
}

pub fn format_run_data_text(view: &RunDataView) -> String {
    if view.jobs.is_empty() {
        let mut lines = vec![format!("Data · {}", view.run)];
        if view.excluded_paths.is_empty() {
            lines.push("Excluded           none".to_string());
        } else {
            lines.push(format!(
                "Excluded           {} paths: {}",
                view.excluded_paths.len(),
                view.excluded_paths.join(", ")
            ));
        }
        lines.push(view.limitation.clone());
        return lines.join("\n");
    }

    let mut sections = Vec::new();
    for job in &view.jobs {
        let mut lines = Vec::new();
        lines.push(format!(
            "Data · {} attempt {} · {} / {}",
            job.stage, job.attempt, job.provider, job.model
        ));
        lines.push(format!(
            "Sent by tandem     {} files · {} · {} redactions",
            job.sent_files.len(),
            format_bytes(job.total_bytes),
            job.redactions
        ));
        for item in &job.sent_files {
            lines.push(format!(
                "  - {}: {} ({}, sha256:{})",
                item.kind,
                item.path,
                format_bytes(item.bytes),
                &item.sha256[..item.sha256.len().min(12)]
            ));
        }
        let tools = if job.tool_grants.is_empty() {
            "none".to_string()
        } else {
            job.tool_grants.join(", ")
        };
        lines.push(format!(
            "Readable by job    {} with {}",
            job.readable_scope, tools
        ));
        if job.excluded_paths.is_empty() {
            lines.push("Excluded           none".to_string());
        } else {
            lines.push(format!(
                "Excluded           {} paths: {}",
                job.excluded_paths.len(),
                job.excluded_paths.join(", ")
            ));
        }
        lines.push(job.limitation.clone());
        sections.push(lines.join("\n"));
    }
    sections.join("\n\n")
}
