# Best Practice: Category Point Capping

Introduced in phase-016. Documents the design for per-category point caps to prevent users from farming one task category repeatedly.

## Schema

`task_categories` table:
- `id BIGSERIAL` — hashid-encoded for client exposure
- `name VARCHAR(100) UNIQUE NOT NULL`
- `point_cap INT NOT NULL DEFAULT 0` — `0` means unlimited
- `cap_reset_period VARCHAR(10) CHECK IN ('day','week','month') DEFAULT 'month'`

`category_cap_resets` table:
- `id BIGSERIAL`
- `category_id BIGINT REFERENCES task_categories(id) ON DELETE CASCADE`
- `reset_by UUID REFERENCES profiles(id)`
- `reset_at TIMESTAMPTZ DEFAULT now()`

`tasks.category_id BIGINT NOT NULL REFERENCES task_categories(id) ON DELETE RESTRICT` — every task must belong to exactly one category.

## Accumulation Logic

Cap is computed **at claim time** from awarded points only:

```
window_start = max(period_start(now, cap_reset_period), last_manual_reset_at_for_category)
accumulated  = SUM(point_transactions.computed_points)
               WHERE user_id = claimant
               AND   point_transactions.created_at >= window_start
               AND   tasks.category_id = target_category_id
               (joined via: point_transactions → task_participations → tasks)
```

Key properties:
- **Only awarded points count.** Pending/unapproved claims have no `point_transactions` row yet, so they never inflate the cap prematurely.
- **No stored counter.** The sum is recomputed fresh on each `claimTask` call. Avoids drift.
- **Cap check runs at claim time.** A user cannot lock other tasks by holding pending claims.

Implementation: `getCategoryAccumulatedPoints(userId, categoryId, period, lastManualReset)` in `lib/actions/task-categories.ts`.

## Window Start Computation

`getWindowStart(period, now)` returns a UTC midnight date:
- `'day'` → today at `00:00 UTC`
- `'week'` → ISO Monday of the current week at `00:00 UTC`
- `'month'` → first of the current month at `00:00 UTC`

`window_start = max(getWindowStart(period, now), last_manual_reset)` — a manual reset after the natural period start tightens the window.

## Manual Reset (Global)

Inserting a row into `category_cap_resets` is a **global reset** — it resets the window for **all users** in that category. There is no per-user variant. Only admins may insert.

`resetCategoryCap(hashedCategoryId)` in `lib/actions/task-categories.ts`.

## Enforcement in claimTask

`lib/actions/task-participants.ts:claimTask` step 3 (after duplicate check, before capacity check):

1. Fetch `category` from the task join (`id, point_cap, cap_reset_period`).
2. If `point_cap === 0`, skip enforcement entirely (unlimited).
3. Fetch latest `category_cap_resets` row → `lastManualReset`.
4. Call `getCategoryAccumulatedPoints(user.id, category.id, period, lastManualReset)`.
5. If `accumulated >= point_cap`, throw with period-labeled message ("daily/weekly/monthly point cap").

## User Visibility

Users can see their cap status per category at `app/account` (profile page):
- `getMyCategoryCaps()` returns `CategoryCapStatus[]` with `accumulated`, `point_cap`, `cap_reset_period`, `window_resets_at`.
- The same `getCategoryAccumulatedPoints` helper is used, so displayed and enforced values always match.
- Component: `app/account/category-caps-section.tsx` — progress bar per category, hidden entirely when `caps.length === 0`.

## Security Checklist
- Admin-only for create/update/delete category and reset cap (`isAdmin()` guard in server action).
- Authenticated-read for `getCategories()` and `getMyCategoryCaps()`.
- hashid-decode for all client-supplied category IDs (`decodeId('category', ...)`).
- RLS on `task_categories`: authenticated SELECT; admin ALL.
- RLS on `category_cap_resets`: authenticated SELECT; admin INSERT only.
- FK `ON DELETE RESTRICT` on `tasks.category_id` prevents deleting a category with tasks; surfaces as a friendly "Reassign those tasks first" message.
