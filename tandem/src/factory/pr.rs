use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::coordination::filelock::exclusive_lock;

use super::FactoryError;
use super::broker::gh::{AllowedGhSubcommand, GhError, run_gh_bin};
use super::broker::git::{AllowedGitSubcommand, GitError, push_branch, run_git};
use super::events::{Event, append_locked, reject_if_symlink, set_private_file_mode};
use super::evidence::{self, EvidenceItem};
use super::ids::is_valid_run_id;
use super::machine::{self, EventData};
use super::model::{Actor, Idea, PrLink, Run};
use super::run::{RunError, get_run};
use super::stages::review::{ReviewRoundsReport, ReviewVerdict, load_findings_report};
use super::store::{FactoryStore, current_time, event_run_id, write_atomic};
use super::workflow::parse::load_workflow_file;
use super::workflow::schema::EvidenceKind;
use super::workflow::validate::validate;

const GIT_TIMEOUT: Duration = Duration::from_secs(15);
const GH_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrOutcome {
    Created(PrLink),
    AlreadyExists(PrLink),
    Paused {
        reason: String,
        commands: Vec<String>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum PrError {
    #[error(transparent)]
    Factory(#[from] FactoryError),
    #[error(transparent)]
    Run(#[from] RunError),
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Gh(#[from] GhError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Deserialize)]
struct GhPrSummary {
    number: u64,
    url: String,
}

#[derive(Debug, Deserialize)]
struct GhDefaultBranchRef {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GhRepoView {
    name_with_owner: Option<String>,
    default_branch_ref: Option<GhDefaultBranchRef>,
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

fn is_protected_branch(branch: &str) -> bool {
    matches!(branch.trim(), "main" | "master" | "trunk")
}

pub fn parse_github_repo(remote_url: &str) -> Option<String> {
    let trimmed = remote_url.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path = if let Some(rest) = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .or_else(|| trimmed.strip_prefix("ssh://"))
    {
        let without_user = rest.split_once('@').map(|(_, r)| r).unwrap_or(rest);
        let (host, repo_path) = without_user.split_once('/')?;
        if !host.eq_ignore_ascii_case("github.com") {
            return None;
        }
        repo_path
    } else {
        trimmed.strip_prefix("git@github.com:")?
    };

    let clean = path.trim_start_matches('/').trim_end_matches(".git");
    let mut parts = clean.split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

fn sanitize_text(input: &str, workspace: &Path, worktree: &Path) -> String {
    let mut out = input.replace('\r', "");
    for path in [workspace, worktree] {
        let p = path.display().to_string();
        if !p.is_empty() {
            out = out.replace(&p, "[local-path]");
        }
    }
    for prefix in [
        "/Users/",
        "/home/",
        "/tmp/",
        "/var/",
        "C:\\",
        ".agent-workspace/",
        "ghp_",
        "gho_",
        "github_pat_",
        "sk-",
        "AKIA",
        "-----BEGIN ",
    ] {
        if out.contains(prefix) {
            out = redact_tokens_with_prefix(&out, prefix);
        }
    }
    out
}

fn redact_tokens_with_prefix(input: &str, prefix: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(idx) = rest.find(prefix) {
        result.push_str(&rest[..idx]);
        result.push_str("[redacted]");
        let after = &rest[idx + prefix.len()..];
        let end = after
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ')' | ']' | ','))
            .unwrap_or(after.len());
        rest = &after[end..];
    }
    result.push_str(rest);
    result
}

pub fn conventional_pr_title(idea_title: &str) -> String {
    let trimmed = idea_title.trim();
    let known_prefixes = [
        "feat:",
        "feat(",
        "fix:",
        "fix(",
        "refactor:",
        "refactor(",
        "docs:",
        "docs(",
        "test:",
        "test(",
        "chore:",
        "chore(",
        "perf:",
        "perf(",
        "ci:",
        "ci(",
        "build:",
        "build(",
    ];
    let lower = trimmed.to_ascii_lowercase();
    if known_prefixes.iter().any(|p| lower.starts_with(p)) {
        return trimmed.to_string();
    }
    let mut chars = trimmed.chars();
    match chars.next() {
        Some(first) => {
            let mut s = String::from("feat: ");
            s.extend(first.to_lowercase());
            s.push_str(chars.as_str());
            s
        }
        None => "feat: factory change".to_string(),
    }
}

fn pause_run_for_pr(
    store: &FactoryStore,
    run: Run,
    reason: String,
    commands: Vec<String>,
    actor: &Actor,
) -> Result<PrOutcome, PrError> {
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

    let ts = current_time();
    let ev_data = EventData::RunPaused {
        run: run.id.clone(),
        reason: reason.clone(),
        commands: commands.clone(),
    };
    let ev = Event::from_data(
        ts,
        actor.clone(),
        run.idea.clone(),
        Some(run.id.clone()),
        &ev_data,
    )?;
    let appended = append_locked(&store.events_path(), &[ev])?;
    let event_ts = appended.first().map(|e| e.ts).unwrap_or(ts);
    if let Some(updated_run) = machine::apply_run(Some(run), &ev_data, &appended[0].idea, event_ts)
    {
        let bytes = serde_json::to_vec_pretty(&updated_run)?;
        write_atomic(&store.run_json_path(&updated_run.id), &bytes)?;
    }

    Ok(PrOutcome::Paused { reason, commands })
}

fn record_pr_link(
    store: &FactoryStore,
    idea_id: &str,
    run_id: &str,
    pr: &PrLink,
    actor: &Actor,
) -> Result<(), PrError> {
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

    let current_idea = store
        .get_idea(idea_id)?
        .ok_or_else(|| FactoryError::InvalidInput(format!("idea {idea_id} not found")))?;
    let current_run =
        get_run(store, run_id)?.ok_or_else(|| RunError::NotFound(format!("run {run_id}")))?;

    let already_on_idea = current_idea
        .prs
        .iter()
        .any(|p| p.url == pr.url || (p.repo == pr.repo && p.number == pr.number));
    let already_on_run = current_run
        .prs
        .iter()
        .any(|p| p.url == pr.url || (p.repo == pr.repo && p.number == pr.number));

    if already_on_idea && already_on_run {
        return Ok(());
    }

    let ts = current_time();
    let ev_data = EventData::PrCreated {
        run: run_id.to_string(),
        pr: pr.clone(),
    };
    let ev = Event::from_data(
        ts,
        actor.clone(),
        idea_id.to_string(),
        event_run_id(&ev_data),
        &ev_data,
    )?;
    let appended = append_locked(&store.events_path(), &[ev])?;
    let event_ts = appended.first().map(|e| e.ts).unwrap_or(ts);

    if let Some(updated_idea) = machine::apply(Some(current_idea), &ev_data, event_ts) {
        let bytes = serde_json::to_vec_pretty(&updated_idea)?;
        write_atomic(&store.idea_path(idea_id), &bytes)?;
    }
    if let Some(updated_run) = machine::apply_run(Some(current_run), &ev_data, idea_id, event_ts) {
        let bytes = serde_json::to_vec_pretty(&updated_run)?;
        write_atomic(&store.run_json_path(run_id), &bytes)?;
    }

    Ok(())
}

fn summarize_tests_section(
    store: &FactoryStore,
    evidence: &[EvidenceItem],
    workspace: &Path,
    worktree: &Path,
) -> String {
    let mut lines = Vec::new();
    for kind in [EvidenceKind::TestReport, EvidenceKind::LintReport] {
        let latest = evidence.iter().rfind(|item| item.kind == kind);
        if let Some(item) = latest {
            let path = store.root.join(&item.path);
            if reject_if_symlink(&path).is_ok()
                && let Ok(raw) = fs::read_to_string(&path)
                && let Ok(parsed) = serde_json::from_str::<CheckReportRead>(&raw)
            {
                for entry in parsed.checks {
                    let cmd = sanitize_text(&entry.command.join(" "), workspace, worktree);
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
                lines.push(format!(
                    "- Summary: {}",
                    sanitize_text(&item.summary, workspace, worktree)
                ));
            }
        }
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

fn summarize_review_section(artifact_dir: &Path) -> String {
    let rounds_path = artifact_dir.join("review-rounds.json");
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
        let open_count = last.findings.len();
        let verdict = match last.stated_verdict {
            ReviewVerdict::Approve => "approve",
            ReviewVerdict::ChangesRequested => "changes_requested",
        };
        return format!(
            "Verdict: {verdict} · Rounds: {} · Findings fixed: {} · Still open: {open_count}",
            rounds.rounds.len(),
            fixed.len()
        );
    }
    if let Ok(report) = load_findings_report(artifact_dir) {
        let verdict = match report.verdict {
            ReviewVerdict::Approve => "approve",
            ReviewVerdict::ChangesRequested => "changes_requested",
        };
        return format!(
            "Verdict: {verdict} · Findings fixed: 0 · Still open: {}",
            report.findings.len()
        );
    }
    "Verdict: approve · Findings fixed: 0 · Still open: 0".to_string()
}

fn summarize_exceptions_section(
    store: &FactoryStore,
    evidence: &[EvidenceItem],
    workspace: &Path,
    worktree: &Path,
) -> String {
    let mut lines = Vec::new();
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
        let summary = sanitize_text(&item.summary, workspace, worktree);
        lines.push(format!("- {} ({}): {summary}", item.stage, label));
    }
    if lines.is_empty() {
        "None.".to_string()
    } else {
        lines.join("\n")
    }
}

fn build_pr_body(
    store: &FactoryStore,
    idea: &Idea,
    run: &Run,
    workspace: &Path,
    worktree: &Path,
) -> String {
    let diff_summary = run
        .evidence
        .iter()
        .rfind(|i| i.kind == EvidenceKind::DiffStat)
        .map(|i| sanitize_text(&i.summary, workspace, worktree))
        .unwrap_or_else(|| "No diff summary recorded.".to_string());

    let tests_summary = summarize_tests_section(store, &run.evidence, workspace, worktree);
    let review_summary = summarize_review_section(&resolve_review_dir(store, &run.id));
    let risk_summary = match idea.risk {
        Some(r) => format!("{r:?}").to_lowercase(),
        None => "none recorded".to_string(),
    };
    let follow_up = if idea.assumptions.is_empty() {
        "None.".to_string()
    } else {
        format!(
            "{} open assumption(s) recorded on idea.",
            idea.assumptions.len()
        )
    };
    let exceptions_summary =
        summarize_exceptions_section(store, &run.evidence, workspace, worktree);
    let screenshot_count = run
        .evidence
        .iter()
        .filter(|i| i.kind == EvidenceKind::Screenshot)
        .count();
    let demo_section = if screenshot_count > 0 {
        format!(
            "## Demo\n{screenshot_count} local screenshot(s) captured (stored locally; not uploaded).\n\n"
        )
    } else {
        String::new()
    };
    let clean_title = sanitize_text(&idea.title, workspace, worktree);

    let body = format!(
        "## What changed\n{diff_summary}\n\n\
         ## Tests\n{tests_summary}\n\n\
         ## Review\n{review_summary}\n\n\
         {demo_section}\
         ## Risks\n{risk_summary}\n\n\
         ## Follow-up\n{follow_up}\n\n\
         ## Recorded exceptions\n{exceptions_summary}\n\n\
         Factory idea: {} {clean_title}\n",
        idea.id
    );
    sanitize_text(&body, workspace, worktree)
}

fn parse_pr_number_from_url(url: &str) -> Option<u64> {
    url.trim().rsplit('/').next()?.parse::<u64>().ok()
}

pub fn open_pull_request(
    store: &FactoryStore,
    workspace: &Path,
    run_id: &str,
    actor: &Actor,
) -> Result<PrOutcome, PrError> {
    open_pull_request_with_gh(store, workspace, run_id, actor, Path::new("gh"))
}

pub fn open_pull_request_with_gh(
    store: &FactoryStore,
    workspace: &Path,
    run_id: &str,
    actor: &Actor,
    gh_binary: &Path,
) -> Result<PrOutcome, PrError> {
    if !is_valid_run_id(run_id) {
        return Err(FactoryError::InvalidInput(format!("invalid run id: {run_id}")).into());
    }

    let run = get_run(store, run_id)?.ok_or_else(|| RunError::NotFound(format!("run {run_id}")))?;
    let idea = store
        .get_idea(&run.idea)?
        .ok_or_else(|| FactoryError::InvalidInput(format!("idea {} not found", run.idea)))?;

    let artifact_dir = store.run_dir(run_id);
    let review_dir = resolve_review_dir(store, run_id);
    let findings = match load_findings_report(&review_dir) {
        Ok(report) if report.verdict == ReviewVerdict::Approve => report,
        _ => {
            return pause_run_for_pr(
                store,
                run,
                "review verdict is not approve".to_string(),
                vec!["complete review approval before opening pull request".to_string()],
                actor,
            );
        }
    };
    let _ = findings;

    let settings = store.settings()?;
    let wf_path = artifact_dir.join("workflow.yaml");
    let raw_wf =
        load_workflow_file(&wf_path).map_err(|e| FactoryError::InvalidInput(e.to_string()))?;
    let wf = validate(&raw_wf, &settings).map_err(|diags| {
        FactoryError::InvalidInput(
            diags
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;

    let missing_evidence = evidence::missing(&run, &wf);
    if !missing_evidence.is_empty() {
        let names: Vec<&str> = missing_evidence.iter().map(EvidenceKind::as_str).collect();
        return pause_run_for_pr(
            store,
            run,
            format!("incomplete evidence: {}", names.join(", ")),
            vec!["record all required evidence before opening pull request".to_string()],
            actor,
        );
    }

    let Some(branch) = run.branch.clone().filter(|b| !b.trim().is_empty()) else {
        return pause_run_for_pr(
            store,
            run,
            "run has no branch configured".to_string(),
            vec!["prepare run worktree before opening pull request".to_string()],
            actor,
        );
    };
    if is_protected_branch(&branch) {
        return pause_run_for_pr(
            store,
            run,
            format!("refusing to open pull request from protected branch {branch}"),
            vec![],
            actor,
        );
    }

    let Some(worktree) = run
        .worktree_path
        .as_deref()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
    else {
        return pause_run_for_pr(
            store,
            run,
            "run worktree does not exist".to_string(),
            vec!["prepare run worktree before opening pull request".to_string()],
            actor,
        );
    };

    let status_out = run_git(
        AllowedGitSubcommand::Status,
        &["--porcelain"],
        &worktree,
        GIT_TIMEOUT,
    )?;
    if !status_out.trim().is_empty() {
        return pause_run_for_pr(
            store,
            run,
            "worktree has uncommitted changes".to_string(),
            vec!["commit or clean uncommitted changes in the run worktree".to_string()],
            actor,
        );
    }

    let base_ref = run.base_commit.as_deref().filter(|s| !s.trim().is_empty());
    let count_out = match base_ref {
        Some(base) => {
            let commit_range = format!("{base}..HEAD");
            run_git(
                AllowedGitSubcommand::RevList,
                &["--count", &commit_range],
                &worktree,
                GIT_TIMEOUT,
            )?
        }
        None => match run_git(
            AllowedGitSubcommand::RevList,
            &["--count", "HEAD~1..HEAD"],
            &worktree,
            GIT_TIMEOUT,
        ) {
            Ok(out) => out,
            Err(GitError::Failed { .. }) => run_git(
                AllowedGitSubcommand::RevList,
                &["--count", "HEAD"],
                &worktree,
                GIT_TIMEOUT,
            )?,
            Err(err) => return Err(err.into()),
        },
    };
    let commit_count: u64 = count_out.trim().parse().unwrap_or(0);
    if commit_count == 0 {
        return pause_run_for_pr(
            store,
            run,
            "branch has no commits beyond base".to_string(),
            vec!["commit changes on the run branch before opening pull request".to_string()],
            actor,
        );
    }

    let remote_url = match run_git(
        AllowedGitSubcommand::Remote,
        &["get-url", "origin"],
        &worktree,
        GIT_TIMEOUT,
    ) {
        Ok(url) => url,
        Err(_) => {
            return pause_run_for_pr(
                store,
                run,
                format!("open the pull request by hand for branch {branch}"),
                vec![format!("open the pull request by hand for branch {branch}")],
                actor,
            );
        }
    };
    let Some(remote_repo) = parse_github_repo(&remote_url) else {
        return pause_run_for_pr(
            store,
            run,
            format!("open the pull request by hand for branch {branch}"),
            vec![format!("open the pull request by hand for branch {branch}")],
            actor,
        );
    };

    match run_gh_bin(
        gh_binary,
        AllowedGhSubcommand::AuthStatus,
        &[],
        &worktree,
        GH_TIMEOUT,
    ) {
        Ok(_) => {}
        Err(GhError::NotInstalled) | Err(GhError::Failed { .. }) => {
            return pause_run_for_pr(
                store,
                run,
                "run gh auth login".to_string(),
                vec!["gh auth login".to_string()],
                actor,
            );
        }
        Err(err) => return Err(err.into()),
    }

    push_branch(&worktree, &branch, GIT_TIMEOUT)?;

    let list_out = run_gh_bin(
        gh_binary,
        AllowedGhSubcommand::PrList,
        &["--head", &branch, "--json", "number,url"],
        &worktree,
        GH_TIMEOUT,
    )?;
    let existing: Vec<GhPrSummary> = serde_json::from_str(list_out.trim()).unwrap_or_default();
    if let Some(first) = existing.into_iter().next() {
        let pr_link = PrLink {
            repo: remote_repo,
            number: first.number,
            url: first.url,
            state: None,
            checks_summary: None,
        };
        record_pr_link(store, &idea.id, &run.id, &pr_link, actor)?;
        return Ok(PrOutcome::AlreadyExists(pr_link));
    }

    let repo_view_out = run_gh_bin(
        gh_binary,
        AllowedGhSubcommand::RepoView,
        &["--json", "nameWithOwner,defaultBranchRef"],
        &worktree,
        GH_TIMEOUT,
    )?;
    let repo_info: Option<GhRepoView> = serde_json::from_str(repo_view_out.trim()).ok();
    let repo_name = repo_info
        .as_ref()
        .and_then(|r| r.name_with_owner.clone())
        .unwrap_or(remote_repo);
    let base_branch = repo_info
        .as_ref()
        .and_then(|r| r.default_branch_ref.as_ref())
        .map(|b| b.name.clone())
        .filter(|b| !b.trim().is_empty())
        .unwrap_or_else(|| "main".to_string());

    let title = conventional_pr_title(&idea.title);
    let body = build_pr_body(store, &idea, &run, workspace, &worktree);
    let body_path = artifact_dir.join("pr-body.md");
    reject_if_symlink(&body_path)?;
    write_atomic(&body_path, body.as_bytes())?;
    set_private_file_mode(&body_path)?;
    let body_path_str = body_path.to_string_lossy().into_owned();

    let create_out = run_gh_bin(
        gh_binary,
        AllowedGhSubcommand::PrCreate,
        &[
            "--base",
            &base_branch,
            "--head",
            &branch,
            "--title",
            &title,
            "--body-file",
            &body_path_str,
        ],
        &worktree,
        GH_TIMEOUT,
    )?;

    let view_out = run_gh_bin(
        gh_binary,
        AllowedGhSubcommand::PrView,
        &[&branch, "--json", "number,url"],
        &worktree,
        GH_TIMEOUT,
    )
    .ok();

    let pr_summary = view_out
        .as_deref()
        .and_then(|raw| serde_json::from_str::<GhPrSummary>(raw.trim()).ok())
        .or_else(|| {
            let url = create_out.lines().last()?.trim().to_string();
            let number = parse_pr_number_from_url(&url)?;
            Some(GhPrSummary { number, url })
        })
        .ok_or_else(|| {
            FactoryError::InvalidInput("failed to parse created pull request metadata".to_string())
        })?;

    let pr_link = PrLink {
        repo: repo_name,
        number: pr_summary.number,
        url: pr_summary.url,
        state: None,
        checks_summary: None,
    };
    record_pr_link(store, &idea.id, &run.id, &pr_link, actor)?;
    Ok(PrOutcome::Created(pr_link))
}
