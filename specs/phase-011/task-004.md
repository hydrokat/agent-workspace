# Task 004: `/welcome` onboarding route (name + password)

## Description
Create the dedicated first-login onboarding page where an invited admin sets their **full name** and
**password**. This is distinct from `/reset-password` (which only updates a password). On successful
submit, the account is fully activated and the user proceeds into the app.

## Reference
- `app/reset-password/page.tsx` (structure, server-action-in-page pattern, Nexus styling)
- `lib/actions/admins.ts::completeOnboarding` (Task 002)
- `app/auth/{confirm,callback}` (token exchange that lands the user here)
- `knowledgebase/guidelines/nexus-ui.md`, i18n dictionaries under `lib/i18n/`

## Where
- New: `app/welcome/page.tsx` (Server Component gating + form → `completeOnboarding`)

## Objectives
- [x] On load: require an authenticated session; if `must_complete_onboarding` is not set, redirect to `/`.
- [x] Form fields: Full Name (required), New Password + Confirm Password (required, min 8, must match).
- [x] Submit calls `completeOnboarding({ fullName, password })`; on success `redirect('/')`.
- [x] Client/server validation for empty name, short password, mismatch — clear inline errors.
- [x] Nexus-styled, responsive, all copy from i18n dictionaries (`en` + `en-simple`).

## Acceptance Criteria
- Only users with `must_complete_onboarding = true` can use the page; others are redirected away.
- Submitting valid input sets name + password, clears the flag, and lands the user on `/` as an active Admin.
- Validation errors are shown without exposing server internals.

## Validation
- Manual: complete the flow end-to-end from an invite email link.
- Action-level tests in Task 006.

## Status
- [x] Completed
