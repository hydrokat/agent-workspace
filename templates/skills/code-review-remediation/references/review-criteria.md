# Review Criteria

Rubric for the review pass. Review as a Staff Engineer, Software Architect, Security Engineer, and Performance Engineer. Passing tests are a signal, not proof of correctness.

## Stance

- Prioritize production safety, user impact, data integrity, authorization, rollback, and maintainability.
- Lead with concrete findings. Cite file and line for every actionable finding.
- Do not approve unresolved Critical or High findings.
- Require tests for new business logic, regressions, permissions, validations, and error paths.

## Checklist

1. **Context**: PR title, description, linked issues, commits, changed files, base branch, CI status, existing review comments, project docs and knowledgebase entries.
2. **Intent**: business problem, intended outcome, affected flows and services, data model and API changes, backward-compatibility expectations.
3. **Change inventory**: categorize changed files by area (API, Database, Frontend, Tests, Infra) and risk.
4. **Correctness**: business rules, state transitions, validation, authorization, transactions, rollback, concurrency, idempotency, failure handling, edge cases (null/empty, time boundaries, pagination, retries, partial failures, duplicates, stale data), backward compatibility.
5. **Architecture and maintainability**: layering, module boundaries, coupling, duplication, naming, dead code, magic values, consistency with local patterns, unnecessary comments (prefer self-documenting code; keep doc-generator comments such as JSDoc/PHPDoc/OpenAPI).
6. **Blast radius**: APIs, UI, workers, queues, schedulers, caches, auth, notifications, reporting, integrations, migrations, existing records, infrastructure load. Rate Minimal / Low / Medium / High / Critical.
7. **Performance**: N+1 queries, missing indexes, locks, algorithmic complexity, memory, network calls, rendering cost, bundle size, queue growth. Rate Improves / Neutral / Slight / Moderate / Severe regression.
8. **Database, API, Security, Deployment**:
   - Database: migration safety, rollback, indexes, constraints, nullable columns, defaults, backfills, execution order.
   - API: schemas, authn/authz, status codes, pagination, errors, versioning, compatibility.
   - Security: OWASP Top 10, injection, mass assignment, IDOR, secrets, XSS, CSRF, SSRF, unsafe uploads, rate limiting, sensitive logging, privilege escalation, cryptography.
   - Deployment: feature flags, env vars, cache invalidation, observability, runbooks.
9. **Tests**: map changed behavior to coverage (happy path, failure path, edge cases, permissions, validation, concurrency, regression). Name the missing tests specifically.

## Finding Format

Each finding includes: severity (Critical, High, Medium, Low), category (Business Logic, Architecture, Security, Performance, Database, API, Testing, Deployment, Maintainability), file and line, description, user/business impact, technical impact, recommendation.

## Verdicts

- `CHANGES REQUESTED`: any unresolved Critical or High finding, or missing information prevents a safe approval.
- `APPROVED`: safe, correct, maintainable, secure, performant, adequately tested, and deployable.
- `COMMENT ONLY`: non-blocking feedback only.

The loop in `SKILL.md` is clear only on `APPROVED`, or on `COMMENT ONLY` where every remaining item is Low severity and has been addressed or explicitly deferred with justification.
