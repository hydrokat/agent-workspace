# PostgreSQL View RLS and `security_invoker` Best Practices

## Context & Problem

By default in PostgreSQL (versions 15+), views created with `CREATE VIEW` have `security_invoker = false`.
This means:
1. Queries against the view execute with the privileges of the view owner (typically `postgres` in Supabase).
2. The view owner possesses `BYPASSRLS`, meaning all underlying Row Level Security (RLS) policies on tables queried by the view are completely bypassed.
3. Combined with broad schema grants (such as `GRANT ALL ON TABLES TO anon, authenticated`), any external PostgREST caller can read rows through the view that RLS on the base tables would otherwise forbid.
4. Supabase Security Advisor flags this vulnerability as `security_definer_view`.

## Pattern: Enforcing `security_invoker = true`

To enforce that callers only see data permitted by their own session (`auth.uid()` and caller roles):

```sql
-- 1. Enable caller security context on the view
ALTER VIEW public.<view_name> SET (security_invoker = true);

-- 2. Revoke default access from unauthenticated / public roles
REVOKE ALL ON public.<view_name> FROM PUBLIC, anon;

-- 3. Revoke any non-SELECT privileges from authenticated callers
REVOKE INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER ON public.<view_name> FROM authenticated;

-- 4. Grant SELECT strictly to authenticated users and service_role
GRANT SELECT ON public.<view_name> TO authenticated, service_role;
```

## How PostgreSQL Evaluates Caller RLS on Views

When `security_invoker = true` is set:
- PostgreSQL rewrites the incoming view query and applies the RLS policies of the underlying tables using the calling user's permissions and session tokens (`auth.uid()`, `auth.jwt()`).
- In aggregated views (e.g. `SELECT user_id, SUM(points) FROM point_transactions GROUP BY user_id`), rows are filtered by the caller's RLS policies **before** aggregation occurs.
- A regular member sees only aggregates computed from rows they are allowed to select.
- An admin (evaluating `is_admin() = true`) sees aggregates across all users.
- An anonymous caller without `SELECT` permissions on the view or underlying tables receives an immediate permission denied error.

## Verification Checklist

1. **Static SQL Contract Test**: Assert that the migration sets `ALTER VIEW ... SET (security_invoker = true)` and restricts grants.
2. **Types Mapping**: Include the view under `Database['public']['Views']` in TypeScript database definitions.
3. **Integration Verification**:
   - Verify unauthenticated (`anon`) query fails or yields zero unauthorized data.
   - Verify non-admin member sees only their own rows, and query filters targeting another user return 0 rows.
   - Verify admin role receives full multi-user aggregates.
