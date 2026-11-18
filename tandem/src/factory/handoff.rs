use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::coordination::filelock::exclusive_lock;

use super::FactoryError;
use super::approval::{ApprovalError, ensure_user_caller};
use super::broker::gh::{AllowedGhSubcommand, GhError, run_gh_bin};
use super::broker::job::job_dir;
use super::events::{Event, append_locked, reject_if_symlink, set_private_file_mode};
use super::evidence::{self, EvidenceItem};
use super::ids::is_valid_run_id;
use super::machine::{self, EventData};
use super::model::{Actor, Idea, PrLink, Run};
use super::run::{RunError, get_run};
use super::stages::plan::SlicesManifest;
use super::stages::review::{ReviewRoundsReport, ReviewVerdict, load_findings_report};
use super::store::{FactoryStore, current_time, event_run_id, write_atomic};
use super::workflow::schema::EvidenceKind;

const GH_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum HandoffError {
    #[error(transparent)]
    Factory(#[from] FactoryError),
    #[error(transparent)]
    Approval(#[from] ApprovalError),
    #[error(transparent)]
    Run(#[from] RunError),
    #[error(transparent)]
    Gh(#[from] GhError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("destination already exists: {}", .0.display())]
    AlreadyExists(PathBuf),
    #[error("{0}")]
    InvalidInput(String),
}

impl HandoffError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Approval(e) => e.exit_code(),
            Self::AlreadyExists(_) => 1,
            _ => 1,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Approval(e) => e.code(),
            Self::AlreadyExists(_) => "already_exists",
            _ => "handoff_error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportSpecsResult {
    pub phase_dir: String,
    pub destination: PathBuf,
    pub files: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct StageResultSummary {
    #[serde(default)]
    summary: String,
}

#[derive(Debug, Deserialize)]
struct CheckReportRead {
    checks: Vec<CheckEntryRead>,
}

#[derive(Debug, Deserialize)]
struct CheckEntryRead {
    command: Vec<String>,
    passed: bool,
    exit_code: Option<i32>,
    timed_out: bool,
}

#[derive(Debug, Deserialize)]
struct ExceptionPayloadRead {
    exception: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhPrViewStatus {
    state: String,
    #[serde(default)]
    status_check_rollup: Vec<GhStatusCheck>,
}

#[derive(Debug, Deserialize)]
struct GhStatusCheck {
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

fn summarize_what_was_built(store: &FactoryStore, idea: &Idea, run: &Run) -> String {
    let mut lines = vec![format!("Idea `{}`: {}", idea.id, idea.title)];

    let artifacts_dir = store.run_dir(&run.id).join("artifacts");
    let slices_path = if artifacts_dir.join("slices.json").is_file() {
        artifacts_dir.join("slices.json")
    } else {
        store.run_dir(&run.id).join("slices.json")
    };
    if reject_if_symlink(&slices_path).is_ok()
        && let Ok(raw) = fs::read_to_string(&slices_path)
        && let Ok(manifest) = serde_json::from_str::<SlicesManifest>(&raw)
    {
        if let Some(phase) = &manifest.phase {
            lines.push(format!("Plan phase: `{phase}`"));
        }
        for slice in &manifest.slices {
            lines.push(format!(
                "- `{}`: {} ({})",
                slice.id, slice.title, slice.slice_type
            ));
        }
    }

    if !run.stages.is_empty() {
        lines.push(String::new());
        lines.push("Stages:".to_string());
        for stage in &run.stages {
            let attempt = stage.attempts.max(1);
            let result_path = job_dir(store, &run.id, &stage.name, attempt).join("result.json");
            let summary = if reject_if_symlink(&result_path).is_ok()
                && let Ok(raw) = fs::read_to_string(&result_path)
                && let Ok(parsed) = serde_json::from_str::<StageResultSummary>(&raw)
                && !parsed.summary.trim().is_empty()
            {
                parsed.summary.trim().to_string()
            } else if let Some(rat) = &stage.rationale
                && !rat.trim().is_empty()
            {
                rat.trim().to_string()
            } else {
                format!("{:?}", stage.state).to_lowercase()
            };
            lines.push(format!("- `{}`: {summary}", stage.name));
        }
    }

    lines.join("\n")
}

fn summarize_tests_section(store: &FactoryStore, evidence: &[EvidenceItem]) -> String {
    let mut lines = Vec::new();
    for kind in [EvidenceKind::TestReport, EvidenceKind::LintReport] {
        if let Some(item) = evidence.iter().rfind(|i| i.kind == kind) {
            let path = store.root.join(&item.path);
            if reject_if_symlink(&path).is_ok()
                && let Ok(raw) = fs::read_to_string(&path)
                && let Ok(parsed) = serde_json::from_str::<CheckReportRead>(&raw)
            {
                for entry in parsed.checks {
                    let cmd = entry.command.join(" ");
                    let result = if entry.passed {
                        "passed".to_string()
                    } else if entry.timed_out {
                        "timed out".to_string()
                    } else {
                        format!(
                            "failed (exit {})",
                            entry
                                .exit_code
                                .map_or_else(|| "signal".to_string(), |c| c.to_string())
                        )
                    };
                    lines.push(format!("- `{cmd}`: {result}"));
                }
            }
            if !item.summary.trim().is_empty() {
                lines.push(format!("- Summary: {}", item.summary.trim()));
            }
        }
    }

    for item in evidence
        .iter()
        .filter(|i| i.kind == EvidenceKind::Exception)
    {
        let path = store.root.join(&item.path);
        let label = if reject_if_symlink(&path).is_ok()
            && let Ok(raw) = fs::read_to_string(&path)
            && let Ok(parsed) = serde_json::from_str::<ExceptionPayloadRead>(&raw)
            && let Some(exc) = parsed.exception
        {
            exc
        } else {
            "exception".to_string()
        };
        lines.push(format!(
            "- Exception ({}, {label}): {}",
            item.stage, item.summary
        ));
    }

    if lines.is_empty() {
        "No test or lint runs recorded.".to_string()
    } else {
        lines.join("\n")
    }
}

fn resolve_review_dir(store: &FactoryStore, run_id: &str) -> PathBuf {
    let artifacts_dir = store.run_dir(run_id).join("artifacts");
    if artifacts_dir.join("review-rounds.json").is_file()
        || artifacts_dir.join("findings.json").is_file()
    {
        artifacts_dir
    } else {
        store.run_dir(run_id)
    }
}

fn summarize_review_section(store: &FactoryStore, run_id: &str) -> (String, Vec<String>) {
    let review_dir = resolve_review_dir(store, run_id);
    let rounds_path = review_dir.join("review-rounds.json");
    if reject_if_symlink(&rounds_path).is_ok()
        && let Ok(raw) = fs::read_to_string(&rounds_path)
        && let Ok(rounds) = serde_json::from_str::<ReviewRoundsReport>(&raw)
        && let Some(last) = rounds.rounds.last()
    {
        let mut fixed: BTreeSet<String> = BTreeSet::new();
        for round in &rounds.rounds {
            for id in &round.fixed_from_previous {
                fixed.insert(id.clone());
            }
        }
        let verdict = match last.stated_verdict {
            ReviewVerdict::Approve => "approve",
            ReviewVerdict::ChangesRequested => "changes_requested",
        };
        let mut lines = vec![format!(
            "Verdict: {verdict} · Rounds: {} · Findings fixed: {} · Still open: {}",
            rounds.rounds.len(),
            fixed.len(),
            last.findings.len()
        )];
        let mut follow_ups = Vec::new();
        for finding in &last.findings {
            let sev = format!("{:?}", finding.severity).to_lowercase();
            let loc = format!("{}:{}", finding.file, finding.line);
            let entry = format!("- [{sev}] {} (`{loc}`)", finding.title);
            lines.push(entry.clone());
            follow_ups.push(entry);
        }
        return (lines.join("\n"), follow_ups);
    }

    if let Ok(report) = load_findings_report(&review_dir) {
        let verdict = match report.verdict {
            ReviewVerdict::Approve => "approve",
            ReviewVerdict::ChangesRequested => "changes_requested",
        };
        let mut lines = vec![format!(
            "Verdict: {verdict} · Rounds: 1 · Findings fixed: 0 · Still open: {}",
            report.findings.len()
        )];
        let mut follow_ups = Vec::new();
        for finding in &report.findings {
            let sev = format!("{:?}", finding.severity).to_lowercase();
            let loc = format!("{}:{}", finding.file, finding.line);
            let entry = format!("- [{sev}] {} (`{loc}`)", finding.title);
            lines.push(entry.clone());
            follow_ups.push(entry);
        }
        return (lines.join("\n"), follow_ups);
    }

    (
        "Rounds: 0 · Findings fixed: 0 · Still open: 0".to_string(),
        Vec::new(),
    )
}

fn collect_spec_rel_paths(base: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if reject_if_symlink(&path).is_err() {
            continue;
        }
        if path.is_dir() {
            collect_spec_rel_paths(base, &path, out);
        } else if path.is_file()
            && let Ok(rel) = path.strip_prefix(base)
        {
            out.push(rel.to_string_lossy().into_owned());
        }
    }
}

pub fn build_handoff_markdown(
    store: &FactoryStore,
    _workspace: &Path,
    idea: &Idea,
    run: &Run,
) -> String {
    let what_was_built = summarize_what_was_built(store, idea, run);
    let tests_section = summarize_tests_section(store, &run.evidence);
    let (review_section, review_follow_ups) = summarize_review_section(store, &run.id);

    let risk_line = match idea.risk {
        Some(r) => format!("- Risk level: {}", format!("{r:?}").to_lowercase()),
        None => "- Risk level: none recorded".to_string(),
    };
    let mut risk_lines = vec![risk_line];
    for assumption in &idea.assumptions {
        risk_lines.push(format!("- Open assumption: {assumption}"));
    }
    risk_lines.extend(review_follow_ups);
    let risks_section = risk_lines.join("\n");

    let prs = if run.prs.is_empty() {
        &idea.prs
    } else {
        &run.prs
    };
    let prs_section = if prs.is_empty() {
        "No pull requests recorded.".to_string()
    } else {
        prs.iter()
            .map(|pr| {
                let mut extra = Vec::new();
                if let Some(st) = &pr.state {
                    extra.push(format!("state: {st}"));
                }
                if let Some(chk) = &pr.checks_summary {
                    extra.push(format!("checks: {chk}"));
                }
                if extra.is_empty() {
                    format!("- `{}` #{}: {}", pr.repo, pr.number, pr.url)
                } else {
                    format!(
                        "- `{}` #{}: {} ({})",
                        pr.repo,
                        pr.number,
                        pr.url,
                        extra.join(", ")
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let branch = run.branch.as_deref().unwrap_or("none recorded");
    let worktree = run.worktree_path.as_deref().unwrap_or("none recorded");
    let artifacts_dir = store.run_dir(&run.id).join("artifacts");
    let mut spec_files = Vec::new();
    collect_spec_rel_paths(
        &artifacts_dir,
        &artifacts_dir.join("specs"),
        &mut spec_files,
    );
    let specs_line = if spec_files.is_empty() {
        "none recorded".to_string()
    } else {
        spec_files.join(", ")
    };
    let where_section =
        format!("- Branch: `{branch}`\n- Worktree: `{worktree}`\n- Plan specs: {specs_line}");

    let deploy_note = if run.deploy_requested {
        "Deployment was requested; user still owns executing and verifying deployment."
    } else {
        "Deployment stays gated unless the user authorized it."
    };
    let owns_section =
        format!("- Review and merge of the pull request(s).\n- Deployment ({deploy_note})");

    format!(
        "# Handoff Summary: {}\n\n\
         ## What was built\n{what_was_built}\n\n\
         ## Tests\n{tests_section}\n\n\
         ## Review\n{review_section}\n\n\
         ## Risks and follow-up\n{risks_section}\n\n\
         ## Pull request links\n{prs_section}\n\n\
         ## Where the work is\n{where_section}\n\n\
         ## What the user still owns\n{owns_section}\n",
        run.id
    )
}

pub fn write_and_record_handoff(
    store: &FactoryStore,
    workspace: &Path,
    run_id: &str,
    actor: &Actor,
) -> Result<(PathBuf, EvidenceItem), HandoffError> {
    if !is_valid_run_id(run_id) {
        return Err(FactoryError::InvalidInput(format!("invalid run id: {run_id}")).into());
    }
    let run = get_run(store, run_id)?.ok_or_else(|| RunError::NotFound(format!("run {run_id}")))?;
    let idea = store
        .get_idea(&run.idea)?
        .ok_or_else(|| FactoryError::InvalidInput(format!("idea {} not found", run.idea)))?;

    let markdown = build_handoff_markdown(store, workspace, &idea, &run);
    let handoff_path = store.run_dir(run_id).join("handoff.md");
    reject_if_symlink(&handoff_path)?;
    write_atomic(&handoff_path, markdown.as_bytes())?;
    set_private_file_mode(&handoff_path)?;

    if let Some(existing) = run
        .evidence
        .iter()
        .find(|e| e.kind == EvidenceKind::Handoff)
        .cloned()
    {
        return Ok((handoff_path, existing));
    }

    let pr_count = if run.prs.is_empty() {
        idea.prs.len()
    } else {
        run.prs.len()
    };
    let item = evidence::add(
        store,
        evidence::AddEvidenceInput {
            idea_id: &idea.id,
            run_id,
            kind: EvidenceKind::Handoff,
            stage: "deliver",
            attempt: 1,
            extension: "md",
            content: markdown.as_bytes(),
            summary: format!("handoff summary for {} ({pr_count} PR(s))", idea.id),
        },
        actor,
    )?;

    Ok((handoff_path, item))
}

fn normalize_pr_state(raw: &str) -> String {
    match raw.trim().to_ascii_uppercase().as_str() {
        "OPEN" => "open".to_string(),
        "MERGED" => "merged".to_string(),
        "CLOSED" => "closed".to_string(),
        other => other.to_ascii_lowercase(),
    }
}

fn summarize_checks_rollup(checks: &[GhStatusCheck]) -> String {
    if checks.is_empty() {
        return "no checks".to_string();
    }
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut pending = 0usize;

    for check in checks {
        if let Some(conclusion) = check.conclusion.as_deref().filter(|s| !s.is_empty()) {
            match conclusion.to_ascii_uppercase().as_str() {
                "SUCCESS" | "NEUTRAL" | "SKIPPED" => passed += 1,
                "FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE"
                | "STALE" => failed += 1,
                _ => pending += 1,
            }
        } else if let Some(state) = check.state.as_deref().filter(|s| !s.is_empty()) {
            match state.to_ascii_uppercase().as_str() {
                "SUCCESS" => passed += 1,
                "FAILURE" | "ERROR" => failed += 1,
                _ => pending += 1,
            }
        } else if let Some(status) = check.status.as_deref()
            && status.eq_ignore_ascii_case("COMPLETED")
        {
            passed += 1;
        } else {
            pending += 1;
        }
    }

    let mut parts = Vec::new();
    if passed > 0 {
        parts.push(format!("{passed} passed"));
    }
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    if pending > 0 {
        parts.push(format!("{pending} pending"));
    }
    if parts.is_empty() {
        "no checks".to_string()
    } else {
        parts.join(", ")
    }
}

pub fn refresh_run_prs(
    store: &FactoryStore,
    workspace: &Path,
    run_id: &str,
    actor: &Actor,
) -> Result<Vec<PrLink>, HandoffError> {
    refresh_run_prs_with_gh(store, workspace, run_id, actor, Path::new("gh"))
}

pub fn refresh_run_prs_with_gh(
    store: &FactoryStore,
    workspace: &Path,
    run_id: &str,
    actor: &Actor,
    gh_binary: &Path,
) -> Result<Vec<PrLink>, HandoffError> {
    if !is_valid_run_id(run_id) {
        return Err(FactoryError::InvalidInput(format!("invalid run id: {run_id}")).into());
    }
    let run = get_run(store, run_id)?.ok_or_else(|| RunError::NotFound(format!("run {run_id}")))?;
    let idea = store
        .get_idea(&run.idea)?
        .ok_or_else(|| FactoryError::InvalidInput(format!("idea {} not found", run.idea)))?;

    let prs = if run.prs.is_empty() {
        idea.prs.clone()
    } else {
        run.prs.clone()
    };
    if prs.is_empty() {
        return Ok(Vec::new());
    }

    let cwd = run
        .worktree_path
        .as_deref()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| workspace.to_path_buf());

    let mut refreshed = Vec::with_capacity(prs.len());
    for mut pr in prs {
        let out = run_gh_bin(
            gh_binary,
            AllowedGhSubcommand::PrView,
            &[&pr.url, "--json", "state,statusCheckRollup"],
            &cwd,
            GH_TIMEOUT,
        )?;
        let parsed: GhPrViewStatus = serde_json::from_str(out.trim())?;
        pr.state = Some(normalize_pr_state(&parsed.state));
        pr.checks_summary = Some(summarize_checks_rollup(&parsed.status_check_rollup));
        refreshed.push(pr);
    }

    store.ensure()?;
    let lock_path = store.lock_path();
    let lock_file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    set_private_file_mode(&lock_path)?;
    let _guard = exclusive_lock(&lock_file)?;

    let mut current_idea = store
        .get_idea(&idea.id)?
        .ok_or_else(|| FactoryError::InvalidInput(format!("idea {} not found", idea.id)))?;
    let mut current_run =
        get_run(store, run_id)?.ok_or_else(|| RunError::NotFound(format!("run {run_id}")))?;

    let ts = current_time();
    let mut events = Vec::with_capacity(refreshed.len());
    let mut ev_datas = Vec::with_capacity(refreshed.len());
    for pr in &refreshed {
        let ev_data = EventData::PrStatusRefreshed {
            run: run_id.to_string(),
            pr: pr.clone(),
        };
        let ev = Event::from_data(
            ts,
            actor.clone(),
            idea.id.clone(),
            event_run_id(&ev_data),
            &ev_data,
        )?;
        events.push(ev);
        ev_datas.push(ev_data);
    }

    let appended = append_locked(&store.events_path(), &events)?;
    for (ev, ev_data) in appended.iter().zip(ev_datas.iter()) {
        if let Some(next) = machine::apply(Some(current_idea.clone()), ev_data, ev.ts) {
            current_idea = next;
        }
        if let Some(next) = machine::apply_run(Some(current_run.clone()), ev_data, &idea.id, ev.ts)
        {
            current_run = next;
        }
    }

    let idea_bytes = serde_json::to_vec_pretty(&current_idea)?;
    write_atomic(&store.idea_path(&idea.id), &idea_bytes)?;
    let run_bytes = serde_json::to_vec_pretty(&current_run)?;
    write_atomic(&store.run_json_path(run_id), &run_bytes)?;

    Ok(current_run.prs)
}

fn sanitize_target_slug(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "workspace".to_string()
    } else {
        trimmed
    }
}

fn parse_phase_number(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("p-").unwrap_or(name);
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.len() >= 3 && rest[digits.len()..].starts_with('-') {
        digits.parse::<u32>().ok()
    } else {
        None
    }
}

fn next_phase_number(workspace: &Path) -> Result<String, HandoffError> {
    if let Some(num) = super::broker::checks::next_phase_via_specs_py(workspace) {
        return Ok(num);
    }
    let specs_root = workspace.join("specs");
    if !specs_root.exists() {
        return Ok("001".to_string());
    }
    reject_if_symlink(&specs_root)?;
    let mut max_num = 0u32;
    for entry in fs::read_dir(&specs_root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && let Some(num) = parse_phase_number(&entry.file_name().to_string_lossy())
        {
            max_num = max_num.max(num);
        }
    }
    Ok(format!("{:03}", max_num + 1))
}

fn resolve_specs_source(specs_root: &Path) -> Result<PathBuf, HandoffError> {
    reject_if_symlink(specs_root)?;
    if !specs_root.is_dir() {
        return Err(HandoffError::InvalidInput(format!(
            "no plan specs found at {}",
            specs_root.display()
        )));
    }
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in fs::read_dir(specs_root)? {
        let entry = entry?;
        let path = entry.path();
        reject_if_symlink(&path)?;
        if path.is_dir() {
            dirs.push(path);
        } else if path.is_file() {
            files.push(path);
        }
    }
    if files.is_empty() && dirs.len() == 1 {
        Ok(dirs.into_iter().next().unwrap())
    } else {
        Ok(specs_root.to_path_buf())
    }
}

fn copy_dir_no_symlinks(
    src_root: &Path,
    src_dir: &Path,
    dst_dir: &Path,
    copied: &mut Vec<String>,
) -> Result<(), HandoffError> {
    fs::create_dir_all(dst_dir)?;
    let mut entries: Vec<PathBuf> = fs::read_dir(src_dir)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|e| e.path())
        .collect();
    entries.sort();

    for path in entries {
        reject_if_symlink(&path)?;
        let name = path
            .file_name()
            .ok_or_else(|| HandoffError::InvalidInput("invalid spec entry name".to_string()))?;
        let dst_path = dst_dir.join(name);
        reject_if_symlink(&dst_path)?;
        if path.is_dir() {
            copy_dir_no_symlinks(src_root, &path, &dst_path, copied)?;
        } else if path.is_file() {
            if dst_path.exists() {
                return Err(HandoffError::AlreadyExists(dst_path));
            }
            fs::copy(&path, &dst_path)?;
            if let Ok(rel) = path.strip_prefix(src_root) {
                copied.push(rel.to_string_lossy().into_owned());
            }
        }
    }
    Ok(())
}

pub fn export_specs(
    store: &FactoryStore,
    workspace: &Path,
    run_id: &str,
    env_get: &dyn Fn(&str) -> Option<String>,
) -> Result<ExportSpecsResult, HandoffError> {
    ensure_user_caller(env_get)?;
    if !is_valid_run_id(run_id) {
        return Err(FactoryError::InvalidInput(format!("invalid run id: {run_id}")).into());
    }
    let run = get_run(store, run_id)?.ok_or_else(|| RunError::NotFound(format!("run {run_id}")))?;

    let artifacts_dir = store.run_dir(run_id).join("artifacts");
    reject_if_symlink(&artifacts_dir)?;
    let specs_src = resolve_specs_source(&artifacts_dir.join("specs"))?;

    let target_raw = run
        .target
        .as_deref()
        .filter(|t| !t.trim().is_empty() && *t != ".")
        .or_else(|| workspace.file_name().and_then(|n| n.to_str()))
        .unwrap_or("workspace");
    let target_slug = sanitize_target_slug(target_raw);

    let phase_num = next_phase_number(workspace)?;
    let phase_dir = format!("p-{phase_num}-{target_slug}");

    let specs_root = workspace.join("specs");
    reject_if_symlink(&specs_root)?;
    let destination = specs_root.join(&phase_dir);
    reject_if_symlink(&destination)?;
    if destination.exists() {
        return Err(HandoffError::AlreadyExists(destination));
    }

    let mut files = Vec::new();
    copy_dir_no_symlinks(&specs_src, &specs_src, &destination, &mut files)?;
    if files.is_empty() {
        let _ = fs::remove_dir_all(&destination);
        return Err(HandoffError::InvalidInput(
            "no spec files found in run artifacts".to_string(),
        ));
    }

    Ok(ExportSpecsResult {
        phase_dir,
        destination,
        files,
    })
}
