# Task 302: Server Actions for Proof Submission

## Objective
Implement the server-side logic for members to submit proof of work for their approved task participations.

## Agent Assignment
- **Primary Agent**: `senior-backend-dev`

## Requirements
- Create `submitTaskProof` Server Action in `lib/actions/submissions.ts`.
- **Logic**:
  - Authenticate user via `auth.uid()`.
  - Validate that `task_participation_id` belongs to the user and has `status = 'approved'`.
  - Check if a submission already exists for this participation (prevent duplicates).
  - Insert record into `task_submissions` with `status = 'pending'`.
- Use Zod for input validation.
- Handle error states (unauthorized, invalid participation, DB errors).

## Deliverables
- [ ] `lib/actions/submissions.ts` with `submitTaskProof` action.
- [ ] Zod schema for submission input.
