---

name: supabase-atomic-migrations-guideline
description: Enforces atomic, immutable, and traceable Supabase migrations. Ensures safe schema evolution, auditability, and prevents destructive migration edits.

triggers:

- supabase project detected
- database schema changes required
- migration creation or modification
- table/index/rls updates
- team working with supabase migrations

principles:

- migrations must be atomic units of change
- migrations are immutable once committed
- schema evolution is append-only
- every change must be traceable and reversible
- no silent or undocumented schema drift allowed
- safe migrations: no conflicts on existing schema and other migrations

atomic_migration_rules:

- each migration must represent exactly ONE logical change:
  examples: - create table - add column - alter constraint - update RLS policy - create index
- do NOT combine unrelated changes in one migration
- keep migrations small, focused, and reversible where possible

immutability_rules:

- NEVER modify an existing migration file after it has been applied or committed
- if a change is needed:
  - create a NEW migration instead of editing old ones
- treat migrations as immutable history logs
- database history must match git history exactly

tracking_rules:

- all migrations must be timestamped and sequential
- migration names must clearly describe intent:
  format:
  YYYYMMDDHHMMSS_descriptive_action.sql
- every migration must be traceable to a feature or ticket
- avoid generic names like "update_table.sql"

safety_rules:

- destructive changes must be explicit:
  - DROP TABLE / DROP COLUMN requires dedicated migration
- no hidden data loss inside multi-purpose migrations
- always prefer additive changes over destructive ones
- schema rollback must be considered before applying migration

supabase_specific_rules:

- always consider RLS (Row Level Security) impact
- ensure policies are updated in separate migrations if needed
- triggers/functions must be versioned separately
- use Supabase CLI migration flow only (no manual DB edits in production)
- always grant USAGE and ALL privileges on the `public` schema to API roles (`anon`, `authenticated`, `service_role`) if there are 'permission denied' or 'relation does not exist' errors across environments.
- explicitly prefix tables with `public.` or use `ALTER TABLE ... SET SCHEMA public` to ensure robust resolution of tables regardless of the active connection's `search_path`.

workflow:

- identify required schema change
- check if similar migration already exists
- design atomic migration step
- generate new migration file only
- write reversible SQL where possible
- apply migration in local environment first
- verify Supabase migration status
- commit migration without modification afterward

anti_patterns:

- editing existing migration files
- bundling multiple schema changes in one migration
- direct production DB edits outside migrations
- unclear or generic migration naming
- skipping migration review before apply
- losing sync between local and remote schema

best_practices:

- prefer incremental schema evolution
- test migrations on local Supabase before production
- pair migration with feature branch
- document intent in migration comments when complex
- review migration diff before commit

output_rules:

- always state if migration is NEW or referencing existing history
- always describe what the migration changes at a high level
- explicitly warn if any modification of existing migration is attempted
- ensure migration remains atomic and single-purpose
