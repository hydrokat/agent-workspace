# Best Practice: Submission Vouching Pattern

Introduced in phase-016. Describes how delegation of submission approval from admin-only to team leads is implemented safely.

## Authorization Model

`reviewTaskSubmission` (in `lib/actions/submissions.ts`) supports two reviewer types:

1. **Admin** — can review any submission, including their own.
2. **Team Lead** — can review submissions for their team's tasks only, but **not their own** submission.

The caller's identity is resolved from `getAuthenticatedUser()`. Authorization is checked against `teams.team_lead_id`.

## Self-Vouch Block

A non-admin team lead cannot approve or reject a submission they submitted themselves (`task_participations.user_id === reviewer.id`). The check is applied:

1. In the **server action** (fast rejection with a human-readable message).
2. Inside the **`approve_task_submission` SECURITY DEFINER RPC** (defense in depth — the DB must not trust the caller's claimed reviewer identity).

Both layers must be kept in sync.

## Reviewer Audit Trail

The `task_submissions` table has `reviewed_by UUID` and `reviewed_at TIMESTAMPTZ` columns (added in the phase-016 migration). These are populated on both approve (inside the RPC) and reject (in the server action update). This provides a full audit trail for all submissions.

## UI: Self-Submission Indicator

In `app/submissions/lead-submissions-table.tsx`, submissions where `isSelfSubmission === true` are shown with a `ShieldAlert` icon and "Admin Required" label. Approve/Decline buttons are disabled for these rows. Admins can still act via `/admin/submissions`.

## RPC Parameter Change

The RPC `approve_task_submission` uses `p_reviewer_id` (not `p_admin_id`). Update any future callers accordingly.

## Team-Lead Submission Page

`app/submissions/page.tsx` — server component that:
1. Verifies the user leads at least one team (redirects otherwise).
2. Calls `getMyTeamPendingSubmissions()` which post-filters by `team_id` membership.
3. Passes submissions to `<LeadSubmissionsTable>` with `isSelfSubmission` flag pre-computed.

## Security Checklist
- Authorization: `isAdmin()` OR `team_lead_id === reviewer.id` checked server-side before any DB mutation.
- Self-vouch: blocked at server action layer AND inside SECURITY DEFINER RPC.
- IDOR: submission id is hashid-decoded (`decodeId('submission', ...)`) before any query.
- RLS: team lead UPDATE policy on `task_submissions` scoped to their team (via join to `tasks.team_id`).
