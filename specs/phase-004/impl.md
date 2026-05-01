# Implementation Plan: Phase 004 - Rewards & Dashboards

## Phase Objectives
Gamify the experience with progress tracking and enable point redemption for rewards. This phase focuses on the "Redeem" part of the "Contribution -> Points -> Rewards" loop.

## Architecture
- **Aggregation**: Total points are calculated on-the-fly via Server Components (sum of `point_transactions`).
- **Reward Tiers**: Configurable thresholds (20, 40, 70, 100 points) for visual milestones.
- **Redemption Flow**: User checks eligibility -> User claims reward -> Admin approves/rejects.
- **Auditable Redemption**: Every claim is tracked in `reward_claims`.

## Tasks

### [Task 401](./task-401.md): Point Aggregation & Dashboard Foundation
- **Agent**: `senior-backend-dev`
- **Goal**: Implement server-side point summation and total point display logic.
- **Status**: Completed

### [Task 402](./task-402.md): Member Points & Task Dashboard
- **Agent**: `frontend-dev`
- **Goal**: Build the main member dashboard showing points, progress, and task history.
- **Status**: Completed

### [Task 403](./task-403.md): Reward Redemption Backend
- **Agent**: `senior-backend-dev`
- **Goal**: Implement `claimReward` Server Action with eligibility validation.
- **Status**: Completed

### [Task 404](./task-404.md): Rewards Marketplace UI
- **Agent**: `frontend-dev`
- **Goal**: UI for members to browse available rewards and submit redemption claims.
- **Status**: Completed

### [Task 405](./task-405.md): Admin Reward & Claim Management
- **Agent**: `frontend-dev`
- **Goal**: UI for admins to manage rewards and approve/reject member claims.
- **Status**: Completed

### [Task 406](./task-406.md): End-to-End Reward Flow Validation
- **Agent**: `qa-engineer`
- **Goal**: Automated tests for the full points-to-reward-redemption lifecycle.
- **Status**: Completed

### [Task 407](./task-407.md): Security Audit of Redemption System
- **Agent**: `security-auditor`
- **Goal**: Review and harden RLS and Server Actions against point/reward fraud.
- **Status**: Completed
