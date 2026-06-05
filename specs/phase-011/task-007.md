# Task 007: Knowledgebase update

## Description
Capture the admin-invitation + onboarding pattern in the knowledgebase so future work reuses it
consistently. Follows the Documentation Sync principle.

## Reference
- `knowledgebase/best-practices/`, `knowledgebase/guidelines/`
- This phase's `impl.md` and tasks 001–006

## Objectives
- [x] Document the invite/onboarding flow: Supabase `inviteUserByEmail` → `must_complete_onboarding`
      flag → middleware gate → `/welcome` (name + password) → activation.
- [x] Note the distinction between `must_reset_password` (password-only) and
      `must_complete_onboarding` (name + password) first-run flows and when to use each.
- [x] Record the auto-approved-Admin decision and the `redirectTo` / Supabase Redirect-URL allowlist requirement.
- [x] Cross-link the security-audit outcome from Task 006.
- [x] Update `CHANGELOG.md`.

## Acceptance Criteria
- A best-practice/guideline note exists describing the reusable invite + onboarding pattern.
- Changelog reflects Phase 011.

## Status
- [x] Completed
