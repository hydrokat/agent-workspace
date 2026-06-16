# Task 002: Admin category management & category-required tasks

## Description
Give admins UI + server actions to create/manage task categories, and make `category_id`
**required** when creating or editing a task.

## Existing Implementation & Guidelines
- `lib/actions/tasks.ts` — `createTask`/`updateTask` + `TaskInputSchema` (zod). NOTE: phase-015
  task-002 switches these to `safeParse` with friendly messages; build on that, don't regress it.
- `app/admin/tasks/task-management.tsx` — task create/edit dialog (currently has title,
  description, team, base_points, role limits; **no category selector**).
- `app/admin/config` + `lib/actions/config.ts` — pattern for admin-only management screens.
- Guidelines: server-side-only DB access; `actionError` for DB errors; admin gate via
  `isAdmin()`.

## Approach
1. Server actions `lib/actions/task-categories.ts`: `createCategory`, `updateCategory`,
   `deleteCategory` (admin-gated, `safeParse` validation, `actionError` on DB errors,
   `revalidatePath`). `getCategories` for selectors.
2. Admin category management UI (new section under `app/admin/config` or a dedicated
   `app/admin/categories` route): list/create/edit/delete with friendly errors and the
   delete-RESTRICT case surfaced clearly.
3. Extend `TaskInputSchema` with `category_id` (required, friendly message) and update
   `createTask`/`updateTask` to persist it.
4. Add a **required** category `<Select>` to the task dialog in `task-management.tsx`; show the
   category column in the task list.

## Objectives
- [ ] Admin can create/edit/delete categories; in-use delete shows a friendly RESTRICT message
- [ ] `TaskInputSchema` requires `category_id`; task create/edit persists it
- [ ] Task dialog has a required category selector; list shows category
- [ ] Validation/server errors render inline (consistent with phase-015 task-002)

## Unit Testing
- [ ] Jest (Docker): create/update/delete category actions (admin vs non-admin); `createTask`
      without `category_id` returns the friendly validation message; with a valid category it
      inserts.

## Security Audit
- [ ] All category mutations admin-gated server-side (not just UI-hidden).
- [ ] No DB-internal leakage (`actionError`); inputs `safeParse`d.
- [ ] Category ids exposed to client follow integer-only hashid rule if encoded.
- [ ] Run `security-auditor` agent on the diff.

## Status
- [ ] Pending
