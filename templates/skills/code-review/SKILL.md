---
name: code-review
description: "Use when the user asks for a production-grade pull request or diff review, including /code-review. Reviews correctness, business flow, architecture, security, performance, database safety, API contracts, tests, deployment risk, blast radius, and merge readiness. Produces findings first with file references, severity, remediation, and an approval or changes-requested verdict."
---

# Code Review

Use this skill to review a pull request, branch diff, commit range, or local changes as a Staff Software Engineer, Software Architect, Security Engineer, and Performance Engineer.

## Review Stance

- Treat compiling code and passing tests as useful signals, not proof of correctness.
- Prioritize production safety, user impact, data integrity, authorization, rollback, and maintainability.
- Lead with concrete findings. Do not bury blockers behind praise or summary.
- Cite affected files and lines for every actionable finding.
- Do not approve unresolved Critical or High severity issues.
- Require tests for new business logic, regressions, permissions, validations, and error paths.

## Step 1: Gather Context

Retrieve or inspect the available review inputs:

- PR title, description, linked issues, commits, changed files, target branch, CI status, and existing review comments when reviewing a remote PR.
- `git status --short`, target branch, merge base, and diffs when reviewing local changes.
- Project documentation, architecture notes, coding standards, and relevant knowledgebase entries when present.

Stop and report the blocker if the PR or diff cannot be retrieved.

## Step 2: Understand Intent

Determine the business problem, intended outcome, affected user flows, affected services, changed modules, data model changes, API changes, and backward compatibility expectations.

Prepare a concise executive summary for the final review only after findings have been identified.

## Step 3: Build The Change Inventory

Categorize every changed file by area and risk.

Use a compact table when the change is non-trivial:

| Area | Files | Risk |
|---|---:|---|
| API | 0 | Low |
| Database | 0 | Low |
| Frontend | 0 | Low |
| Tests | 0 | Low |

## Step 4: Review Core Correctness

Validate:

- Business rules, state transitions, workflow ordering, validation, authorization, transactions, rollback behavior, concurrency, idempotency, and failure handling.
- Edge cases, null or empty states, time boundaries, pagination, retries, partial failures, duplicate requests, and stale data.
- Backward compatibility for public APIs, events, queues, database schemas, configuration, and user-visible behavior.

Flag skipped rules, incorrect flows, missing validations, race conditions, inconsistent states, and hidden side effects.

## Step 5: Review Architecture And Maintainability

Evaluate separation of concerns, layering, module boundaries, dependency direction, abstraction quality, reuse, duplication, naming, logging, error handling, readability, and consistency with local patterns.

Identify tight coupling, unnecessary complexity, dead code, unreachable paths, excessive nesting, magic values, and architectural violations.

## Step 6: Analyze Blast Radius

Determine what the change can affect beyond its apparent scope:

- Systems: APIs, UI, workers, queues, schedulers, background jobs, caches, authentication, authorization, notifications, reporting, analytics, and integrations.
- Data: migrations, indexes, constraints, foreign keys, existing records, and integrity guarantees.
- Infrastructure: Redis, database load, storage, queues, memory, CPU, and networking.

For broad changes, include:

| Component | Impact | Risk | Notes |
|---|---|---|---|

Rate overall blast radius as Minimal, Low, Medium, High, or Critical.

## Step 7: Review Performance

Check:

- Database: N+1 queries, missing indexes, expensive joins, eager loading, query count, locks, migrations, and production data volume.
- Backend: CPU, memory, serialization, allocation, network calls, algorithmic complexity, and queue growth.
- Frontend: rendering cost, bundle size, unnecessary rerenders, API chatter, layout shifts, and asset loading.
- Infrastructure: cache efficiency, storage growth, bandwidth, scheduler pressure, and worker throughput.

Classify performance impact as Improves performance, Neutral, Slight regression, Moderate regression, or Severe regression.

## Step 8: Review Database, API, Security, And Deployment

Database:
- Validate migration safety, rollback plan, indexes, constraints, locks, nullable columns, default values, backfills, data integrity, and production execution order.

API:
- Validate request and response schemas, authentication, authorization, status codes, pagination, errors, versioning, and backward compatibility.

Security:
- Review authentication, authorization, OWASP Top 10, injection, mass assignment, IDOR, secrets, XSS, CSRF, SSRF, unsafe file upload, SQL injection, rate limiting, sensitive logging, privilege escalation, and cryptography.
- Assign Critical, High, Medium, or Low severity to security findings.

Deployment:
- Assess feature flags, migrations, rollback, environment variables, queues, cache invalidation, compatibility, observability, and operational runbooks.

## Step 9: Review Tests

Map changed behavior to coverage. Verify happy paths, failure paths, edge cases, permissions, validation, exceptions, concurrency, and regression scenarios.

Recommend missing tests with enough specificity that an implementer can add them directly.

## Step 10: Use Diagrams When Helpful

For workflow or architecture changes, include a Mermaid flowchart or sequence diagram when it clarifies risk or behavior.

Use diagrams only when they add review value.

## Output Format

Use this order:

1. Findings, ordered by severity.
2. Open questions or assumptions.
3. Executive summary.
4. Change inventory, blast radius, performance impact, test assessment, and deployment risk when relevant.
5. Final verdict.

For each finding include:

- Severity: Critical, High, Medium, or Low.
- Category: Business Logic, Architecture, Security, Performance, Database, API, Testing, Deployment, or Maintainability.
- File and line reference.
- Description.
- Business or user impact.
- Technical impact.
- Recommendation.

If there are no findings, say that clearly and mention any remaining test gaps or residual risk.

## Verdict Rules

Use `CHANGES REQUESTED` when any unresolved Critical or High severity issue exists, or when missing information prevents a safe approval.

Use `APPROVED` only when the change appears safe, correct, maintainable, secure, performant, adequately tested, and deployable.

Use `COMMENT ONLY` for non-blocking feedback when approval authority is unclear or the user asked for review comments rather than a merge decision.
