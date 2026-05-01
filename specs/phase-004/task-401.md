# Task 401: Point Aggregation & Dashboard Foundation

## Objective
Implement a robust and performant way to calculate total points for a user from their transaction history.

## Agent Assignment
- **Primary Agent**: `senior-backend-dev`

## Requirements
- Create a helper function/Server Action `getUserTotalPoints(userId)` in `lib/actions/points.ts`.
- **Logic**: Sum `computed_points` from `point_transactions` where `user_id` matches.
- Consider creating a Database View `user_points_summary` in Supabase for easier fetching:
  ```sql
  CREATE VIEW user_points_summary AS
  SELECT user_id, SUM(computed_points) as total_points
  FROM point_transactions
  GROUP BY user_id;
  ```
- Ensure RLS on the view (or underlying table) allows users to only see their own totals.

## Deliverables
- [ ] SQL for view or Server Action for aggregation.
- [ ] Integration into `profiles` fetch logic if needed.
