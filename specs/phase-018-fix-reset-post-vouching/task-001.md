# Task 001: Migration — Update `reset_system_state()` RPC

## Description
Create a new Supabase migration that replaces `reset_system_state()` to account for the `tasks.category_id → task_categories(id) ON DELETE RESTRICT` FK introduced in phase 016, and adds `category_cap_resets` to the TRUNCATE list.

## Objectives
- [ ] Create migration `20260709000000_fix_reset_system_state_category_fk.sql`
- [ ] Add `category_cap_resets` to the TRUNCATE list (transactional state)
- [ ] Ensure `task_categories` is **not** truncated (catalog data, like `rewards`)
- [ ] Handle the `ON DELETE RESTRICT` FK on `tasks.category_id` — either by reordering the TRUNCATE
      (truncating the referencing table is valid even with RESTRICT), or by explicitly handling the
      constraint if Postgres's `CASCADE` mode conflicts
- [ ] Verify the migration applies cleanly on the current schema
- [ ] Test that `reset_system_state()` succeeds with tasks referencing categories

## Technical Notes

The TRUNCATE statement should become:
```sql
TRUNCATE TABLE
  public.point_transactions,
  public.reward_claims,
  public.task_submissions,
  public.task_participations,
  public.tasks,
  public.team_members,
  public.teams,
  public.category_cap_resets
  RESTART IDENTITY CASCADE;
```

Key consideration: `tasks.category_id` references `task_categories(id)` with `ON DELETE RESTRICT`.
When we `TRUNCATE tasks`, Postgres should allow it because we're truncating the *referencing* table
(child side), not the *referenced* table (parent side). The `RESTRICT` constraint only prevents
deletion/truncation of the parent when children exist. However, `CASCADE` in `TRUNCATE ... CASCADE`
tells Postgres to also truncate any tables that have FK references *to* the listed tables. Since
`task_categories.id` is referenced *by* `tasks.category_id`, `CASCADE` would try to include
`task_categories` in the truncation set — but `task_categories` is not in our list and has `RESTRICT`.

If this causes a conflict, the fix is to remove `CASCADE` from the TRUNCATE and instead rely on
explicit ordering (truncate child tables before parent tables), or to list tables in dependency order
without `CASCADE`.

## Agent Assignment
- **Primary Agent**: senior-backend-dev
- **Supporting Agents**: tech-lead-orchestrator
- **Skills**: None

## Knowledgebase References
- [knowledgebase/best-practices/admin-reset.md](file:///Users/hydrokat/Projects/hackthenorth/task-tracker/agent-workspace/knowledgebase/best-practices/admin-reset.md) — Governed system state reset architecture and TRUNCATE gotchas.

## Progress
- [x] Read spec and task context
- [x] Assigned appropriate agent and skills
- [x] Implemented changes
- [ ] Updated knowledgebase
- [x] Validated changes

## Knowledgebase Updates
- [ ] [knowledgebase/best-practices/admin-reset.md](file:///Users/hydrokat/Projects/hackthenorth/task-tracker/agent-workspace/knowledgebase/best-practices/admin-reset.md) — Add category_cap_resets to scope contract, document category FK-topology restrict gotcha.

## Status
- Done
