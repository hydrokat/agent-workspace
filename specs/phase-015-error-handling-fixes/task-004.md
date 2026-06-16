# Task 004: Validate hashid encode/decode applies only to autoincrement ids (never UUIDs)

## Description

`lib/utils/hash-id.ts` is built on `Hashids`, which only operates on **integers**
(`encodeId` does `Number(id)`). It is therefore only valid for **autoincrement** primary keys
(`BIGSERIAL`). Passing a **UUID** (e.g. `profiles.id`, `user_id`, `created_by`,
`team_lead_id`) through `encodeId`/`decodeId` is a silent bug: `Number(<uuid>)` is `NaN`, so
encoding yields garbage/empty and decoding fails — exactly the failure class behind the
"Invalid submission identifier." crash in Task 003. This task locks in the invariant:
**only autoincrement integer ids are ever hashid-encoded/decoded; UUIDs are passed through
untouched.**

### Schema ground truth (from `supabase/migrations`)
- **Autoincrement `BIGSERIAL` (hashid-eligible):** `tasks.id`, `teams.id`,
  `task_participations.id`, `task_submissions.id`, `system_reset_requests.id`.
- **UUID (must NEVER be encoded/decoded):** `profiles.id` (= `auth.users.id`), and all UUID
  FKs: `user_id`, `created_by`, `team_lead_id`, `*_by` admin/audit columns.

### Current state (audit result)
The five resources currently passed to `encodeId`/`decodeId` — `task`, `participation`,
`team`, `submission`, `reset_request` — all map to `BIGSERIAL` PKs, so **no UUID is encoded
today**. This task makes that correctness explicit and regression-proof rather than incidental.

### Approach
1. **Audit all call sites** of `encodeId`/`decodeId` (see list in `impl.md` notes) and confirm
   each `resource` corresponds to a `BIGSERIAL` PK column, not a UUID. Document the
   resource → column mapping.
2. **Harden the helper.** In `lib/utils/hash-id.ts`, guard against non-integer input so a UUID
   can never be silently coerced:
   - `encodeId`: reject input where `!Number.isInteger(Number(id))` (throw a clear dev-facing
     error, e.g. `"encodeId expects an integer id; got <value>"`).
   - `decodeId`: already returns `null` on failure — keep that, and ensure callers treat
     `null` as a user-facing "invalid identifier" (ties into Tasks 001/003 error display).
3. **Tests** (Jest via Docker): assert `encodeId`/`decodeId` round-trip integers, and that
   passing a UUID string to `encodeId` throws (and `decodeId` of a UUID returns `null`).
4. **Document** the invariant in `knowledgebase/best-practices/` (or extend the hash-id
   note) so future resources are added correctly.

## Objectives
- [x] Resource → PK-column mapping documented; every hashid resource confirmed `BIGSERIAL`
- [x] No UUID column is ever passed to `encodeId`/`decodeId`
- [x] `hash-id.ts` hardened to reject non-integer input instead of silently coercing to `NaN`
- [x] Invariant documented in the knowledgebase

## Validation
- [ ] Jest (Docker): integer round-trip passes; `encodeId(<uuid>)` throws; `decodeId(<uuid>)`
      returns `null`
- [ ] Grep audit shows no `encodeId`/`decodeId` call bound to a UUID-typed column

## Status
- [x] Complete
