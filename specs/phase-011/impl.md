# Implementation Plan: Phase 011 - Admin Invitation & Onboarding

## Phase Objectives
Allow an existing **Admin** to invite additional admins by email. An invited admin receives a
Supabase-native invitation email, follows the setup link, and completes a dedicated `/welcome`
onboarding flow where they set their **full name** and **password** on first login. Once
onboarding completes, the account becomes a fully active Admin. All database work stays
server-side (Server Actions / Server Components) per workspace standards, and the feature ships
with passing unit tests and a documented security audit.

## Target Application
- **Repo**: `task-tracker-web/app-src` (sibling of this workspace)
- **Stack**: Next.js (App Router), React 19, Tailwind v4, shadcn/ui (Radix), Supabase SSR + Auth
- **Key existing surfaces this builds on**:
  - `lib/supabase/admin.ts` — `getAdminClient()` (service-role client for Auth Admin API)
  - `lib/actions/teams.ts` — `isAdmin()` authorization helper (`role === 'Admin'` + Approved + not reset-pending)
  - `lib/supabase/middleware.ts` — first-login gating via `must_reset_password` / `status` redirects
  - `app/reset-password/page.tsx` — existing mandatory-credential flow (password only)
  - `app/auth/{callback,confirm,auth-code-error}` — Supabase token-exchange routes (already wired)
  - `app/admin/{config,teams,...}` — existing admin surfaces and authorization pattern
  - `lib/actions/team-members.ts::createAndAddTeamMember` — reference for the admin-create-user pattern
  - `supabase/migrations/*` — timestamp-prefixed atomic migrations; `profiles` created by `handle_new_user` trigger

## Decisions (locked)
- **Email delivery**: Supabase **native invite** — `supabaseAdmin.auth.admin.inviteUserByEmail(email, { redirectTo, data })`.
  Supabase sends the invite email via its configured SMTP. (Assumes project SMTP is configured — see Dependencies.)
- **Onboarding surface**: a **dedicated `/welcome` route** (not the existing `/reset-password` page), since
  onboarding must capture **name + password**, a distinct first-run experience.
- **Admin management UI + approval**: a **new `/admin/admins` page**; invited admins are **auto-approved**
  with `role = 'Admin'`, `status = 'Approved'` on creation (mirrors the existing `createAndAddTeamMember`
  status handling). No second-admin approval step.

## Architecture

### Data model
A new boolean flag on `profiles` drives the onboarding gate, kept **separate** from
`must_reset_password` so the two first-run flows do not collide:
- `must_complete_onboarding boolean NOT NULL DEFAULT false` — set `true` for invited admins,
  cleared when `/welcome` is completed.

Invited admins are distinguished by `role = 'Admin'` + `must_complete_onboarding = true`. No
hashed-public-id concerns are introduced (no new auto-increment IDs leave the backend); admin
identity continues to key off the auth `user.id` (UUID).

### Invite flow (server-side)
1. Admin opens `/admin/admins`, submits an email in the invite dialog.
2. Server Action `inviteAdmin(email)`:
   - Authorizes with `isAdmin()`; rejects otherwise (`Unauthorized: Admin only.`).
   - Validates + normalizes the email (and honors existing email-domain enforcement, migration
     `20260517000000_email_domain_enforcement.sql`).
   - Calls `getAdminClient().auth.admin.inviteUserByEmail(email, { redirectTo: <APP_URL>/auth/confirm?next=/welcome })`.
   - Updates the resulting `profiles` row: `role = 'Admin'`, `status = 'Approved'`,
     `must_complete_onboarding = true`.
   - Handles the already-exists case gracefully (idempotent message, no duplicate).
3. Supabase emails the invitee a setup link.

### Onboarding flow (first login)
1. Invitee clicks the email link → Supabase token exchange via existing `app/auth/confirm` (then `callback`).
2. `middleware.ts` sees an authenticated user with `must_complete_onboarding = true` and redirects any
   route (except the onboarding/auth/static allowlist) to `/welcome`.
3. `/welcome` collects **full name** + **new password** (with confirm + min-length validation).
4. Server Action `completeOnboarding({ fullName, password })`:
   - Re-checks the session + `must_complete_onboarding` flag (defense in depth).
   - Sets the auth password (`supabase.auth.updateUser`) and `profiles.full_name`.
   - Clears `must_complete_onboarding = false`.
   - Redirects to `/`.

### Authorization & middleware gating
- All admin reads/writes go through Server Actions guarded by `isAdmin()`.
- `middleware.ts` gains a `must_complete_onboarding` redirect branch, ordered alongside the existing
  `must_reset_password` / `status === 'Pending'` branches, with `/welcome` and `/auth` allowlisted.

### Testing
- **Jest** (run via Docker per workspace convention: `docker compose ... run --rm npx jest` — the RTK
  proxy breaks direct `npx jest` output parsing). Cover `inviteAdmin` (authz, validation, profile
  update), `completeOnboarding` (flag clear, name/password set), and middleware redirect logic with
  mocked Supabase clients (reuse `lib/actions/supabase-mock.helper.ts`).

## Timeline
- **Task 001**: Schema migration — `must_complete_onboarding` flag on `profiles` (Completed)
- **Task 002**: Server Actions — `inviteAdmin`, `listAdmins`, `completeOnboarding` (Completed)
- **Task 003**: `/admin/admins` page + invite dialog UI (Completed)
- **Task 004**: `/welcome` onboarding route (name + password) (Completed)
- **Task 005**: Middleware onboarding gate + auth-link wiring (Completed)
- **Task 006**: Unit tests + security audit (Completed)
- **Task 007**: Knowledgebase update (Completed)

## Tasks
- [x] Task 001: Schema migration — `must_complete_onboarding` flag on `profiles`
- [x] Task 002: Server Actions — `inviteAdmin`, `listAdmins`, `completeOnboarding`
- [x] Task 003: `/admin/admins` page + invite dialog UI
- [x] Task 004: `/welcome` onboarding route (name + password)
- [x] Task 005: Middleware onboarding gate + auth-link wiring
- [x] Task 006: Unit tests + security audit
- [x] Task 007: Knowledgebase update

## Dependencies
- **Supabase SMTP** must be configured for `inviteUserByEmail` to actually deliver mail. If unconfigured,
  the invite link can be retrieved via `generateLink` for manual delivery in dev — note this in Task 002.
- **APP URL / Redirect allowlist**: the `redirectTo` target (`/auth/confirm?next=/welcome`) must be added
  to the Supabase Auth "Redirect URLs" allowlist for each environment.
- 001 blocks 002, 004, 005 (the flag must exist before code reads/writes it).
- 002 blocks 003 (UI calls the actions) and 004 (onboarding action).
- 005 depends on 001 + 004 (redirect target must exist).
- 006 depends on 002–005; 007 follows once patterns are stable.

## Acceptance Criteria
- An Admin can invite another admin by email from `/admin/admins`; a non-admin cannot reach the page or
  invoke the action (`isAdmin()` enforced server-side).
- The invitee receives a Supabase invite email; following the link lands them on `/welcome`.
- `/welcome` requires full name + password (confirm match, min 8 chars); on submit the profile name and
  auth password are set and `must_complete_onboarding` is cleared.
- After onboarding, the user is a fully active Admin (`role = 'Admin'`, `status = 'Approved'`) and is no
  longer redirected to `/welcome`.
- `must_complete_onboarding` users are redirected to `/welcome` from any protected route via middleware.
- All DB access is server-side; strict TypeScript; ESLint clean.
- Jest suite (via Docker) passes with new + existing tests green.
- A security audit (OWASP 2025 baseline) is documented: authorization, invite abuse/enumeration,
  input validation, email handling, and error/logging hygiene.
