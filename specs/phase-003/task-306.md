# Task 306: Points Calculation Validation

## Objective
Ensure the points engine is accurate and handles all role/point combinations correctly.

## Agent Assignment
- **Primary Agent**: `qa-engineer`

## Requirements
- Create test suite in `app-src/__tests__/points-engine.test.ts`.
- **Test Scenarios**:
  - Role: `lead` (multiplier 1.0) with base points 50 -> 50 points.
  - Role: `contributor` (multiplier 0.6) with base points 100 -> 60 points.
  - Role: `assistant` (multiplier 0.3) with base points 100 -> 30 points.
  - Handle rounding (if applicable - default to floor/ceil as per business logic).
  - Verify atomic transaction: points are NOT awarded if the status update fails.
  - Verify idempotency: points cannot be awarded twice for the same submission.

## Deliverables
- [ ] Jest/Vitest test file for points engine.
- [ ] Test report showing all scenarios passing.
