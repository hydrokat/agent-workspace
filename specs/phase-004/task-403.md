# Task 403: Reward Redemption Backend

## Objective
Implement the logic for members to claim rewards once they have enough points.

## Agent Assignment
- **Primary Agent**: `senior-backend-dev`

## Requirements
- Create `claimReward` Server Action in `lib/actions/rewards.ts`.
- **Logic**:
  - Authenticate user.
  - Fetch user's `total_points` (sum of transactions).
  - Fetch target `reward.points_required`.
  - Validate: `total_points >= points_required`.
  - Check if user has already claimed this specific reward (if it's a one-time reward).
  - Insert record into `reward_claims` with `status = 'pending'`.
- Use Zod for input validation.

## Deliverables
- [ ] `lib/actions/rewards.ts` with `claimReward` action.
- [ ] Zod schema for reward claim input.
