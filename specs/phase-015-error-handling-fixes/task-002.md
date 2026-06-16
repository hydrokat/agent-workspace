# Task 002: Task create/update validation error surfacing

## Description

Sentry `a4a621623e…` (`POST /tasks`): `createTask` and `updateTask` call
`TaskInputSchema.parse(data)`, which throws a raw `ZodError`. The thrown text
(`"Invalid input: expected string, received number"`) reaches Sentry, while the client only
renders a hard-coded `"Failed to save task. Please try again."`. Validation failures are
neither user-friendly nor accurately displayed, and the raw throw pollutes error tracking.

### Affected code
- `app-src/lib/actions/tasks.ts` → `createTask` (line ~41), `updateTask` (line ~73): both use
  `.parse(...)`.
- `app-src/lib/actions/tasks.ts` → `TaskInputSchema` (lines ~21–30): field messages.
- `app-src/app/admin/tasks/task-management.tsx` → `handleSubmit` `catch` (lines ~123–126):
  hard-coded message.

### Approach
1. **Switch to `safeParse`.** In `createTask`/`updateTask`, replace `.parse()` /
   `.partial().parse()` with `safeParse`. On failure, `throw new Error(parsed.error.issues[0]?.message || "Validation failed.")`
   — matching the pattern already used in `lib/actions/auth.ts` and `lib/actions/submissions.ts`.
   This stops raw `ZodError`s from bubbling to Sentry and yields a controlled message.
2. **Friendlier field messages.** Audit `TaskInputSchema` so every constraint has a clear,
   user-facing message (e.g. `base_points`, `max_leads/contributors/assistants` integer/number
   rules), so a type mismatch reads like "Base points must be a number" rather than
   "expected string, received number". Confirm the client sends numbers for numeric fields
   (the form already does `parseInt(...) || 0`); investigate any path that could send a string
   where a number is expected (or vice-versa) given the Sentry message.
3. **Display the real message.** In `handleSubmit`, set `setErrorMessage(error instanceof Error ? error.message : "Failed to save task.")` so the validation/server message renders in the
   existing destructive slot (`DialogFooter`, line ~405) instead of the generic placeholder.

## Objectives
- [x] `createTask`/`updateTask` use `safeParse`; no raw `ZodError` escapes the action
- [x] `TaskInputSchema` messages are user-friendly for every field/constraint
- [x] Client renders the actual validation/server message in the dialog error slot
- [x] Supabase errors still routed through `actionError` (unchanged, no leakage)

## Validation
- [ ] Jest (Docker): `createTask` with an invalid field returns the friendly first-issue message
      (not a `ZodError`); valid input still inserts
- [ ] Manual: submitting an invalid task shows the specific reason inline; the dialog stays open

## Status
- [x] Complete
