# Task 303: Member Proof Submission UI

## Objective
Build the frontend interface allowing members to submit proof for their assigned tasks.

## Agent Assignment
- **Primary Agent**: `frontend-dev`

## Requirements
- Create a submission form component using `shadcn/ui`.
- Integrated into the task details page or a dedicated `/tasks/[id]/submit` route.
- Input field for `proof` (textarea for links/description).
- Display current submission status (Pending/Approved/Rejected) and admin feedback if available.
- Proper loading states using `useFormStatus` or `useTransition`.
- Adhere to `knowledgebase/guidelines/ui-design-guideline.md`.

## Deliverables
- [ ] Submission form component.
- [ ] Updated Task Details page with submission status/action.
