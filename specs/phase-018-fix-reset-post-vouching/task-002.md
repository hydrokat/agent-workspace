# Task 002: Server Action — Improve Error Diagnostics in `approveSystemReset`

## Description
The current `approveSystemReset` catches RPC errors and maps them against a hardcoded `known` allowlist.
Any error not in the list becomes `"Failed to approve reset request."` — which is what Sentry captured.
This task improves observability and adds defensive error handling.

## Objectives
- [ ] Add structured logging to `approveSystemReset` that captures the full Postgres error (code, message,
      details, hint) for server-side diagnostics
- [ ] Add a more descriptive generic fallback that hints at a possible schema issue (e.g.,
      `"Failed to approve reset request. An internal error occurred — please contact an administrator."`)
- [ ] Use `actionError` consistently (the `decodeId` path already uses it; the RPC error path should too)
- [ ] Capture the Sentry context (Supabase error code, RPC name) in the `console.error` for future triage

## Files
- [`admin-reset.ts`](file:///Users/hydrokat/Projects/hackthenorth/task-tracker/task-tracker-web/app-src/lib/actions/admin-reset.ts#L103-L138)

## Agent Assignment
- **Primary Agent**: senior-backend-dev
- **Supporting Agents**: None
- **Skills**: None

## Knowledgebase References
- [knowledgebase/best-practices/admin-reset.md](file:///Users/hydrokat/Projects/hackthenorth/task-tracker/agent-workspace/knowledgebase/best-practices/admin-reset.md) — Server Action error flow patterns.

## Progress
- [x] Read spec and task context
- [x] Assigned appropriate agent and skills
- [x] Implemented changes
- [ ] Updated knowledgebase
- [x] Validated changes

## Knowledgebase Updates
- [ ] [knowledgebase/best-practices/admin-reset.md](file:///Users/hydrokat/Projects/hackthenorth/task-tracker/agent-workspace/knowledgebase/best-practices/admin-reset.md) — Update error fallback logging practices.

## Status
- Done
