---
name: code-review-remediation
description: Use when the user asks to review and remediate pull requests end to end, resolve PR/MR review feedback, address reviewer comments, fix review-driven or CI failures, resolve merge conflicts, or get PRs merge-ready. Pulls open PRs/MRs, reviews them, comments, remediates, and loops until clear, then reports and asks before merging. Supports /code-review-remediation, with a PR number, or with a PR URL.
---

# Code Review Remediation

## Overview

One loop that takes pull requests (GitHub) or merge requests (GitLab) from open to merged: **pull -> review -> comment -> remediate -> repeat until all clear -> report -> ask -> merge**. This skill includes the review rubric; see [references/review-criteria.md](references/review-criteria.md). The standalone `code-review` skill remains available for review-only requests.

Use `gh` for GitHub and `glab` for GitLab, detected from the remote URL.

## Workflow

### 1. Identify Targets

- If the user gives a PR number or URL, target only that PR.
- Otherwise target **every open PR in every relevant repository**: the current repo plus each repo linked in `codebase-symlinks/` (resolve symlinks; read `knowledgebase/context-history/codebase-map.md` first when present). Skip drafts and PRs authored by bots unless the user says otherwise.
- Stop and report clearly if no PRs are found or the CLI is unauthenticated.

For each target collect: repo, number, title, author, base and head branches, linked issues, CI status, review state, and mergeability. Process PRs independently; for multiple targets use parallel subagents only when they touch separate repos or worktrees.

### 2. Prepare the Branch

1. Run `git status --short`; preserve unrelated user changes and ask before anything that could disturb them.
2. Fetch the remote. Prefer a dedicated worktree per PR so parallel work never collides.
3. Check out the PR head branch, pull latest, and fetch the base branch.
4. Never work on `main`, `master`, or `trunk` directly.

### 3. Review

Review the diff against the base using [references/review-criteria.md](references/review-criteria.md). Also read existing review threads and unresolved conversations; ignore resolved or outdated threads unless the issue persists. Produce findings ordered by severity.

### 4. Comment

Post the review on the PR/MR (`gh pr review` / `gh pr comment`, or `glab mr note`):

- Inline comments for line-specific findings, one summary comment for the rest.
- Include severity, category, file/line, impact, and recommendation.
- Mark the round (`Review round N`) so later rounds are traceable.
- Do not approve on the platform unless the user has asked; the verdict goes in the comment text.

### 5. Remediate

For every finding and every reviewer thread:

1. Classify it and validate whether it is correct, partially correct, or incorrect. Do not blindly apply suggestions; reply with justification when rejecting one.
2. Plan briefly: root cause, files, regression surface, tests needed.
3. Apply the smallest correct fix. Follow local architecture and style; no unrelated refactors, debug code, or leftover TODOs. Add or update tests for any behavior change or coverage gap.
4. Commit with the `commit` skill, reconcile with the base branch (resolve conflicts preserving both sides, re-run affected tests), and push. Use `--force-with-lease` only when rewritten history requires it; never plain `--force`.
5. Reply on each addressed thread and resolve it when the platform allows.

### 6. Validate (Local CI First)

Run CI locally whenever possible, before relying on the remote:

- Discover the pipeline: `.github/workflows/`, `.gitlab-ci.yml`, `Makefile`, package scripts, `composer.json`, `pyproject.toml`, and project docs.
- Prefer running the workflow itself (`act`, `gitlab-ci-local`) when installed; otherwise run the equivalent commands directly: install, lint, format, type check, static analysis, unit/feature/integration tests, build, and security checks.
- Never disable, skip, or weaken a check to get green.

**Remote CI failure with local CI passing**: do not assume either is right. Re-run the remote job once to rule out flakiness, diff environments (versions, env vars, services, OS, cache), and fix the cause if it is real. If it stays failing while local passes and the cause is environmental or flaky, post a PR comment with the evidence: commands run, versions, full pass output (or a log excerpt), commit SHA, and the failing remote job link with the suspected cause.

If CI cannot be run locally, state which checks were not run and why.

### 7. Loop

Repeat steps 3-6 until **all clear**:

- Latest review verdict is `APPROVED` (or `COMMENT ONLY` with only Low items addressed or deferred with justification).
- No unresolved threads or open Critical/High/Medium findings.
- Local CI is green and remote CI is green, or remote failure is documented with evidence per step 6.
- No merge conflicts with the base.

Cap at 5 rounds per PR. If still not clear, stop, post a status comment, and report what is blocking instead of looping further. Also stop and ask the user if a finding needs a product or business decision.

### 8. Merge Report

For each PR that is clear, write a report of what is to be merged:

- PR title/link, repo, head -> base, author, and linked issues.
- Summary of the change and the change inventory (areas, files, risk) with blast radius.
- Review rounds, findings found/fixed/rejected, and open deferred items.
- Validation: local commands run with results, remote CI status, and any evidence comments posted.
- Commits to be merged and the merge method (use the repo's convention; default squash only if the repo does).

Show the report in the terminal, and publish it per workspace conventions if the user wants to share it.

### 9. Ask, Then Merge

On green, ask the user whether to merge (one question covering all clear PRs; allow per-PR answers).

- **Yes**: merge using the repo's normal method (`gh pr merge` / `glab mr merge`), never bypassing branch protection or required checks. Delete the head branch only if the repo convention does.
- **No**: hold. Leave the PR open and unmerged, and do not ask again unless the user raises it.
- Never merge without an explicit yes in the current run; approval to review or remediate is not approval to merge.

### 10. After the Merge

Report nothing beyond the result. On success, reply exactly `Merged` (for multiple PRs, one line each: `Merged <repo>#<number>`). No summary, no recap.

If a merge fails, tell the user why in one or two lines (branch protection, required check, conflict, permissions, etc.) and the next step needed.

## Operating Rules

- Validate reviewer feedback against the codebase before implementing; prefer the smallest correct fix.
- Preserve business logic unless the review intentionally changes it.
- Respect user-owned working tree changes and other agents' branches; coordinate through `specs/in-progress.md` when present.
- Never include secrets or sensitive data in PR comments or evidence.
- Do not hide residual risk; report skipped checks, known regressions, and unresolved discussions before asking to merge.
