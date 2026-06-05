# Task 006: Unit tests + security audit

## Description
Validate the feature with automated unit tests and perform the mandatory feature security audit
(OWASP 2025 baseline). No task in this phase is complete without this gate.

## Reference
- `knowledgebase/guidelines/security.md` (Feature Security Audit; OWASP review)
- `knowledgebase/guidelines/backend-testing-strategy.md`
- `lib/actions/supabase-mock.helper.ts`, `lib/actions/__tests__/` (existing patterns)
- Jest runs via Docker: `docker compose ... run --rm npx jest` (RTK proxy breaks direct `npx jest`).

## Objectives — Tests
- [x] `inviteAdmin`: rejects non-admins; validates/normalizes email + domain rules; sets
      `role='Admin'`, `status='Approved'`, `must_complete_onboarding=true`; handles already-exists.
- [x] `listAdmins`: rejects non-admins; returns only `role='Admin'` profiles with expected fields.
- [x] `completeOnboarding`: rejects unauthenticated / unflagged users; validates name + password;
      sets name + password and clears the flag.
- [x] Middleware: flagged user redirected to `/welcome`; unflagged passes; no loop with reset/pending branches.
- [x] All new + existing suites green via Docker.

## Objectives — Security Audit (document findings in this file)
- [x] **Authorization**: all admin actions gated by `isAdmin()`; `/admin/admins` + `/welcome` access control verified.
- [x] **Invite abuse / enumeration**: ensure responses don't leak whether an email already exists;
      consider rate-limiting / admin-only invocation as the control.
- [x] **Input validation**: email format + domain enforcement; name/password bounds.
- [x] **Privilege escalation**: confirm a non-admin cannot self-assign `role='Admin'` via any path.
- [x] **Secrets/logging**: service-role key stays server-only; no tokens/links leaked to clients or logs in prod.
- [x] **Error handling**: generic client-facing errors; detailed logs server-side.
- [x] Record each item as fixed / accepted (with rationale) / deferred (owner + follow-up).

## Security Audit Notes
| Area | Finding | Status |
|------|---------|--------|
| **Authorization** | All admin actions explicitly require `isAdmin()`. `/welcome` onboarding requires authenticated session and `must_complete_onboarding` flag. | FIXED / VERIFIED |
| **Invite abuse / enumeration** | inviteAdmin is admin-only. Already-registered emails return generic "processed" message to prevent email enumeration. | FIXED / VERIFIED |
| **Input validation** | Email domain constraint enforced in Server Action. Name/Password inputs validated for bounds (Name: 1-100 chars, Password: >=8 chars). | FIXED / VERIFIED |
| **Privilege escalation** | Non-admins cannot self-assign Admin roles. Role updates are server-side only, gated by `isAdmin()`. | FIXED / VERIFIED |
| **Secrets/logging** | Supabase service-role key stays server-only. Action links/tokens are only logged in dev/fallback mode. | FIXED / VERIFIED |
| **Error handling** | Internal errors are logged on the server. Generic user-facing messages are returned to prevent database leakage. | FIXED / VERIFIED |

## Acceptance Criteria
- Jest suite passes (Docker) with the new coverage.
- Security audit table is filled in; no open critical/high issues at release.

## Status
- [x] Completed
