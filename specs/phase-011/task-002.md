# Task 002: Server Actions — `inviteAdmin`, `listAdmins`, `completeOnboarding`

## Description
Implement the server-side logic for inviting admins, listing current admins, and completing
onboarding. All actions are server-only and authorization-gated. Follows the established
`createAndAddTeamMember` pattern (service-role admin client + profile update).

## Reference
- `lib/actions/team-members.ts::createAndAddTeamMember` (admin-create-user reference)
- `lib/actions/teams.ts::isAdmin` (authorization)
- `lib/actions/auth.ts::updatePassword`, `lib/actions/profiles.ts` (profile/password mutations)
- `lib/supabase/admin.ts::getAdminClient`
- `supabase/migrations/20260517000000_email_domain_enforcement.sql` (email domain rules)

## Where
- New: `lib/actions/admins.ts`

## Objectives
- [x] `inviteAdmin(email: string)`:
  - [x] Authorize via `isAdmin()`; throw `Unauthorized: Admin only.` otherwise.
  - [x] Validate + normalize email (trim/lowercase); enforce existing email-domain rules; reject invalid.
  - [x] Call `getAdminClient().auth.admin.inviteUserByEmail(email, { redirectTo, data: { role: 'Admin' } })`
        with `redirectTo = ${APP_URL}/auth/confirm?next=/welcome`.
  - [x] Update the new `profiles` row: `role = 'Admin'`, `status = 'Approved'`, `must_complete_onboarding = true`.
  - [x] Handle already-registered email idempotently (clear, non-leaking message).
  - [x] If SMTP is not configured (dev), fall back to `generateLink` and return/log the setup link — document.
  - [x] `revalidatePath('/admin/admins')`.
- [x] `listAdmins()`:
  - [x] Authorize via `isAdmin()`.
  - [x] Return profiles where `role = 'Admin'` (id, full_name, email, status, must_complete_onboarding)
        for the management table.
- [x] `completeOnboarding({ fullName, password })`:
  - [x] Require an authenticated session AND `must_complete_onboarding = true` (re-check; do not trust client).
  - [x] Validate `fullName` (non-empty, trimmed, length bound) and `password` (min 8).
  - [x] Set auth password (`supabase.auth.updateUser`) and `profiles.full_name`.
  - [x] Clear `must_complete_onboarding = false`.
  - [x] Return success for the page to redirect to `/`.

## Acceptance Criteria
- Non-admin callers cannot invoke `inviteAdmin` / `listAdmins` (server-side rejection).
- A successful invite produces an `Admin` / `Approved` / `must_complete_onboarding = true` profile.
- `completeOnboarding` only succeeds for a flagged user and atomically sets name+password and clears the flag.
- No secrets, tokens, or full error objects are returned to the client; failures log server-side only.

## Validation
- Unit tests in Task 006 (mocked Supabase via `lib/actions/supabase-mock.helper.ts`).
- Manual: invite a test email, confirm profile row state in DB.

## Status
- [x] Completed
