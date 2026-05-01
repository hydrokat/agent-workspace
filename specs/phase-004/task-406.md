# Task 406: End-to-End Reward Flow Validation

## Objective
Validate the entire lifecycle from point acquisition to reward claiming via automated tests.

## Agent Assignment
- **Primary Agent**: `qa-engineer`

## Requirements
- Create integration test `app-src/__tests__/reward-flow.test.ts`.
- **Test Scenarios**:
  - User starts with 0 points -> Cannot claim 20-point reward.
  - Admin awards 30 points to user -> User can now claim 20-point reward.
  - User claims reward -> Claim status is `pending`.
  - Admin approves claim -> Claim status is `approved`.
  - User's point total is still 30 (points are NOT "spent" but "milestones" reached, unless business logic dictates otherwise - verify with `core-spec.md`).
    - *Correction based on Spec*: Spec implies "points_required", but doesn't explicitly state points are "spent". Re-verify: "User reaches required points -> User submits claim -> Admin approves". Usually HTN points are cumulative.

## Deliverables
- [ ] Integration test file for reward flow.
