# Task 404: Rewards Marketplace UI

## Objective
Build a "Rewards Shop" where members can view and claim prizes.

## Agent Assignment
- **Primary Agent**: `frontend-dev`

## Requirements
- Page: `/rewards` (Member-specific).
- List available rewards from the `rewards` table (where `is_active = true`).
- Each card shows: Name, Description, Point Cost.
- "Claim" button:
  - Enabled if user has sufficient points.
  - Disabled with "Insufficient points" message otherwise.
- Show "My Claims" section with status (`pending | approved | rejected`).

## Deliverables
- [ ] Reward Marketplace page.
- [ ] Reward card component with conditional action states.
