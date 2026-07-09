# Implementation Plan: Phase 018 — Fix System Reset Post-Vouching Schema Changes

## Sentry Reference
- **Issue**: [6dadd1a54a514254a8e0a8618f4e3bf5](https://hackthenorthph.sentry.io/share/issue/6dadd1a54a514254a8e0a8618f4e3bf5/)
- **Error**: `Failed to approve reset request.` — generic fallback in `approveSystemReset`
- **Route**: `POST /admin/danger-zone`
- **Source**: `lib/actions/admin-reset.ts → approveSystemReset`

## Root Cause Analysis

Phase 016 (Submission Vouching) introduced five migrations (`20260616000001`–`20260616000005`) that modified
the schema **after** the system reset feature (Phase 013) was completed. Two of these changes broke the
`reset_system_state()` RPC:

### Problem 1: `ON DELETE RESTRICT` FK blocks TRUNCATE

Migration `20260616000002_add_category_to_tasks.sql` added:
```sql
ALTER TABLE public.tasks
    ADD COLUMN category_id BIGINT NOT NULL
    REFERENCES public.task_categories(id) ON DELETE RESTRICT;
```

`reset_system_state()` runs `TRUNCATE TABLE ... public.tasks ... RESTART IDENTITY CASCADE`. The `CASCADE`
propagates truncation to tables that FK **into** the truncated tables, but `tasks.category_id` points
**outward** to `task_categories` with `ON DELETE RESTRICT`. Postgres raises:

> `cannot truncate a table referenced in a foreign key constraint` (or similar FK violation)

This error message is not in the `known` allowlist in `approveSystemReset` (line 120–126 of
`admin-reset.ts`), so it surfaces as the generic `"Failed to approve reset request."`.

### Problem 2: `category_cap_resets` not in TRUNCATE list

Migration `20260616000004` created `category_cap_resets` — a transactional state table (manual cap resets
per category). This table should be wiped on system reset (it records operational state, not configuration),
but it was never added to `reset_system_state()`.

### Problem 3: `task_submissions.reviewed_by` / `reviewed_at` columns

Migration `20260616000005` added `reviewed_by UUID REFERENCES profiles(id) ON DELETE SET NULL` and
`reviewed_at` to `task_submissions`. These are safe under TRUNCATE (the submissions table is already
truncated), but the Danger Zone UI's "DESTRUCTIVE_WIPE" list doesn't mention submission review audit data,
and the knowledgebase scope contract doesn't cover these columns.

## Phase Objectives

1. Update `reset_system_state()` to handle the new FK topology introduced by phase 016.
2. Add `category_cap_resets` to the TRUNCATE list (transactional state, should be wiped).
3. Preserve `task_categories` (catalog/config data, like `rewards`).
4. Update the Danger Zone UI to reflect the updated scope (mention category cap resets in wipe list;
   mention task categories in preserved list).
5. Update the knowledgebase scope contract and error-handling patterns.
6. Add regression tests to prevent future schema drift from silently breaking the reset.

## Architecture

### Fix Strategy

The `TRUNCATE ... CASCADE` in `reset_system_state()` needs to be aware that `tasks.category_id`
references `task_categories(id)` with `ON DELETE RESTRICT`. There are two viable approaches:

**Chosen approach: Reorder truncation with explicit FK handling**

1. Add `category_cap_resets` to the TRUNCATE list (it FKs into `task_categories` with `ON DELETE CASCADE`,
   so it must be truncated **before** or **alongside** `tasks` to avoid orphan issues).
2. The core issue is that `TRUNCATE tasks` with `CASCADE` does not cascade *to* `task_categories`
   (the FK points outward), but Postgres still validates the constraint. Since we are truncating `tasks`
   (not `task_categories`), and `tasks` is the *referencing* table, the RESTRICT constraint should not
   fire on truncation of the referencing side — **but** `RESTART IDENTITY CASCADE` can cause issues
   when Postgres tries to restart sequences across related tables.

   **Actual fix**: The `TRUNCATE ... RESTART IDENTITY CASCADE` statement truncates tables that reference
   the listed tables via FK. The `ON DELETE RESTRICT` on `tasks.category_id` means Postgres sees
   `task_categories` as referenced and refuses to cascade-truncate it (which is correct — we don't want
   to truncate categories). But the error occurs because Postgres validates that truncating `tasks` won't
   leave dangling references. Since `tasks` is the *child* side, truncating it is safe — the issue is
   likely that `CASCADE` tries to pull `task_categories` into the truncation set.

   **Resolution**: Explicitly list the truncation order and ensure `tasks` is truncated without
   cascading into `task_categories`. If needed, temporarily drop and re-add the FK constraint within
   the transaction, or simply list `category_cap_resets` alongside the existing tables and ensure the
   FK direction is respected.

### Migration Plan

A single new migration that:
1. `CREATE OR REPLACE FUNCTION public.reset_system_state()` with the updated TRUNCATE list including
   `category_cap_resets`.
2. Adjusts the TRUNCATE statement to handle the `tasks → task_categories` FK constraint safely.

### Server Action Changes

- **`admin-reset.ts`**: Add the Postgres FK-violation error message to the `known` allowlist as a
  defensive measure, with a user-friendly message. This ensures that even if future schema changes
  re-introduce the problem, the admin sees a meaningful error rather than a generic fallback.

### UI Changes

- **`danger-zone-client.tsx`**: Update the DESTRUCTIVE_WIPE list to include "ALL category point cap
  reset logs". Update the PRESERVED_ASSETS list to include "Task categories (category catalog)".

### Knowledgebase Updates

- **`admin-reset.md`**: Add `category_cap_resets` to the scope contract table. Add `task_categories`
  to the preserved list. Document the FK-topology gotcha for future schema authors.

## Target Application
- **Repo**: `task-tracker-web/app-src` (sibling of this workspace)
- **Stack**: Next.js (App Router), React 19, Tailwind v4, shadcn/ui, Supabase SSR + Auth

## Key Files
- [`reset_system_state()` RPC](file:///Users/hydrokat/Projects/hackthenorth/task-tracker/task-tracker-web/supabase/migrations/20260607141000_create_system_reset_tables_and_rpcs.sql#L44-L65)
- [`approveSystemReset` Server Action](file:///Users/hydrokat/Projects/hackthenorth/task-tracker/task-tracker-web/app-src/lib/actions/admin-reset.ts#L103-L138)
- [`danger-zone-client.tsx`](file:///Users/hydrokat/Projects/hackthenorth/task-tracker/task-tracker-web/app-src/components/admin/danger-zone-client.tsx)
- [`admin-reset.md` knowledgebase](file:///Users/hydrokat/Projects/hackthenorth/task-tracker/agent-workspace/knowledgebase/best-practices/admin-reset.md)
- [`add_category_to_tasks` migration (root cause)](file:///Users/hydrokat/Projects/hackthenorth/task-tracker/task-tracker-web/supabase/migrations/20260616000002_add_category_to_tasks.sql)
- [`create_category_cap_resets` migration](file:///Users/hydrokat/Projects/hackthenorth/task-tracker/task-tracker-web/supabase/migrations/20260616000004_create_category_cap_resets.sql)

## Timeline
- **Task 001**: Migration — update `reset_system_state()` RPC to handle new FK topology + add `category_cap_resets` (Done)
- **Task 002**: Server Action — improve error diagnostics in `approveSystemReset` (Done)
- **Task 003**: UI — update Danger Zone scope lists (Done)
- **Task 004**: Knowledgebase — update scope contract + document FK-topology gotcha (Done)
- **Task 005**: Tests — regression tests for reset with category FK + schema drift detection (Done)

## Tasks
- [x] Task 001: Migration — update `reset_system_state()` RPC to handle new FK topology + add `category_cap_resets`
- [x] Task 002: Server Action — improve error diagnostics in `approveSystemReset`
- [x] Task 003: UI — update Danger Zone scope lists
- [x] Task 004: Knowledgebase — update scope contract + document FK-topology gotcha
- [x] Task 005: Tests — regression tests for reset with category FK + schema drift detection

## Dependencies
- 001 blocks 002 (action error messages depend on RPC behavior).
- 001 blocks 005 (tests validate the fix).
- 003 and 004 are independent of each other but should follow 001.

## Acceptance Criteria
- `approveSystemReset` succeeds when the 2nd approval triggers `reset_system_state()` on a database with
  `task_categories`, `category_cap_resets`, and `tasks.category_id` FK in place.
- After reset: `task_categories` rows are preserved (catalog data); `category_cap_resets` rows are wiped
  (transactional state).
- The Danger Zone UI accurately reflects the updated scope (cap resets wiped, categories preserved).
- The knowledgebase scope contract is current and includes the FK-topology gotcha.
- Regression tests verify that the reset succeeds with the full post-phase-016 schema.
- Existing tests remain green.
