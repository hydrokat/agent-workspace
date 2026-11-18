use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::FactoryError;
use super::broker::checks::CheckOutcome;
use super::broker::git::{AllowedGitSubcommand, run_git};
use super::checks::CheckKind;
use super::events::{
    Event, append_locked, read_all, redact_event, reject_if_symlink, set_private_dir_mode,
    set_private_file_mode,
};
use super::ids::{is_valid_idea_id, is_valid_run_id};
use super::machine::{self, EventData};
use super::model::{Actor, Run, StageState};
use super::redact::{RedactContext, Redacted, redact};
use super::stages::test::{TestStageReport, format_failures_summary};
use super::store::{FactoryStore, current_time, event_run_id, write_atomic};
use super::workflow::schema::{EvidenceKind, Workflow};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceItem {
    pub id: String,
    pub kind: EvidenceKind,
    pub stage: String,
    pub attempt: u32,
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub summary: String,
    pub created: f64,
}

pub enum EvidenceContent<'a> {
    Text(Redacted),
    Screenshot(&'a [u8]),
}

pub struct AddEvidenceInput<'a> {
    pub idea_id: &'a str,
    pub run_id: &'a str,
    pub kind: EvidenceKind,
    pub stage: &'a str,
    pub attempt: u32,
    pub extension: &'a str,
    pub content: EvidenceContent<'a>,
    pub summary: Redacted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckReportEntry {
    command: Vec<String>,
    exit_code: Option<i32>,
    duration_ms: u64,
    timed_out: bool,
    log_cap_exceeded: bool,
    log_path: String,
    passed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CheckReportPayload {
    attempt: u32,
    passed: bool,
    checks: Vec<CheckReportEntry>,
    failures_summary: String,
}

fn validate_extension(ext: &str) -> Result<&str, FactoryError> {
    let trimmed = ext.trim_start_matches('.');
    if trimmed.is_empty() || !trimmed.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(FactoryError::InvalidInput(format!(
            "invalid evidence extension: {ext}"
        )));
    }
    Ok(trimmed)
}

fn validate_item_id(id: &str) -> Result<(), FactoryError> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(FactoryError::InvalidInput(format!(
            "invalid evidence item id: {id}"
        )));
    }
    Ok(())
}

fn parse_ev_seq(name: &str) -> Option<u32> {
    let stem = name.split('.').next()?;
    let num = stem.strip_prefix("ev-")?;
    num.parse::<u32>().ok()
}

pub fn add(
    store: &FactoryStore,
    input: AddEvidenceInput<'_>,
    actor: &Actor,
) -> Result<EvidenceItem, FactoryError> {
    if !is_valid_idea_id(input.idea_id) {
        return Err(FactoryError::InvalidInput(format!(
            "invalid idea id: {}",
            input.idea_id
        )));
    }
    if !is_valid_run_id(input.run_id) {
        return Err(FactoryError::InvalidInput(format!(
            "invalid run id: {}",
            input.run_id
        )));
    }
    if input.stage.trim().is_empty() {
        return Err(FactoryError::InvalidInput(
            "evidence stage must not be empty".to_string(),
        ));
    }
    let ext = validate_extension(input.extension)?;

    store.ensure()?;
    let lock_path = store.lock_path();
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    set_private_file_mode(&lock_path)?;
    let _guard = crate::coordination::filelock::exclusive_lock(&lock_file)?;

    let runs_dir = store.runs_dir();
    let run_dir = store.run_dir(input.run_id);
    let evidence_dir = run_dir.join("evidence");
    reject_if_symlink(&runs_dir)?;
    reject_if_symlink(&run_dir)?;
    reject_if_symlink(&evidence_dir)?;

    if !evidence_dir.exists() {
        fs::create_dir_all(&evidence_dir)?;
        set_private_dir_mode(&evidence_dir)?;
    }

    let mut max_seq = 0u32;
    for entry in fs::read_dir(&evidence_dir)? {
        let entry = entry?;
        if let Some(seq) = parse_ev_seq(&entry.file_name().to_string_lossy()) {
            max_seq = max_seq.max(seq);
        }
    }
    let existing = read_all(&store.events_path())?;
    for ev in &existing.events {
        if ev.run.as_deref() == Some(input.run_id)
            && ev.r#type == "evidence.added"
            && let Ok(EventData::EvidenceAdded { item, .. }) = ev.to_event_data()
            && let Some(seq) = parse_ev_seq(&item.id)
        {
            max_seq = max_seq.max(seq);
        }
    }

    let id = format!("ev-{:04}", max_seq + 1);
    let filename = format!("{id}.{ext}");
    let dest = evidence_dir.join(&filename);
    reject_if_symlink(&dest)?;

    let content_bytes: &[u8] = match &input.content {
        EvidenceContent::Text(redacted) => redacted.as_str().as_bytes(),
        EvidenceContent::Screenshot(bytes) => bytes,
    };

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&dest)?;
    file.write_all(content_bytes)?;
    file.flush()?;
    set_private_file_mode(&dest)?;

    let mut hasher = Sha256::new();
    hasher.update(content_bytes);
    let digest = hasher.finalize();
    let sha256: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let created = current_time();

    let item = EvidenceItem {
        id,
        kind: input.kind,
        stage: input.stage.to_string(),
        attempt: input.attempt,
        path: format!("runs/{}/evidence/{filename}", input.run_id),
        sha256,
        bytes: content_bytes.len() as u64,
        summary: input.summary.into_string(),
        created,
    };

    let ev_data = EventData::EvidenceAdded {
        run: input.run_id.to_string(),
        item: item.clone(),
    };
    let ev = Event::from_data(
        created,
        actor.clone(),
        input.idea_id.to_string(),
        event_run_id(&ev_data),
        &ev_data,
    )?;
    let redacted_ev = redact_event(ev, &RedactContext::from_env());
    let appended = append_locked(&store.events_path(), &[redacted_ev])?;

    let run_json_path = store.run_json_path(input.run_id);
    if run_json_path.exists() {
        reject_if_symlink(&run_json_path)?;
        let raw = fs::read_to_string(&run_json_path)?;
        let current_run: Run = serde_json::from_str(&raw)?;
        let ts = appended.first().map(|e| e.ts).unwrap_or(created);
        if let Some(updated_run) =
            machine::apply_run(Some(current_run), &ev_data, input.idea_id, ts)
        {
            let bytes = serde_json::to_vec_pretty(&updated_run)?;
            write_atomic(&run_json_path, &bytes)?;
        }
    }

    Ok(item)
}

pub fn list(store: &FactoryStore, run_id: &str) -> Result<Vec<EvidenceItem>, FactoryError> {
    if !is_valid_run_id(run_id) {
        return Err(FactoryError::InvalidInput(format!(
            "invalid run id: {run_id}"
        )));
    }
    let res = read_all(&store.events_path())?;
    let mut items = Vec::new();
    for ev in res.events {
        if ev.run.as_deref() == Some(run_id)
            && ev.r#type == "evidence.added"
            && let Ok(EventData::EvidenceAdded { item, .. }) = ev.to_event_data()
        {
            items.push(item);
        }
    }
    Ok(items)
}

pub fn read_item(
    store: &FactoryStore,
    run_id: &str,
    item_id: &str,
) -> Result<String, FactoryError> {
    validate_item_id(item_id)?;
    let items = list(store, run_id)?;
    let item = items
        .into_iter()
        .find(|i| i.id == item_id)
        .ok_or_else(|| FactoryError::InvalidInput(format!("evidence item {item_id} not found")))?;
    let path = store.root.join(&item.path);
    reject_if_symlink(&path)?;
    let bytes = fs::read(&path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub fn missing(run: &Run, workflow: &Workflow) -> Vec<EvidenceKind> {
    let mut out = Vec::new();
    for stage_def in &workflow.stages {
        let is_skipped = run
            .stages
            .iter()
            .any(|s| s.name == stage_def.name && s.state == StageState::Skipped);
        if is_skipped {
            continue;
        }
        for req in &stage_def.evidence {
            let present = run
                .evidence
                .iter()
                .any(|item| item.stage == stage_def.name && item.kind == *req);
            if !present && !out.contains(req) {
                out.push(*req);
            }
        }
    }
    out
}

pub fn load_review_rounds(
    store: &FactoryStore,
    run_id: &str,
) -> super::stages::review::ReviewRoundsReport {
    let run_dir = store.run_dir(run_id);
    let candidates = [
        run_dir
            .join("artifacts")
            .join("review")
            .join("review-rounds.json"),
        run_dir.join("artifacts").join("review-rounds.json"),
        run_dir.join("review-rounds.json"),
    ];
    for path in &candidates {
        if reject_if_symlink(path).is_ok()
            && let Ok(raw) = fs::read_to_string(path)
            && let Ok(parsed) =
                serde_json::from_str::<super::stages::review::ReviewRoundsReport>(&raw)
        {
            return parsed;
        }
    }
    super::stages::review::ReviewRoundsReport::default()
}

pub fn bundle_summary(items: &[EvidenceItem]) -> String {
    if items.is_empty() {
        return "no evidence recorded".to_string();
    }
    let mut latest: BTreeMap<(String, &'static str), &str> = BTreeMap::new();
    for item in items {
        latest.insert(
            (item.stage.clone(), item.kind.as_str()),
            item.summary.as_str(),
        );
    }
    latest
        .into_values()
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn build_check_report(attempt: u32, outcomes: &[&CheckOutcome], label: &str) -> (String, String) {
    let owned: Vec<CheckOutcome> = outcomes.iter().map(|o| (*o).clone()).collect();
    let passed_count = owned.iter().filter(|o| o.passed()).count();
    let total = owned.len();
    let all_passed = owned.iter().all(CheckOutcome::passed);
    let failures_summary = format_failures_summary(&owned);

    let checks: Vec<CheckReportEntry> = owned
        .iter()
        .map(|o| CheckReportEntry {
            command: o.argv.clone(),
            exit_code: o.exit_code,
            duration_ms: o.duration_ms,
            timed_out: o.timed_out,
            log_cap_exceeded: o.log_cap_exceeded,
            log_path: o.log_path.display().to_string(),
            passed: o.passed(),
        })
        .collect();

    let payload = CheckReportPayload {
        attempt,
        passed: all_passed,
        checks,
        failures_summary,
    };
    let text = serde_json::to_string_pretty(&payload).unwrap_or_default();

    let summary = if total == 0 {
        format!("{label}: none configured")
    } else if all_passed {
        if label == "lint" {
            "lint: clean".to_string()
        } else {
            format!("{label}: {passed_count}/{total} passed")
        }
    } else {
        let failed = total - passed_count;
        format!("{label}: {failed}/{total} failed")
    };

    (text, summary)
}

pub fn record_test_stage_evidence(
    store: &FactoryStore,
    idea_id: &str,
    run_id: &str,
    attempt: u32,
    report: &TestStageReport,
    actor: &Actor,
) -> Result<Vec<EvidenceItem>, FactoryError> {
    let test_outcomes: Vec<&CheckOutcome> = report
        .outcomes
        .iter()
        .filter(|o| o.kind == CheckKind::Test)
        .collect();
    let lint_outcomes: Vec<&CheckOutcome> = report
        .outcomes
        .iter()
        .filter(|o| o.kind == CheckKind::Lint)
        .collect();

    let ctx = RedactContext::from_env();
    let (test_text, test_summary) = build_check_report(attempt, &test_outcomes, "tests");
    let (lint_text, lint_summary) = build_check_report(attempt, &lint_outcomes, "lint");

    let test_item = add(
        store,
        AddEvidenceInput {
            idea_id,
            run_id,
            kind: EvidenceKind::TestReport,
            stage: "test",
            attempt,
            extension: "json",
            content: EvidenceContent::Text(redact(&test_text, &ctx)),
            summary: redact(&test_summary, &ctx),
        },
        actor,
    )?;

    let lint_item = add(
        store,
        AddEvidenceInput {
            idea_id,
            run_id,
            kind: EvidenceKind::LintReport,
            stage: "test",
            attempt,
            extension: "json",
            content: EvidenceContent::Text(redact(&lint_text, &ctx)),
            summary: redact(&lint_summary, &ctx),
        },
        actor,
    )?;

    Ok(vec![test_item, lint_item])
}

pub fn record_review_findings(
    store: &FactoryStore,
    idea_id: &str,
    run_id: &str,
    attempt: u32,
    content: &[u8],
    summary: String,
    actor: &Actor,
) -> Result<EvidenceItem, FactoryError> {
    let ctx = RedactContext::from_env();
    let raw_text = String::from_utf8_lossy(content);
    add(
        store,
        AddEvidenceInput {
            idea_id,
            run_id,
            kind: EvidenceKind::ReviewFindings,
            stage: "review",
            attempt,
            extension: "json",
            content: EvidenceContent::Text(redact(&raw_text, &ctx)),
            summary: redact(&summary, &ctx),
        },
        actor,
    )
}

pub fn record_create_tests_exception(
    store: &FactoryStore,
    idea_id: &str,
    run_id: &str,
    attempt: u32,
    rationale: &str,
    actor: &Actor,
) -> Result<EvidenceItem, FactoryError> {
    let ctx = RedactContext::from_env();
    let payload = serde_json::json!({
        "exception": "no_behaviour_change",
        "rationale": rationale,
    });
    let text = serde_json::to_string_pretty(&payload)?;
    add(
        store,
        AddEvidenceInput {
            idea_id,
            run_id,
            kind: EvidenceKind::Exception,
            stage: "create-tests",
            attempt,
            extension: "json",
            content: EvidenceContent::Text(redact(&text, &ctx)),
            summary: redact(&format!("no_behaviour_change: {rationale}"), &ctx),
        },
        actor,
    )
}

pub fn record_committed_diff_stat(
    store: &FactoryStore,
    idea_id: &str,
    run_id: &str,
    stage: &str,
    attempt: u32,
    worktree_path: &Path,
    actor: &Actor,
) -> Result<EvidenceItem, FactoryError> {
    let ctx = RedactContext::from_env();
    let timeout = Duration::from_secs(10);
    let stat = match run_git(
        AllowedGitSubcommand::Diff,
        &["--stat", "HEAD~1", "HEAD"],
        worktree_path,
        timeout,
    ) {
        Ok(out) => out,
        Err(_) => run_git(
            AllowedGitSubcommand::Diff,
            &["--stat"],
            worktree_path,
            timeout,
        )
        .unwrap_or_default(),
    };
    let summary_line = stat
        .lines()
        .last()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("no diff stat")
        .to_string();

    add(
        store,
        AddEvidenceInput {
            idea_id,
            run_id,
            kind: EvidenceKind::DiffStat,
            stage,
            attempt,
            extension: "txt",
            content: EvidenceContent::Text(redact(&stat, &ctx)),
            summary: redact(&format!("{stage}: {summary_line}"), &ctx),
        },
        actor,
    )
}

pub fn record_demo_screenshots(
    store: &FactoryStore,
    idea_id: &str,
    run_id: &str,
    attempt: u32,
    artifact_dir: &Path,
    actor: &Actor,
) -> Result<Vec<EvidenceItem>, FactoryError> {
    let ctx = RedactContext::from_env();
    let manifest = super::stages::demo::load_and_validate_demo_manifest(artifact_dir)
        .map_err(|e| FactoryError::InvalidInput(e.to_string()))?;
    let mut items = Vec::with_capacity(manifest.screenshots.len());
    for entry in &manifest.screenshots {
        let png_path = artifact_dir.join(entry.path.trim());
        let bytes = fs::read(&png_path)?;
        let item = add(
            store,
            AddEvidenceInput {
                idea_id,
                run_id,
                kind: EvidenceKind::Screenshot,
                stage: "demo",
                attempt,
                extension: "png",
                content: EvidenceContent::Screenshot(&bytes),
                summary: redact(entry.caption.trim(), &ctx),
            },
            actor,
        )?;
        items.push(item);
    }
    Ok(items)
}

pub fn record_demo_skipped_no_tool(
    store: &FactoryStore,
    idea_id: &str,
    run_id: &str,
    reason: &str,
    actor: &Actor,
) -> Result<EvidenceItem, FactoryError> {
    let ctx = RedactContext::from_env();
    let payload = serde_json::json!({
        "skipped": true,
        "reason": reason,
        "screenshots_taken": 0,
        "note": "no screenshots were taken",
    });
    let text = serde_json::to_string_pretty(&payload)?;
    add(
        store,
        AddEvidenceInput {
            idea_id,
            run_id,
            kind: EvidenceKind::Exception,
            stage: "demo",
            attempt: 0,
            extension: "json",
            content: EvidenceContent::Text(redact(&text, &ctx)),
            summary: redact(
                &format!("demo skipped: no screenshots were taken ({reason})"),
                &ctx,
            ),
        },
        actor,
    )
}
