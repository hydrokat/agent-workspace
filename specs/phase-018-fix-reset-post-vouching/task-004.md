# Task 004: Knowledgebase — Update Scope Contract + Document FK-Topology Gotcha

## Description
The `knowledgebase/best-practices/admin-reset.md` scope contract table is missing the new phase 016 tables.
Additionally, document the FK-topology gotcha so future schema authors know that adding FK constraints
to tables in the TRUNCATE list requires updating `reset_system_state()`.

## Objectives
- [ ] Add `task_categories` to the scope contract table as **Preserved** (catalog data)
- [ ] Add `category_cap_resets` to the scope contract table as **Wiped** (transactional state)
- [ ] Add `task_submissions.reviewed_by` / `reviewed_at` context (wiped with submissions, no special handling)
- [ ] Add a new section: "Schema Change Checklist" documenting:
  - Any new table with transactional state must be added to the TRUNCATE list
  - Any FK pointing from a truncated table to a preserved table must use `ON DELETE SET NULL` or be
    handled explicitly — `ON DELETE RESTRICT` will block TRUNCATE CASCADE
  - Test the reset after any schema change that touches tables in the TRUNCATE list
- [ ] Cross-reference this phase as a case study

## Files
- [`admin-reset.md`](file:///Users/hydrokat/Projects/hackthenorth/task-tracker/agent-workspace/knowledgebase/best-practices/admin-reset.md)

## Agent Assignment
- **Primary Agent**: technical-writer
- **Supporting Agents**: business-analyst
- **Skills**: None

## Knowledgebase References
- [knowledgebase/best-practices/admin-reset.md](file:///Users/hydrokat/Projects/hackthenorth/task-tracker/agent-workspace/knowledgebase/best-practices/admin-reset.md) — Document we are editing.

## Progress
- [x] Read spec and task context
- [x] Assigned appropriate agent and skills
- [x] Implemented changes
- [x] Updated knowledgebase
- [x] Validated changes

## Knowledgebase Updates
- [x] [knowledgebase/best-practices/admin-reset.md](file:///Users/hydrokat/Projects/hackthenorth/task-tracker/agent-workspace/knowledgebase/best-practices/admin-reset.md) — Complete scope contract and schema change checklist.

## Status
- Done
