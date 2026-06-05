# Task 001: Schema migration — `must_complete_onboarding` flag on `profiles`

## Description
Add a boolean flag to `profiles` that gates the new-admin onboarding (`/welcome`) flow. It is kept
separate from `must_reset_password` so the password-reset and name+password onboarding flows do not
interfere. Delivered as a single atomic, timestamp-prefixed Supabase migration.

## Reference
- `knowledgebase/guidelines/supabase-atomic-migrations-guideline.md`
- Existing migrations: `supabase/migrations/20260430182554_add_must_reset_password_to_profiles.sql`,
  `supabase/migrations/20260426191000_profile_triggers.sql` (`handle_new_user`)
- `knowledgebase/guidelines/security.md`

## Objectives
- [x] Add migration `supabase/migrations/<timestamp>_add_must_complete_onboarding_to_profiles.sql`.
- [x] Column: `must_complete_onboarding boolean NOT NULL DEFAULT false`.
- [x] Ensure the migration is idempotent-safe (`ADD COLUMN IF NOT EXISTS`) and wrapped atomically.
- [x] Confirm no change is needed to the `handle_new_user` trigger (the flag is set by the
      `inviteAdmin` action, not at trigger time) — document the decision in the migration comment.
- [x] Regenerate / update TS types: add `must_complete_onboarding?: boolean` to the `Profile` type in
      `lib/types.ts`.

## Acceptance Criteria
- Migration applies cleanly on a fresh DB and is re-runnable without error.
- `profiles.must_complete_onboarding` exists with the correct default.
- `lib/types.ts` reflects the new field.

## Validation
- Apply the migration locally (Supabase CLI / Docker) and verify the column + default.
- `tsc` passes with the updated `Profile` type.

## Status
- [x] Completed
