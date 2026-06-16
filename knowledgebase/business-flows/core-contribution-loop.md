# Business Flow: Core Contribution Loop

This flow describes the lifecycle of an event contribution, from task creation to reward redemption.

## 1. Task Creation & Setup
- **Actor**: Admin
- **Action**: Create tasks assigned to a team, selecting a **category**.
- **Details**: Tasks include base points, team ownership, role capacities (Lead, Contributor, Assistant), and a required category. A task must always belong to exactly one category. If no category fits, use the seeded "General" category.

## 2. Task Claiming
- **Actor**: Member
- **Action**: Claim a task with a specific role.
- **Roles**:
    - **Lead**: 100% points
    - **Contributor**: 60% points
    - **Assistant**: 30% points
- **Prerequisite**: User must be an approved member of the task's team.
- **Cap enforcement**: Before the claim is accepted, the system computes the user's accumulated awarded points for the task's category within the current reset window (day/week/month). If `accumulated >= point_cap` (and `point_cap > 0`), the claim is **blocked** with a friendly message. Cap enforcement runs at claim time only (not retroactively). Admin approval of the participation is still required after claiming.

## 3. Work & Submission
- **Actor**: Member
- **Action**: Perform work and submit proof once participation is approved.
- **Output**: Proof of work (link, description, attachment).

## 4. Approval & Points Awarding
- **Actor**: Admin **or** Team Lead (vouching)
- **Action**: Review submission and approve or reject.
- **Team lead vouching rules**:
  - A team lead may approve/reject submissions for their own team's tasks.
  - A team lead **may NOT approve their own submission** — an admin must handle those ("self-vouch block"). Self-submissions appear with a "Admin Required" indicator.
  - The self-vouch block is enforced both in the server action and inside the `SECURITY DEFINER` RPC (defense in depth).
  - The reviewer is recorded in `task_submissions.reviewed_by` / `reviewed_at`.
- **Result**: Points are automatically calculated based on the role and awarded to the member via `approve_task_submission` RPC.

## 5. Reward Redemption
- **Actor**: Member
- **Action**: Redeem points for ticket discounts/free tickets once thresholds are met.
- **Thresholds**:
    - 20-39: 30% off
    - 40-69: 50% off
    - 70-99: 80% off
    - 100+: Free ticket
- **Result**: Admin approves the claim, and the reward is issued.
