# Implementation Plan: Error Handling Fixes (Sentry Triage)

## Phase Objectives

Resolve three production error-handling defects surfaced by Sentry. The common thread is
that **server actions throw raw errors** that either (a) crash into the Next.js error
boundary, (b) leak unfriendly framework/validation text, or (c) mask a real logic bug. Each
fix must surface a clear, user-facing message **inline** in the relevant UI rather than
crashing or showing a generic fallback, while continuing to follow the established
[`actionError`](../../knowledgebase/best-practices/error-normalization.md) normalization pattern.

Sentry issues addressed:
- `cf3b9ad4…` — `POST /reset-password` — "New password should be different from the old password."
- `a4a621623e…` — `POST /tasks` — "Invalid input: expected string, received number"
- `5a6b5ac9…` — `POST /admin/submissions` — "Invalid submission identifier."

## Architecture

### Root causes
1. **Reset password** (`app/reset-password/page.tsx` + `lib/actions/auth.ts:updatePassword`)
   - The inline `handleReset` server action `throw`s on validation failure, and
     `updatePassword` wraps the Supabase error through `actionError(...)` with no allowlist,
     so the Supabase "same password" message is collapsed into the generic
     `"Failed to update password."` and then bubbles up to the **error boundary** (full-page
     crash). The user is never told the real reason.

2. **Tasks** (`lib/actions/tasks.ts:createTask`/`updateTask` + `app/admin/tasks/task-management.tsx`)
   - Both actions call `TaskInputSchema.parse(data)`, which throws a raw `ZodError`. The
     client `catch` shows a hard-coded `"Failed to save task. Please try again."`, while the
     raw Zod message (`"Invalid input: expected string, received number"`) is what reaches
     Sentry. Validation failures are neither friendly nor displayed accurately.

3. **Admin submissions** (`lib/actions/submissions.ts:reviewTaskSubmission` + table/page)
   - `getPendingSubmissions` returns **raw numeric** `id`s. The table passes `String(s.id)`
     to `reviewTaskSubmission`. The **reject** branch uses the id raw
     (`submission_id as unknown as number`) and works; the **approve** branch calls
     `decodeId('submission', submission_id)` on that same raw numeric id, which is not a valid
     hashid, returns `null`, and throws `"Invalid submission identifier."`. The two branches
     disagree on the id encoding. This is a genuine logic bug, not just a display problem.

### Cross-cutting approach
- Prefer `safeParse` + friendly first-issue message over `.parse()` in server actions.
- Use the `actionError` `allowlist` to surface safe, user-meaningful provider messages
  (e.g. the "same password" case) instead of swallowing them.
- Render returned messages **inline** in the existing UI error slots (no error-boundary
  crash, no generic placeholder when a specific message exists).
- Keep id handling **consistent** across approve/reject in submissions.
- **Hashid integer-only invariant (Task 004):** `lib/utils/hash-id.ts` (Hashids) only works on
  integers. Only **autoincrement `BIGSERIAL`** ids may be encoded/decoded; **UUIDs are never
  encoded/decoded** (coercing a UUID through `Number(...)` yields `NaN` — the same failure class
  as Task 003). Autoincrement: `tasks.id`, `teams.id`, `task_participations.id`,
  `task_submissions.id`, `system_reset_requests.id`. UUID (pass-through): `profiles.id`,
  `user_id`, `created_by`, `team_lead_id`.

### `encodeId`/`decodeId` call sites (audit target for Task 004)
All five resources in use map to `BIGSERIAL` PKs (no UUID encoded today):
- `task` → `tasks.id` · `team` → `teams.id` · `participation` → `task_participations.id`
- `submission` → `task_submissions.id` · `reset_request` → `system_reset_requests.id`

## Timeline
- **Task 001**: Reset-password — reject old-password reuse & show errors inline (Complete)
- **Task 002**: Tasks — friendly, displayed validation errors (Complete)
- **Task 003**: Admin submissions — consistent id handling & proper error display (Complete)
- **Task 004**: Validate hashid encode/decode applies only to autoincrement ids, never UUIDs (Complete)

## Tasks
- [x] Task 001: Reset-password old-password rejection & inline error display
- [x] Task 002: Task create/update validation error surfacing
- [x] Task 003: Admin submission approve/reject id consistency & error handling
- [x] Task 004: Hashid integer-only invariant (no UUID encode/decode)

## Validation Strategy
- Unit tests (Jest via Docker — see project memory) for the three server actions:
  `updatePassword` allowlist behavior, `createTask`/`updateTask` `safeParse` messaging, and
  `reviewTaskSubmission` approve/reject id paths.
- Manual validation of each form's inline error UI (no full-page crash) where automated
  coverage of the boundary is impractical.
