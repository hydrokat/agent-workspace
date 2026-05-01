# Implementation Plan: Phase 002 - Task Lifecycle & Team Structure

## Phase Objectives
Enable the core workflow of creating tasks, claiming roles, and managing participation approvals. This phase also implements the multi-team membership model and the task submission (Proof of Work) workflow.

## Architecture
- **Multi-Team Membership**: Transition from a single `committee_id` on profiles to a dedicated `team_members` many-to-many relationship.
- **Task Management**: Admin-only CRUD via Server Actions.
- **Claiming System**: Real-time role availability checks and approval status management.
- **Task Submissions**: A flow for members to provide proof of work and for admins to review/approve.
- **Permissions**: Admins manage all tasks and teams; Members interact with tasks belonging to teams they are members of.

## Timeline
- **Task 201**: Admin Task Management UI (Done)
- **Task 202**: Member Task Discovery & Dashboard (Done)
- **Task 203**: Role Claiming & Approval Workflow (Done)
- **Task 204**: Multi-Team Membership Schema & Actions (Done)
- **Task 205**: Task Submission (Proof of Work) Implementation (Done)
- **Task 206**: Submission Review UI for Admins (Done)
- **Task 207**: Final UI Polish for Phase 2 (Done)

## Tasks
- [x] Task 201: Admin Task Management UI
- [x] Task 202: Member Task Discovery & Dashboard
- [x] Task 203: Role Claiming & Approval Workflow
- [x] Task 204: Multi-Team Membership Schema & Actions
- [x] Task 205: Task Submission (Proof of Work) Implementation
- [x] Task 206: Submission Review UI for Admins
- [x] Task 207: Final UI Polish for Phase 2

## Implementation Notes
- **Schema Migration**: Task 204 involves creating `team_members` and migrating existing `profiles.committee_id` data if applicable, then removing the column.
- **Submissions**: `task_submissions` should link to `task_participations`. Only approved participants can submit proof.
- **Points**: While full point engine is Phase 3, Task 206 sets the stage by allowing admins to "Approve" a submission, which will eventually trigger point awarding.
- Use `revalidatePath` to keep the task board, dashboards, and admin lists in sync.
- Role limits are soft-checked in UI but must be hard-checked in `claimTask` server action.