# Task 003: Admin submission approve/reject id consistency & error handling

## Description

Sentry `5a6b5ac9…` (`POST /admin/submissions`): approving a pending submission throws
`"Invalid submission identifier."`. This is a **real logic bug**, not just a display problem.

### Root cause
- `lib/actions/submissions.ts:getPendingSubmissions` returns rows with the **raw numeric** `id`.
- `app/admin/submissions/submissions-table.tsx` passes `String(s.id)` (a raw number string) to
  `reviewTaskSubmission`.
- In `reviewTaskSubmission`:
  - **reject** branch uses the id raw: `.eq('id', submission_id as unknown as number)` → works.
  - **approve** branch calls `decodeId('submission', submission_id)` on that same raw numeric
    string. It is not a valid hashid, so `decodeId` returns `null` → `throw "Invalid submission identifier."`.

The two branches disagree on whether the id is hashid-encoded.

### Approach — make id handling consistent
**Recommended (Option A): encode on the way out, decode on both branches** — keeps DB ids opaque
to the client, consistent with the hashid pattern used elsewhere (`participation`, dashboard
submit):
1. In `getPendingSubmissions`, map each row's `id` to `encodeId('submission', row.id)` before
   returning (and adjust the `SubmissionsTable` prop type to `id: string`).
2. In `reviewTaskSubmission`, decode in **both** branches: replace the reject branch's
   `submission_id as unknown as number` with `parseInt(decodedSubmissionId, 10)` using the same
   `decodeId('submission', ...)` guard already present in the approve branch. Decode once at the
   top, throw the friendly identifier error once, then branch on status.

**Alternative (Option B, minimal): drop the decode** — if exposing sequential admin-only ids is
acceptable, remove `decodeId` from the approve branch and use the raw id like reject. Lower
surface area but leaves sequential ids exposed and abandons the hashid convention. Option A is
preferred.

### Error display
- The table already has an `errorMessage` slot and `catch`es approve/reject failures, but shows
  generic text. Surface the action's actual `Error.message` (e.g. invalid identifier, RPC
  failure) so the admin sees the real reason. Keep `actionError` normalization for the RPC/DB
  errors (no leakage).

## Objectives
- [x] Approve and reject use the **same** id encoding/decoding scheme (Option A recommended)
- [x] Approving a pending submission succeeds end-to-end (no "Invalid submission identifier.")
- [x] If an id is genuinely invalid, a friendly message renders inline (no crash)
- [x] `SubmissionsTable` prop/type updated to match the chosen id format
- [x] RPC/DB errors still routed through `actionError`

## Validation
- [ ] Jest (Docker): `reviewTaskSubmission` approve and reject both accept the encoded id and
      reach the correct Supabase call; an invalid id yields the friendly identifier error
- [ ] Manual: approve a real pending submission (points disbursed); reject still works; both
      show inline errors on failure

## Status
- [x] Complete
