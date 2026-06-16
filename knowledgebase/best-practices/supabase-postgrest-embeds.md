# Supabase/PostgREST: Disambiguate Embeds on Multi-FK Tables

## Bug Record

**Cause**: `getTeamLeaderboard` used an unqualified embed `user:profiles(points, status)` on `team_members`. The table has two foreign keys to `profiles` — `fk_tm_user` (on `user_id`) and an unnamed FK on `approved_by` (added in migration `20260502120000`). PostgREST raises **PGRST201** ("more than one relationship was found") and rejects the query entirely.

Unit tests mocked the Supabase client and passed. The error only surfaces against a real database.

**Fix**: Add the FK hint: `user:profiles!fk_tm_user(points, status)`.

**Prevention rules:**

1. **Always disambiguate embeds on tables with multiple FKs to the same target.** Use the `!constraint_name` hint (e.g., `profiles!fk_tm_user`).

2. **Add a select-string regression assertion in unit tests** when the join is non-trivial:
   ```ts
   expect(queryBuilder.select.mock.calls.some(
     (c) => c[0].includes("profiles!fk_tm_user")
   )).toBe(true);
   ```

3. **Require manual validation against the local Supabase stack** before marking any DB-integration task Complete. Jest mocks cannot reproduce PostgREST errors. Check with:
   ```bash
   supabase db reset   # apply all migrations
   # then load the feature page in the browser
   ```

4. **When adding a new FK column to an existing table**, grep all `select()` strings in the codebase that embed the target table from that source table and add hints if ambiguous.

## Reference
- Affected file (fixed): `app-src/lib/actions/leaderboard.ts:105`
- Correct pattern: `app-src/lib/actions/team-members.ts:325, 365, 416`
- PostgREST docs: [Disambiguation](https://postgrest.org/en/stable/references/api/resource_embedding.html#hint-disambiguation)
