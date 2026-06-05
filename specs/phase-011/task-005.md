# Task 005: Middleware onboarding gate + auth-link wiring

## Description
Extend `middleware.ts` so authenticated users with `must_complete_onboarding = true` are redirected to
`/welcome`, and ensure the Supabase invite link's `redirectTo` target is correctly handled by the
existing auth routes and allowlisted everywhere needed.

## Reference
- `lib/supabase/middleware.ts` (existing `must_reset_password` / `status === 'Pending'` branches)
- `app/auth/{confirm,callback,auth-code-error}` (token exchange + `next` param handling)
- Supabase Auth "Redirect URLs" allowlist (per environment)

## Objectives
- [x] In `middleware.ts`, extend the profile select to include `must_complete_onboarding`.
- [x] Add a redirect branch: if `must_complete_onboarding` and the path is not `/welcome`, `/auth`,
      or `/_next`, redirect to `/welcome`. Order it sensibly relative to the existing
      `must_reset_password` and `Pending` branches (avoid redirect loops).
- [x] Confirm `app/auth/confirm` (and `callback`) honor a `next=/welcome` param and route the user to
      `/welcome` after token exchange; adjust if needed.
- [x] Document the required Supabase "Redirect URLs" allowlist entry (`<APP_URL>/auth/confirm`) for dev,
      staging, and prod.
- [x] Verify no interference with existing reset-password / pending-approval gating.

## Acceptance Criteria
- A flagged user hitting any protected route is sent to `/welcome`; once the flag clears, normal routing resumes.
- The invite email link lands the user authenticated on `/welcome` with no redirect loop.
- Existing first-run flows (password reset, pending approval) are unaffected.

## Validation
- Manual: invite → email link → `/welcome`; and direct-navigation attempts while flagged.
- Middleware redirect unit tests in Task 006.

## Status
- [x] Completed
