# Implementation Plan: Phase 003 - Points Engine

## Phase Objectives
Implement the proof-of-work submission system and the automated points calculation engine based on task roles. This phase ensures that work done by members is verified and correctly credited via an auditable transaction system.

## Architecture
- **Submission Workflow**: Member submits proof (link/text) -> Admin reviews -> Approval triggers points.
- **Points Logic**: Points = `task.base_points` * `role_point_configs.multiplier`.
- **Source of Truth**: `point_transactions` table stores every point event. Computed totals are derived from these transactions.
- **Server-Side Integrity**: All state changes and point calculations MUST occur in Server Actions.

## Tasks

### [Task 301](./task-301.md): Schema & RLS for Submissions & Transactions
- **Agent**: `senior-backend-dev`
- **Goal**: Finalize DB schema and RLS for `task_submissions` and `point_transactions`.
- **Status**: Completed

### [Task 302](./task-302.md): Server Actions for Proof Submission
- **Agent**: `senior-backend-dev`
- **Goal**: Implement `submitTaskProof` with validation (one submission per participation).
- **Status**: Completed

### [Task 303](./task-303.md): Member Proof Submission UI
- **Agent**: `frontend-dev`
- **Goal**: Build the UI for members to submit proof and view submission status.
- **Status**: Completed

### [Task 304](./task-304.md): Points Awarding & Review Logic
- **Agent**: `senior-backend-dev`
- **Goal**: Implement `approveTaskSubmission` which atomically records point transactions.
- **Status**: Completed

### [Task 305](./task-305.md): Admin Submission Review UI
- **Agent**: `frontend-dev`
- **Goal**: Dashboard for admins to review, approve, or reject pending submissions.
- **Status**: Completed

### [Task 306](./task-306.md): Points Calculation Validation
- **Agent**: `qa-engineer`
- **Goal**: Unit and integration tests for points logic and transaction integrity.
- **Status**: Completed
