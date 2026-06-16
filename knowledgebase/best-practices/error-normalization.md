# Error Normalization: `actionError` Helper Pattern

## Context

Established in Phase 014 (Task 001) to close the 45 raw `throw new Error(error.message)` sites
that leaked Supabase/PostgreSQL driver details to the client.

## The Helper

**Location**: `lib/utils/action-error.ts`

```ts
export function actionError(
  context: string,           // e.g. "teams.createTeam"
  error: unknown,            // the raw caught error
  fallback: string,          // generic user-facing message
  allowlist?: string[]       // optional: error codes/messages safe to surface
): Error
```

### Behavior

1. Logs the full raw error server-side (preserving diagnostic detail).
2. If `allowlist` is provided and the error message/code is in the list, surfaces the original
   message to the caller.
3. Otherwise returns a new `Error(fallback)` — a generic, non-leaking message.

## Usage Pattern

```ts
// In a Server Action:
const { data, error } = await supabase.from('teams').insert([...]).select().single();

if (error) {
  throw actionError("teams.createTeam", error, "Failed to create team.");
}
```

With allowlist (e.g. duplicate-key is safe to surface):
```ts
if (error) {
  if (error.code === '23505') {
    throw new Error("User is already a member of this team.");
  }
  throw actionError("team-members.addTeamMember", error, "Failed to add team member.");
}
```

## Rules

- Never `throw new Error(error.message)` directly in a Server Action — this leaks DB internals.
- Always provide a meaningful `fallback` that is safe to display in the UI.
- Add to the `allowlist` only messages that contain no schema/table/column information.
- The `context` string should be `"<file>.<functionName>"` to enable log correlation.

## OWASP Alignment

Closes OWASP A05:2021 (Security Misconfiguration) — prevents Supabase error codes, Postgres
error details, and hint messages from reaching the client layer.
