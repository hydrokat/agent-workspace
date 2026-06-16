# Task 004: Category cap enforcement, accumulation & manual reset

## Description
Enforce per-category point caps: once a user's **accumulated points in a category** reach the
configured cap within the current reset window, they cannot **claim** another task in that
category until the cap resets. Provide admin **manual reset**. Goal: prevent farming one
category repeatedly.

## Existing Implementation & Guidelines
- `lib/actions/task-participants.ts:claimTask(taskId, role)` — the gate where a user "takes" a
  task (validates membership, capacity, inserts participation). Cap check belongs **here**,
  before the participation insert.
- `point_transactions` (`computed_points`, `created_at`, `task_participation_id`) →
  `task_participations` (`task_id`, `user_id`) → `tasks` (`category_id`) is the join path for
  accumulated category points.
- `approve_task_submission` RPC writes the `point_transactions` rows that count toward the cap.
- Guidelines: server-side-only; `actionError`-friendly messages (no raw throws);
  `is_admin()` gate for the reset.

## Approach
1. **Accumulation = awarded points only, evaluated at claim time (compute on the fly — no
   counter table to drift):** helper `getCategoryAccumulatedPoints(userId, categoryId)` summing
   `point_transactions.computed_points` over the join path where `created_at >= window_start`.
   - **Only actually-awarded points count.** Claimed-but-not-yet-approved participations do
     **not** count toward the cap. This is deliberate: a user with several pending claims must
     not be locked out before those tasks are ever rewarded ("prevent user from locking other
     tasks and not be able to be rewarded"). The cap is recomputed fresh **each time a task is
     taken**, against the user's currently-awarded category points.
   - `window_start = max(period_start(now, cap_reset_period), last_manual_reset)`.
   - `period_start`: start of current day/week/month (define week boundary explicitly, e.g.
     ISO Monday 00:00 UTC; document the timezone choice).
2. **Manual reset table (GLOBAL only):** migration `category_cap_resets(id BIGSERIAL,
   category_id BIGINT NOT NULL REF task_categories, reset_at TIMESTAMPTZ DEFAULT now(),
   reset_by UUID REF profiles)`. A reset applies to **all users** of that category.
   `last_manual_reset` for a category = the latest `reset_at` row for that category (same value
   for every user). RLS: admin-only writes; authenticated read.
3. **Admin reset action** `resetCategoryCap(categoryId)` (admin-gated, no per-user param)
   inserting one global reset row; UI control on the category screen.
4. **Enforcement in `claimTask`:** resolve the task's category; if `point_cap > 0` and
   accumulated awarded points `>= point_cap`, block with a friendly message naming the reset
   cadence (e.g. "Category cap reached. Resets monthly."). Otherwise proceed.
   - Concurrency note: the on-the-fly sum may race with simultaneous approvals; acceptable since
     the cap gates *claiming* against already-awarded points (pending claims excluded by design).

## Objectives
- [ ] `getCategoryAccumulatedPoints` counts only awarded `point_transactions`, correct across
      day/week/month windows + global manual reset
- [ ] Pending/unapproved claims do NOT count toward the cap (verified)
- [ ] `category_cap_resets` table (global) + RLS (admin write, authenticated read) + admin
      `resetCategoryCap(categoryId)`
- [ ] `claimTask` blocks claims when cap reached; friendly message with reset cadence
- [ ] `point_cap = 0` (unlimited) never blocks
- [ ] Admin global manual reset immediately re-permits claiming for all users

## Unit Testing
- [ ] Jest (Docker): under-cap allows claim; at/over-cap blocks; unlimited never blocks;
      pending (unapproved) claims do not count toward the cap; window rollover (day/week/month)
      resets accumulation; global manual reset re-permits all users; period-boundary math correct.

## Security Audit
- [ ] Enforcement is server-side in `claimTask` (cannot be bypassed by client).
- [ ] `resetCategoryCap` admin-gated server-side; RLS prevents non-admin reset writes.
- [ ] No IDOR: users can't read/influence others' accumulation.
- [ ] Friendly blocking message via `actionError` style; no DB internals leaked.
- [ ] Run `security-auditor` agent on the diff.

## Status
- [ ] Pending
