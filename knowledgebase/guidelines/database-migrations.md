# Database Migrations Policy

## Rule 1 — Never Modify a Committed Migration

Once a migration file has been committed to version control, it is immutable.
Do not edit, delete, or rebase committed migration files. Always add a new
migration to reverse or correct the schema.

## Rule 2 — Destructive Changes Must Be Backward-Compatible

A destructive schema change — any operation that could cause data loss or
downtime — must follow this sequence:

1. **Additive phase:** Add the new column, table, or index while keeping the
   old structure operational.
2. **Backfill phase:** Run a backfill script or migration to populate the new
   structure from the old data.
3. **Dual-write phase:** Write to both old and new structures until the
   migration is verified.
4. **Cutover phase:** Switch reads to the new structure.
5. **Cleanup phase:** Remove the old structure in a separate, later migration
   after a successful cutover.

**Examples of destructive operations:**

| Operation | Backfill requirement |
|---|---|
| Drop a column | Backfill data into replacement column first |
| Rename a column | Add new column, dual-write, backfill, drop old |
| Change column type | Add new column with new type, backfill, drop old |
| Drop a table | Ensure no production queries reference it for N days |

## Rule 3 — Local-Only During Development

All schema changes must be tested against a local database first. No migration
may be applied to staging or production without first passing local and CI
validation.

## Rule 4 — Migration Naming

Use a consistent prefix format for migration files:

```
YYYYMMDD_HHMMSS_description.<ext>
```

Or follow the framework's convention (e.g., Laravel, Prisma, TypeORM) as long
as it produces ordered, non-conflicting filenames.