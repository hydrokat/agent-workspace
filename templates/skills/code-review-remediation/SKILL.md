---
name: code-review-remediation
description: Use when the user asks to resolve pull request review feedback, address reviewer comments, remediate requested changes, fix review-driven test failures, resolve PR merge conflicts, or push a ready-for-re-review remediation branch. Supports explicit invocations such as /code-review-remediation, /code-review-remediation with a PR number, or /code-review-remediation with a PR URL.
---

# Code Review Remediation

## Overview

Use this skill to take a pull request from reviewer feedback to a validated remediation commit. Validate reviewer intent before changing code, keep the changes tightly scoped, run the right regression checks, resolve merge conflicts when needed, and leave a clear final report.

## Workflow

### 1. Identify the Pull Request

If the user supplies a PR number or URL, use it. Otherwise:

1. Detect the current branch.
2. Find the associated pull request with the available GitHub tooling.
3. Stop and report clearly if no pull request can be found.

Collect the PR metadata, base branch, head branch, repository, and current CI or review status.

### 2. Prepare the Branch

Before editing:

1. Inspect `git status --short`.
2. Preserve unrelated user changes; do not overwrite them.
3. Fetch the latest remote state.
4. Check out the PR branch and pull its latest changes.
5. Fetch the base branch so conflicts and CI failures are evaluated against current code.

If the working tree is dirty with unrelated changes, work around them when safe. Ask before taking any action that could disturb user work.

### 3. Gather Review Feedback

Retrieve unresolved review feedback, including:

- General review comments
- Inline review comments
- Requested changes
- Blocking conversations
- CI failures or failing checks tied to review feedback

Ignore resolved conversations and outdated comments unless the underlying issue is still present.

Organize the feedback by file, line or thread, reviewer intent, severity, and whether it requires code, tests, docs, or explanation only.

### 4. Analyze Before Implementing

For every review thread:

1. Classify the issue: bug, logic, business rule, security, performance, maintainability, naming, readability, architecture, refactor, test coverage, documentation, API contract, validation, error handling, concurrency, database, or UX.
2. Validate whether the reviewer suggestion is correct, partially correct, or incorrect.
3. Cross-check project conventions, architecture, tests, docs, and business rules.
4. Decide the smallest correct remediation.

Do not blindly apply suggestions. If a suggestion is invalid, do not implement it; document why in the final report or PR reply.

### 5. Plan the Remediation

Before writing code, prepare a concise implementation plan covering:

- Root cause per comment
- Files likely affected
- Risk and regression surface
- Required refactoring, if any
- Required tests or fixture updates
- Work order that minimizes conflicts

For simple review comments, keep the plan brief and proceed.

### 6. Implement Incrementally

Apply focused edits only:

- Preserve existing behavior unless the review requires a behavior change.
- Follow the existing architecture and local style.
- Avoid unrelated refactors, broad formatting churn, debug code, commented-out code, and leftover TODOs.
- Add or update tests whenever a review comment changes behavior, fixes a bug, or closes a coverage gap.
- Keep staging or commits logically grouped if the user asks you to commit.

### 7. Validate and Fix Regressions

Run the most relevant checks for the touched surface, then broaden if risk warrants it:

- Unit, feature, integration, and end-to-end tests as applicable
- Linting, formatting, type checks, static analysis, and security checks as available
- Targeted manual verification for user-facing behavior

If a check fails, determine whether the failure is from an existing flaky test, a missing fixture, an incorrect expectation, or a real regression. Fix the root cause and rerun the affected checks. Do not disable or bypass tests to pass validation.

### 8. Reconcile With the Base Branch

Fetch the latest base branch and merge or rebase according to the repository's normal workflow. If conflicts occur:

1. Resolve them carefully.
2. Preserve both the base branch changes and the reviewer-requested fixes.
3. Re-run affected tests after conflict resolution.

Use `git push --force-with-lease` only when a rewritten branch history truly requires it. Never use plain `git push --force`.

### 9. Final Verification

Before pushing or reporting completion, verify:

- All actionable review comments are addressed or intentionally rejected with justification.
- No merge markers remain.
- No unrelated user changes were included.
- Tests and checks are run or explicitly reported as not run.
- `git status --short` reflects only the intended final state.
- The branch is pushed if the user asked for push/remediation completion and approval requirements allow it.

## Final Report

Report the outcome with:

- Review summary: total comments, addressed comments, rejected suggestions with justification, and unresolved discussions.
- Code changes: modified files, tests added or updated, and refactoring performed.
- Validation results: commands run and pass/fail status.
- Regression report: regressions found, fixed, or still known.
- Merge status: conflicts encountered and how they were resolved.
- Push status: whether the branch was pushed and whether it is ready for re-review.

## Operating Rules

- Validate reviewer feedback against the codebase before implementing.
- Prefer the smallest correct fix.
- Preserve business logic unless the review intentionally changes it.
- Keep changes scoped to the PR feedback and any regressions found during validation.
- Respect user-owned working tree changes.
- Do not suppress failing tests, skip meaningful validation, or hide residual risk.
