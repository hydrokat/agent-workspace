# Task 006: User-facing category cap visibility in profile

## Description
A user must be able to **view their caps in their profile** — for each capped category, how
many points they have accumulated toward the cap in the current window, the cap value, and when
it resets. This makes the claim-time blocking (Task 004) transparent instead of surprising.

## Existing Implementation & Guidelines
- `app/account/page.tsx` — the profile screen (server component; loads `getMyProfile()` from
  `lib/actions/profiles.ts`; composes `update-profile-form` + `change-password-form`). This is
  where the cap view belongs.
- `getCategoryAccumulatedPoints(userId, categoryId)` + window/`last_manual_reset` logic from
  **Task 004** — reuse it; do not duplicate the accumulation math.
- `task_categories` (`point_cap`, `cap_reset_period`) from Tasks 001/003.
- Guidelines: server-side-only data access (compute in a Server Component / Server Action, never
  client-side Supabase); strict typing; `actionError` for any action.

## Approach
1. Server action `getMyCategoryCaps()` (authenticated) returning, per category with
   `point_cap > 0`: `{ category name, accumulated, point_cap, cap_reset_period, window_resets_at }`.
   - Reuse Task 004's accumulation helper and window computation; derive the next reset instant
     (`period_start` of the next window, accounting for `last_manual_reset`).
   - Unlimited categories (`point_cap = 0`) may be omitted or shown as "Unlimited" (decide in UI).
2. Add a **"Category Caps"** section to `app/account/page.tsx` rendering each cap as
   accumulated / cap with a progress indicator (a `Progress` component already exists in the UI
   kit) and the reset cadence + next reset time. Show an at-cap state clearly.
3. Show only the current user's data (no admin/other-user info).

## Objectives
- [ ] `getMyCategoryCaps()` returns the current user's accumulated/cap/reset info per capped category
- [ ] Account page shows a Category Caps section (accumulated vs cap, cadence, next reset)
- [ ] At-cap categories are clearly indicated; unlimited handled per UI decision
- [ ] Numbers match the claim-time enforcement (same helper as Task 004 — no divergence)

## Unit Testing
- [ ] Jest (Docker): `getMyCategoryCaps` returns correct accumulated/cap/reset per category;
      reflects window rollover and global manual reset; only the caller's data is returned;
      consistent with `claimTask`'s computed accumulation for the same fixtures.

## Security Audit
- [ ] Data scoped to the authenticated user only (no IDOR; cannot request another user's caps).
- [ ] Computed server-side; no client-side DB access.
- [ ] No DB-internal leakage; `actionError` on failure.
- [ ] Run `security-auditor` agent on the diff.

## Status
- [ ] Pending
