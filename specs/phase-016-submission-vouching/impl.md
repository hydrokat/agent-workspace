# Implementation Plan: Submission Vouching & Task Categories with Point Capping

## Phase Objectives

Two related capabilities that extend the contribution/approval loop:

1. **Submission Vouching** — Let a **team lead** validate ("vouch for") a proof of submission
   for their team's tasks, awarding the points exactly as an admin approval does. A team lead
   **may not vouch for their own submission** (an admin must act on those). Admins retain full
   authority to validate **all** submissions.

2. **Task Categories + Per-Category Point Capping** — Every task belongs to an admin-managed
   **category**. Admins can configure, per category, a **cap on accumulated point rewards per
   user** (0 = unlimited) and a **reset cadence** (per day / week / month), plus a **manual
   reset**. Once a user reaches a category's cap, they cannot claim further tasks in that
   category until the cap resets. Goal: stop users from farming one category repeatedly.

## Existing Implementation (ground truth)

These are the real components every task below must build on (verified against the codebase):

- **Roles & authorization** — `app-src/lib/auth/guards.ts`: `getAuthenticatedUser()`,
  `isAdmin()`, `isTeamLead(teamId)` (Admin OR `teams.team_lead_id === user.id`). Re-exported
  via `lib/actions/teams.ts`. SQL helper `is_admin()` used in RLS.
- **Approval flow** — `lib/actions/submissions.ts:reviewTaskSubmission(submission_id, status, feedback)`;
  approve path calls RPC `approve_task_submission(p_submission_id BIGINT, p_admin_id UUID)`
  (`SECURITY DEFINER`, `supabase/migrations/20260502120200_update_approve_submission_rpc.sql`):
  resolves role multiplier from `app_configurations.multipliers`, inserts a `point_transactions`
  row (`source = 'task'`); the `sync_profile_points()` trigger updates `profiles.points`.
  UI: `app/admin/submissions/page.tsx` + `submissions-table.tsx`.
- **Tasks & claiming** — `tasks` table (`BIGSERIAL` id, `team_id`, `base_points`, role limits;
  **no category column yet**). `lib/actions/tasks.ts` CRUD + admin UI
  `app/admin/tasks/task-management.tsx`. `lib/actions/task-participants.ts:claimTask(taskId, role)`
  is the gate where a user "takes" a task — the natural enforcement point for caps.
- **Configuration** — `app_configurations` (single `global_settings` row, JSONB columns,
  admin-only RLS via `is_admin()`), `lib/actions/config.ts`, admin UI `app/admin/config`.
- **Data model** — `task_participations` (BIGSERIAL, unique `(task_id, user_id)`),
  `task_submissions` (BIGSERIAL, unique `task_participation_id`, `status`, `admin_feedback`),
  `point_transactions` (BIGSERIAL, `computed_points`, `source`). `profiles.role` ∈
  {Admin, Lead, Member}; `profiles.points` synced by trigger.
- **Guidelines** — Server-side-only DB access (CLAUDE.md), `actionError` normalization
  (`knowledgebase/best-practices/error-normalization.md`), hashid integer-only invariant
  (phase-015 task-004), strict typing, RLS on every new table.

## Architecture & Key Decisions

### Submission Vouching
- Generalize `reviewTaskSubmission` authorization from admin-only to **admin OR the team lead
  of the submission's task team**, with an explicit **self-vouch block**: a team lead cannot
  approve/reject a submission whose participation `user_id === self`.
- Reuse the existing `approve_task_submission` RPC but **rename/extend** its second parameter
  semantics from `p_admin_id` to a generic `p_reviewer_id` (the acting lead/admin), and add an
  authorization guard **inside** the `SECURITY DEFINER` function (defense in depth: it must not
  trust the caller). Record the reviewer so vouches are auditable.
- RLS: add team-lead UPDATE/SELECT policies on `task_submissions` scoped to their team, keeping
  existing admin-all policies.
- UI: surface pending submissions for a team lead's team(s) (extend the submissions view or add
  a lead-facing route), with self-submissions shown as "Requires admin review" (disabled).

### Task Categories
- New `task_categories` table (BIGSERIAL id, `name` unique, cap config columns). Admin CRUD.
- `tasks.category_id BIGINT NOT NULL REFERENCES task_categories(id)`. Requires a **default
  category + backfill migration** for existing rows, then the NOT NULL constraint.
- Task create/update (`TaskInputSchema` / forms) must **require** `category_id`.

### Per-Category Point Capping
- Cap config on `task_categories`: `point_cap INT NOT NULL DEFAULT 0` (0 = unlimited),
  `cap_reset_period VARCHAR CHECK (cap_reset_period IN ('day','week','month')) DEFAULT 'month'`.
- **Accumulated points are computed on the fly, at claim time, from awarded points only** (no
  drift-prone counter): sum `point_transactions.computed_points` for the user, joined
  participation→task→category, where `created_at >= window_start`.
  `window_start = max(period_start(now, reset_period), last_manual_reset)`.
  **Claimed-but-not-yet-approved participations do NOT count** — only actually-awarded points do.
  This prevents a user with pending claims from locking themselves out before those tasks are
  ever rewarded. The check is recomputed each time a task is taken.
- **Manual reset is GLOBAL**: `category_cap_resets(category_id, reset_at, reset_by)` — one row
  resets the category's window for **all users**. Effective `last_manual_reset` = latest row for
  the category. Admin action `resetCategoryCap(categoryId)` (no per-user variant).
- **Enforcement in `claimTask`**: if the task's category `point_cap > 0` and the user's
  accumulated (awarded) category points in the current window `>= point_cap`, block the claim
  with a friendly message indicating when/how the cap resets. (Use an `actionError`-friendly
  message, not a raw throw.)
- **User visibility (Task 006)**: users can see their own caps in their profile
  (`app/account`) — accumulated vs cap, reset cadence, and next reset — reusing the same
  accumulation helper as enforcement so displayed and enforced values never diverge.

## Timeline
- **Task 001**: Task categories — schema, RLS, `tasks.category_id`, backfill (Complete)
- **Task 002**: Admin category management + require category on tasks (Complete)
- **Task 003**: Per-category point-cap configuration (schema + admin config) (Complete)
- **Task 004**: Cap enforcement on claim + accumulation logic + manual reset (Complete)
- **Task 005**: Submission vouching — team-lead validation with self-vouch block (Complete)
- **Task 006**: User-facing category cap visibility in profile (Complete)
- **Task 007**: "My Tasks" tab in the task list (Complete)
- **Task 008**: Knowledgebase update for both features (Complete)

## Tasks
- [x] Task 001: Task categories data model & migration
- [x] Task 002: Admin category management & category-required tasks
- [x] Task 003: Per-category point-cap configuration
- [x] Task 004: Category cap enforcement, accumulation & manual reset
- [x] Task 005: Submission vouching (team-lead validation)
- [x] Task 006: User-facing category cap visibility in profile
- [x] Task 007: "My Tasks" tab in the task list
- [x] Task 008: Knowledgebase update

## Cross-Cutting Requirements (apply to every task)
- **Existing implementation & guidelines:** each task documents the exact files/migrations it
  touches and the conventions it follows (server-side-only data access, `actionError`, RLS,
  hashid integer-only).
- **Unit tests:** each task ships Jest tests (run via Docker — see project memory:
  `docker compose ... run --rm npx jest`, not raw `npx jest`).
- **Security audit:** each task includes a security-audit checklist (authz, RLS, RPC
  `SECURITY DEFINER` guards, IDOR via hashids, no DB-internal leakage). Recommend running the
  `security-auditor` agent against the diff before marking complete.
