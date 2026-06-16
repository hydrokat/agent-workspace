# Hashid Integer-Only Invariant

## Rule

`lib/utils/hash-id.ts` uses the `Hashids` library which **only operates on non-negative integers**.
`encodeId` and `decodeId` are therefore **only valid for autoincrement `BIGSERIAL` primary keys**.
Passing a UUID through `encodeId`/`decodeId` is a programming error.

## Schema Ground Truth

| Resource key   | Table column         | Type      | Hashid-eligible |
|----------------|----------------------|-----------|-----------------|
| `task`         | `tasks.id`           | BIGSERIAL | ✅              |
| `team`         | `teams.id`           | BIGSERIAL | ✅              |
| `participation`| `task_participations.id` | BIGSERIAL | ✅          |
| `submission`   | `task_submissions.id`| BIGSERIAL | ✅              |
| `reset_request`| `system_reset_requests.id` | BIGSERIAL | ✅        |

| Column         | Type | Must NEVER be hashid-encoded |
|----------------|------|------------------------------|
| `profiles.id`  | UUID | ✅                           |
| `user_id` FKs  | UUID | ✅                           |
| `created_by`   | UUID | ✅                           |
| `team_lead_id` | UUID | ✅                           |
| Any `*_by` admin/audit columns | UUID | ✅      |

## Enforcement

As of Phase 015 (Task 004), `encodeId` throws a developer-facing error if given a non-integer id:

```ts
// This throws:
encodeId('submission', 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11')
// → Error: encodeId expects a non-negative integer id; got "..." for resource "submission"

// decodeId already returned null on failure — this behaviour is preserved.
decodeId('submission', 'some-uuid-string') // → null
```

## When Adding a New Resource

1. Verify the PK column is `BIGSERIAL` in the migration SQL.
2. Add it to the table above.
3. UUID columns (`profiles.id`, FK columns) must **never** be passed to `encodeId`/`decodeId`.
4. Add a Jest test confirming the round-trip for the new resource.
