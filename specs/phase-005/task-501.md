# Task 501: Database Migration: `must_reset_password`

## Goal
Implement the necessary database changes to support forced password resets through a completely new, atomic migration, in strict adherence to the project's migration standards.

## Requirements
- Read and strictly comply with [`supabase-atomic-migrations-guideline.md`](../../knowledgebase/guidelines/supabase-atomic-migrations-guideline.md).
- Create a **NEW** migration to add `must_reset_password` (boolean, default false) to the `profiles` table.
- **DO NOT** modify any existing migration files in `supabase/migrations/`.
- Ensure the migration is atomic, reversible, and correctly timestamped.

## Implementation Details
- **Migration File**: `supabase/migrations/[timestamp]_add_must_reset_password_to_profiles.sql`.
- **Command**: Generate the migration via the Supabase CLI (`supabase migration new add_must_reset_password_to_profiles`) and write the corresponding SQL manually. No existing history is to be tampered with.

## Verification
- Verify that a completely new migration file has been created.
- Verify that NO existing migration files have been modified.
- Run the migration locally (`supabase db push` or `supabase db reset`) and verify the new column exists.
