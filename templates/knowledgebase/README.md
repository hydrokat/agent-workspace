# Knowledgebase

Single source of truth for project context, standards, and decisions. Every contributor
consults this directory before modifying code to ensure consistency.

## Directory Map

| Directory | Purpose |
|---|---|
| `guidelines/` | Enforced rules — env access, ID hashing, migrations, code limits, testing |
| `best-practices/` | Reusable patterns, lessons learned, coding conventions |
| `api-design/` | Endpoint conventions, request/response shapes, error formats, versioning |
| `db-schema/` | Entity relationships, migration policy, indexing strategy |
| `architecture-decisions/` | ADRs — why we built it this way, with date and context |
| `business-flows/` | Process flows, decision paths, operational context |
| `context-history/` | Historical decisions, codebase maps, task completion logs |

## Policy Summary

- **Env access:** Never read `process.env` directly. Use a config module that
  reads env and exposes typed getters.
- **ID hashing:** Hash auto-increment IDs on egress, decode on ingress.
  Use per-resource salts. Never persist hashed IDs. Prefer uuidv7 for
  shared resources.
- **Migrations:** Never modify a committed migration. Destructive schema
  changes must include a backfill or backwards-compatibility step.
- **Code limits:** 500 lines per file, 110 columns per line.
- **Coverage:** Minimum 85% unit test coverage.
- **Principles:** Clean architecture, KISS, DRY, no god services/controllers.
- **Environment:** During development, only local env is accessible. No agent
  accesses non-local env unless explicitly allowed.