# Task 001: Reset-password old-password rejection & inline error display

## Description

Sentry `cf3b9ad4…` (`POST /reset-password`): Supabase rejects a reset when the new password
equals the current one ("New password should be different from the old password."). Today this
error is swallowed by `actionError` (generic `"Failed to update password."`) and the inline
`handleReset` server action `throw`s, so the user hits the **Next.js error boundary** (full-page
crash) instead of seeing why the reset failed.

### Affected code
- `app-src/lib/actions/auth.ts` → `updatePassword(newPassword)` (lines ~40–75)
- `app-src/app/reset-password/page.tsx` → inline `handleReset` server action (lines ~41–57) and
  the `<form action={handleReset}>` UI (no error slot currently).

### Approach
1. **Surface the real reason.** In `updatePassword`, when `authError` occurs, detect the
   "different from the old password" case and surface a friendly message. Prefer the existing
   `actionError` `allowlist` parameter, or pattern-match the Supabase message/`code`
   (e.g. `same_password`) and `throw new Error("Your new password must be different from your current password.")`. Keep the generic fallback for everything else (no DB-internal leakage).
2. **Stop crashing the page.** Convert the reset form to display errors **inline** instead of
   throwing into the error boundary. Recommended: make `handleReset` return a serializable
   `{ error?: string }` and drive it with `useActionState` (form must become a client component
   or extract a small client form), rendering the message in a destructive text slot like the
   other forms (`app/admin/tasks/task-management.tsx`). Validation messages (min length,
   passwords-don't-match) flow through the same slot.
3. Preserve the success path: on success, `redirect('/')` as today.

## Objectives
- [x] `updatePassword` returns/throws a friendly, allowlisted message for the same-password case
- [x] No DB/provider internals leak for non-allowlisted errors (generic fallback retained)
- [x] Reset form shows validation + server errors **inline** — no error-boundary full-page crash
- [x] Min-length and passwords-mismatch errors render in the same inline slot
- [x] Successful reset still redirects to `/`

## Validation
- [ ] Jest (Docker): `updatePassword` surfaces friendly message when Supabase returns the
      same-password error; returns generic fallback for an unknown error
- [ ] Manual: submitting the current password as the new one shows the inline message, page does
      not crash

## Status
- [x] Complete
