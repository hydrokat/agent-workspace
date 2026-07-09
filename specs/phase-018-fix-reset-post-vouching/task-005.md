# Task 005: Tests — Regression Tests for Reset with Category FK + Schema Drift Detection

## Description
Add regression tests to ensure the system reset works correctly with the full post-phase-016 schema,
and add a schema drift detection mechanism to catch future TRUNCATE-list mismatches.

## Objectives
- [ ] Add a test case to `admin-reset.test.ts` that verifies `approveSystemReset` succeeds when tasks
      have category assignments (the 2nd-approval path triggers `reset_system_state()`)
- [ ] Add a test case verifying `category_cap_resets` rows are wiped after reset
- [ ] Add a test case verifying `task_categories` rows are preserved after reset
- [ ] (Stretch) Add a schema introspection test that queries `information_schema.tables` for all
      user-created tables and asserts they are either in the TRUNCATE list or in an explicit "preserved"
      list — any new table not in either list fails the test, forcing a conscious decision

## Files
- [`admin-reset.test.ts`](file:///Users/hydrokat/Projects/hackthenorth/task-tracker/task-tracker-web/app-src/lib/actions/__tests__/admin-reset.test.ts)

## Agent Assignment
- **Primary Agent**: qa-engineer
- **Supporting Agents**: None
- **Skills**: None

## Knowledgebase References
- [knowledgebase/best-practices/admin-reset.md](file:///Users/hydrokat/Projects/hackthenorth/task-tracker/agent-workspace/knowledgebase/best-practices/admin-reset.md) — Scope contract for test references.

## Progress
- [x] Read spec and task context
- [x] Assigned appropriate agent and skills
- [x] Implemented changes
- [ ] Updated knowledgebase
- [x] Validated changes

## Knowledgebase Updates
- [ ] [knowledgebase/best-practices/admin-reset.md](file:///Users/hydrokat/Projects/hackthenorth/task-tracker/agent-workspace/knowledgebase/best-practices/admin-reset.md) — None (covered by Task 004).

## Status
- Done
