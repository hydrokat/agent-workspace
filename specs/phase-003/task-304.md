# Task 304: Points Awarding & Review Logic

## Objective
Implement the logic for admins to review submissions and automatically award points upon approval.

## Agent Assignment
- **Primary Agent**: `senior-backend-dev`

## Requirements
- Create `reviewTaskSubmission` Server Action.
- **Arguments**: `submission_id`, `status` (`approved` or `rejected`), `feedback`.
- **Logic for Approval**:
  - Must be an atomic transaction.
  - Update `task_submissions` status to `approved` and set `approved_at`.
  - Fetch `base_points` from `tasks` and `multiplier` from `role_point_configs` (matching the participant's role).
  - Calculate `computed_points`.
  - Insert record into `point_transactions`.
- **Logic for Rejection**:
  - Update `task_submissions` status to `rejected`.
  - Store `admin_feedback`.
- Ensure only Admins can execute this action.

## Deliverables
- [ ] `reviewTaskSubmission` Server Action.
- [ ] Internal helper for points calculation logic.
