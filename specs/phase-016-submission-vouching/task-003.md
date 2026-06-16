# Task 003: Per-category point-cap configuration

## Description
Let admins configure, per category, a **cap on accumulated point rewards per user** (0 =
unlimited) and the **reset cadence** (per day / week / month). This task covers the schema and
admin configuration surface; enforcement and manual reset are Task 004.

## Existing Implementation & Guidelines
- `task_categories` table (Task 001) — cap columns are added here.
- `app_configurations` + `lib/actions/config.ts` + `app/admin/config` — the established pattern
  for admin-tunable settings and admin-only RLS (`is_admin()`).
- `lib/actions/task-categories.ts` (Task 002) — extend its update action to carry cap fields.
- Guidelines: server-side-only writes; `safeParse`; `actionError`.

## Approach
1. Migration adding cap config to `task_categories`:
   - `point_cap INT NOT NULL DEFAULT 0 CHECK (point_cap >= 0)` (0 = unlimited).
   - `cap_reset_period VARCHAR(10) NOT NULL DEFAULT 'month'
     CHECK (cap_reset_period IN ('day','week','month'))`.
2. Extend category schema + `createCategory`/`updateCategory` validation to accept and persist
   `point_cap` and `cap_reset_period` (friendly messages; non-negative cap; enum period).
3. Admin UI: add cap amount + reset-period controls to the category management screen, with
   clear copy ("0 = unlimited").

## Objectives
- [ ] `task_categories.point_cap` and `cap_reset_period` columns added with CHECK constraints
- [ ] Category create/update validates and persists cap config (0 = unlimited; valid period)
- [ ] Admin UI exposes cap + reset-period with explanatory copy

## Unit Testing
- [ ] Jest (Docker): negative cap rejected; invalid period rejected; valid values persist;
      defaults (`0` / `'month'`) applied when omitted.

## Security Audit
- [ ] Cap config writable only by admins (RLS + action gate).
- [ ] CHECK constraints enforce domain (no negative caps, valid period) at the DB layer.
- [ ] `safeParse` + `actionError`; no leakage.
- [ ] Run `security-auditor` agent on the diff.

## Status
- [ ] Pending
