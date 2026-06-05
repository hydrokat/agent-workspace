# Admin Invitation & Onboarding Pattern

Introduced in Phase 011. Describes how to programmatically invite new admins and gate their
first-login experience to capture name and password.

## Overview

1. An existing Admin submits an email via `/admin/admins` → `inviteAdmin()` Server Action.
2. Supabase sends a native invite email (`auth.admin.inviteUserByEmail`).
3. The invitee follows the link → existing `app/auth/confirm` token exchange → lands on `/welcome`.
4. `/welcome` captures **full name + password** (distinct from `/reset-password` which is password-only).
5. `completeOnboarding()` clears `must_complete_onboarding`, sets name/password → user is active Admin.

## Data model

`profiles.must_complete_onboarding boolean NOT NULL DEFAULT false`

- Set `true` by `inviteAdmin()` immediately after `inviteUserByEmail` succeeds.
- Cleared to `false` by `completeOnboarding()` on successful first-login setup.
- **Separate from** `must_reset_password`: that flag is for existing users resetting credentials; this
  flag is for new invitees completing initial profile setup.

## When to use each flag

| Scenario | Flag to set |
|----------|-------------|
| Admin forgets / needs to change password | `must_reset_password = true` → `/reset-password` |
| New admin invited via email, must set name + password | `must_complete_onboarding = true` → `/welcome` |

## Middleware gate order

The `must_complete_onboarding` redirect is checked **before** `must_reset_password` in middleware.
Both `/welcome` and `/auth` are in the allowlist for the other branches to avoid redirect loops.

## Supabase configuration requirements

- `redirectTo` (`<APP_URL>/auth/confirm?next=/welcome`) must be in the Supabase Auth **Redirect URLs**
  allowlist for each environment (dev, staging, prod).
- Supabase project SMTP must be configured for `inviteUserByEmail` to deliver email. In dev (or when
  SMTP is unconfigured), `inviteAdmin()` falls back to `auth.admin.generateLink` and returns the link
  in the response for manual use — this link is logged / surfaced in the UI under a "DEV ONLY" label.

## Authorization

- `inviteAdmin()` and `listAdmins()` are guarded by `isAdmin()` from `lib/actions/teams.ts`.
- `completeOnboarding()` re-verifies both session and `must_complete_onboarding = true` server-side
  (defense in depth — does not trust client state).
- Auto-approved: invited admins receive `role='Admin'`, `status='Approved'` immediately on invite.
  No second-admin approval step is required.

## Security notes (OWASP 2025 reviewed)

- Email already-exists responses are generic to prevent enumeration.
- Invite is admin-only; no public endpoint exists to self-assign Admin role.
- Service-role key never leaves the server; invite links are not stored in DB.
