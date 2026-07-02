---
name: pull-request
description: Use when the user asks to create, prepare, submit, or open a GitHub pull request. Guides repository state checks, target branch selection, feature branch creation, atomic commits, validation, PR description writing, screenshots or diagrams when useful, and safe handling of unrelated changes.
---

# Pull Request

Use this skill to turn completed implementation work into a high-quality pull request that is easy to review, easy to revert, and clear about why the change exists.

## Workflow

1. Inspect repository state with `git status --short`, `git branch --show-current`, remotes, and targeted diffs.
2. Determine the target branch, preferring `main`, then `master`, then `trunk`, then the repository default branch.
3. Protect unrelated work. If unrelated changes are present, do not include them. Ask whether to leave them alone, commit them separately, stash them, or split exact paths or hunks.
4. Do not commit directly to `main`, `master`, or `trunk`. Create or switch to a descriptive branch using `<type>/<short-description>`, where type is `feature`, `bugfix`, `hotfix`, `refactor`, `chore`, `docs`, or `test`.
5. Review all modifications and identify business requirements, architectural impact, API changes, database changes, migrations, breaking changes, backward compatibility, deployment notes, and rollback needs.
6. Create atomic commits when committing is part of the request. Keep one logical change per commit and prefer exact path or hunk staging over `git add .` when multiple contexts exist.
7. Run appropriate validation before opening the PR: formatter, linter, type checks, static analysis, unit tests, feature tests, integration tests, security checks, or project-specific checks.
8. Fix validation failures before creating the PR unless the user explicitly instructs otherwise. If a failure cannot be fixed in scope, document it clearly in the PR and final response.
9. Push the branch and create the PR with the project's preferred tool, usually `gh pr create`, when credentials and network access are available. If PR creation is not possible, prepare the exact title and body for the user.

## Branch And Commit Guidance

Use branch names like:

```text
feature/user-import
bugfix/inventory-sync
hotfix/login-timeout
refactor/payment-allocation
chore/update-dependencies
```

Use conventional, reviewable commit subjects when the repository has no stronger local convention:

```text
feat(auth): add OAuth callback endpoint
refactor(auth): extract token validation service
test(auth): add callback integration tests
docs(auth): update authentication flow
```

Ensure each commit builds successfully when practical. Split formatting-only work from behavioral changes unless formatting is required for that exact change.

## PR Title

Write a concise title that explains the user-facing or maintainer-facing outcome:

```text
feat: add inventory synchronization
fix: prevent duplicate payment processing
refactor: simplify booking allocation workflow
```

## PR Description

Use this structure, omitting optional sections that do not apply:

```markdown
## Summary

Briefly describe the purpose of the pull request.

## Issue

Describe the existing problem, business impact, why the work is necessary, and any related issues.

Fixes #
Relates to #

## Changes Made

- ...

## Thinking Process

Explain why the implementation is shaped this way, including alternatives considered, trade-offs, performance implications, and maintainability concerns.

## Business Logic

Describe validation rules, workflows, edge cases, assumptions, authorization behavior, error handling, fallback behavior, and changes to existing flows.

## Testing Plan

Automated:
- ...

Manual:
1. ...

Expected results:
- ...

Regression areas:
- ...

## Revert Plan

Explain how to safely revert, including migration rollback, feature flags, data migration requirements, operational impact, and deployment order.

## Architecture Diagram

Include Mermaid, PlantUML, sequence diagrams, flowcharts, or ER diagrams when they clarify the review.

## Screenshots / Video

Attach compressed before/after screenshots or a short video when UI changed. Keep total media size practical and preferably under 10 MB.

## Performance Notes

Include benchmarks, expected latency changes, memory usage, or database query impact when relevant.

## Security Notes

Include authorization, authentication, input validation, encryption, secrets, and rate limiting impacts when relevant.

## Deployment Notes

Include migrations, feature flags, environment variables, infrastructure changes, deployment order, and backward compatibility when relevant.

## Other Remarks

Document known limitations, follow-up work, or reviewer guidance.
```

## Final Review Checklist

Before submitting, verify:

- Feature branch exists and protected branches were not committed to directly.
- Unrelated changes are excluded.
- Commits are atomic, logically grouped, and conventionally named.
- Build, formatting, linting, static analysis, and tests have been run or explicitly documented as not run.
- PR title is clear.
- Issue, changes, reasoning, business logic, testing, revert, breaking changes, and deployment notes are covered as applicable.
- Diagrams, screenshots, or video are included when they materially improve reviewability.
- Reviewers can understand the implementation without reading every line of code.

## Final Response

Report the PR link if created. Otherwise report the prepared title and body location or text. Always include branch name, target branch, commits created, validation performed, and any residual risks or checks not run.
