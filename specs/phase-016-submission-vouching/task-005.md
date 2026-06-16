# Task 005: Submission vouching (team-lead validation)

## Description
Allow a **team lead** to vouch for (validate/confirm) a proof of submission for their team's
tasks — awarding points exactly as an admin approval does. A team lead **may not vouch for
their own submission**; those must be handled by an admin. Admins retain authority over **all**
submissions.

## Existing Implementation & Guidelines
- `lib/auth/guards.ts:isTeamLead(teamId)` — Admin OR `teams.team_lead_id === user.id`. SQL
  `is_admin()`; team lead is `teams.team_lead_id`.
- `lib/actions/submissions.ts:reviewTaskSubmission(submission_id, status, feedback)` — currently
  `isAdmin()`-only; approve calls RPC `approve_task_submission(p_submission_id, p_admin_id)`.
- RPC `supabase/migrations/20260502120200_update_approve_submission_rpc.sql` (`SECURITY DEFINER`)
  — computes points and inserts `point_transactions`.
- RLS on `task_submissions` (phase-3 points engine migration): admins can SELECT/UPDATE all;
  users can view own. No team-lead policy yet.
- Join path for "which team owns this submission": submission → participation → task → team.
- Guidelines: server-side-only; `actionError`; `SECURITY DEFINER` functions must re-check authz
  internally (never trust caller); hashid integer-only for submission ids (phase-015 task-003
  fixes the approve/reject id consistency — build on that).

## Approach
1. **Authorization** in `reviewTaskSubmission`: allow when `isAdmin()` **OR**
   `isTeamLead(teamId)` for the submission's task team. Resolve `team_id` via the join path.
2. **Self-vouch block:** if the actor is a (non-admin) team lead and the submission's
   participation `user_id === actor`, reject with a clear message: "You cannot vouch for your
   own submission — an admin must review it." Admins are exempt.
3. **RPC update:** rename `p_admin_id` → `p_reviewer_id` (acting lead/admin) and add an
   **internal authorization guard** in the `SECURITY DEFINER` function: verify the reviewer is
   admin or the team lead of the submission's team, and is not the submitter. Persist the
   reviewer for audit (e.g. `task_submissions.reviewed_by UUID` + `reviewed_at`). Keep the
   reject path symmetric (record reviewer/feedback).
4. **RLS:** add team-lead `SELECT`/`UPDATE` policies on `task_submissions` scoped to their
   team's tasks (submission → participation → task → team where `team_lead_id = auth.uid()`),
   excluding their own submissions for the write policy. Keep admin-all policies.
5. **UI:** expose pending submissions for a team lead's team(s) — extend the submissions view or
   add a lead-facing route reachable by leads. Render the lead's own team submissions where they
   are the submitter as "Requires admin review" (action disabled). Inline error display
   (consistent with phase-015 task-003).

## Objectives
- [ ] Team lead can approve (vouch) and reject submissions for their team → points awarded on approve
- [ ] Team lead **cannot** act on their own submission (admin-only); admin can act on all
- [ ] RPC re-checks authorization internally and records `reviewed_by`/`reviewed_at`
- [ ] RLS lets leads SELECT/UPDATE only their team's (non-self) submissions; admin-all retained
- [ ] Lead-facing UI lists their team's pending submissions; self-submissions disabled with copy
- [ ] Errors surfaced inline (no boundary crash, no leakage)

## Unit Testing
- [ ] Jest (Docker): lead approves teammate's submission → points awarded; lead rejects → status
      updated; lead acting on own submission → blocked; lead acting on another team's submission
      → blocked; admin can act on any (incl. lead's own); reviewer recorded.

## Security Audit
- [ ] Authorization enforced in BOTH the server action AND the `SECURITY DEFINER` RPC (defense
      in depth; RPC must not trust the caller-supplied reviewer id — derive/verify from auth).
- [ ] Self-vouch impossible for non-admin leads at action + RPC + RLS layers.
- [ ] No IDOR: a lead cannot vouch across teams or escalate via crafted submission ids
      (hashid-decoded, integer-only).
- [ ] RLS policies reviewed for the join-path scoping; no over-broad UPDATE.
- [ ] `actionError` normalization; no DB internals leaked.
- [ ] Run `security-auditor` agent (and consider `api-pentester`) on the diff.

## Status
- [ ] Pending
