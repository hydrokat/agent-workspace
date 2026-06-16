# Task 001: Task categories data model & migration

## Description
Introduce admin-managed **task categories** and make every task belong to exactly one
category. This is the foundation for category-based point capping (Tasks 003/004).

## Existing Implementation & Guidelines
- `supabase/migrations/20240520000002_create_tasks.sql` — `tasks` table (`BIGSERIAL` id,
  `team_id`, `base_points`, role limits). No category column today.
- RLS pattern reference: `app_configurations` policies in
  `20260502100000_add_app_config_and_profile_status.sql` (`is_admin()` for writes, authenticated
  read). Mirror this for `task_categories`.
- Migration naming/style: timestamped SQL files; use `update_updated_at_column()` trigger as
  other tables do.
- Guideline: BIGSERIAL PKs only are hashid-encoded (phase-015 task-004); category ids follow
  the same rule when exposed to the client.

## Approach
1. New migration `..._create_task_categories.sql`:
   - `task_categories(id BIGSERIAL PK, name VARCHAR UNIQUE NOT NULL, description TEXT NULL,
     created_at, updated_at)` + updated_at trigger. (Cap columns added in Task 003.)
   - Enable RLS: authenticated `SELECT`; `ALL` only `is_admin()`.
   - Seed a default `'General'` category.
2. Migration to add `tasks.category_id`:
   - Add nullable `category_id BIGINT REFERENCES task_categories(id)`.
   - **Backfill** all existing tasks to the `'General'` category.
   - `ALTER ... SET NOT NULL` and add FK (`ON DELETE RESTRICT` so a category in use can't be
     deleted out from under tasks).
3. Update `lib/types.ts` (`Task`, new `TaskCategory`) to reflect the schema.

## Objectives
- [ ] `task_categories` table created with RLS (admin-write, authenticated-read) + default seed
- [ ] `tasks.category_id` added, backfilled, set NOT NULL with FK (RESTRICT on delete)
- [ ] TypeScript types updated (`TaskCategory`, `Task.category_id`)

## Unit Testing
- [ ] Jest (Docker): a created task resolves its category; deleting an in-use category is
      rejected (RESTRICT); non-admin cannot insert/update a category (RLS).

## Security Audit
- [ ] RLS verified: only `is_admin()` may write categories; authenticated read only.
- [ ] FK `ON DELETE RESTRICT` prevents orphaned tasks.
- [ ] No UUID encoded for category ids; integer-only hashid rule respected.
- [ ] Run `security-auditor` agent on the migration + type diff.

## Status
- [ ] Pending
