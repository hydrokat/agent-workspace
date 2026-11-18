use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::factory::events::reject_if_symlink;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobResultStatus {
    Succeeded,
    Failed,
    NeedsInput,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobResult {
    pub status: JobResultStatus,
    pub summary: String,
    #[serde(default)]
    pub files_changed: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hypothesis: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<JobUsage>,
    #[serde(default, alias = "no_behavior_change")]
    pub no_behaviour_change: bool,
}

fn no_structured_result(detail: &str) -> JobResult {
    JobResult {
        status: JobResultStatus::Failed,
        summary: String::new(),
        files_changed: Vec::new(),
        artifacts: Vec::new(),
        rationale: Some(if detail.is_empty() {
            "no structured result".to_string()
        } else {
            format!("no structured result: {detail}")
        }),
        hypothesis: None,
        usage: None,
        no_behaviour_change: false,
    }
}

pub fn validate_result(res: &JobResult, is_retry: bool) -> Result<(), &'static str> {
    if res.status == JobResultStatus::Failed
        && res.rationale.as_deref().is_none_or(|r| r.trim().is_empty())
    {
        return Err("failed result requires rationale");
    }
    if is_retry
        && res
            .hypothesis
            .as_deref()
            .is_none_or(|h| h.trim().is_empty())
    {
        return Err("retry result requires hypothesis");
    }
    Ok(())
}

pub fn parse_and_validate_result_strict(path: &Path, is_retry: bool) -> Result<JobResult, String> {
    reject_if_symlink(path).map_err(|e| e.to_string())?;
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let parsed: JobResult = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    validate_result(&parsed, is_retry).map_err(str::to_string)?;
    Ok(parsed)
}

pub fn parse_and_validate_result(path: &Path, is_retry: bool) -> JobResult {
    match parse_and_validate_result_strict(path, is_retry) {
        Ok(res) => res,
        Err(detail) => no_structured_result(&detail),
    }
}
