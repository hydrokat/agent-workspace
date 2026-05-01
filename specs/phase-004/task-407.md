# Task 407: Security Audit of Redemption System

## Objective
Audit the points and rewards system for any security vulnerabilities or logic flaws.

## Agent Assignment
- **Primary Agent**: `security-auditor`

## Requirements
- Review all RLS policies for `point_transactions`, `rewards`, and `reward_claims`.
- Audit Server Actions in `lib/actions/points.ts` and `lib/actions/rewards.ts` for authorization checks.
- **Vulnerability Checks**:
  - Can a user claim a reward by spoofing their point total? (Ensure total is calculated server-side).
  - Can a user approve their own reward claim?
  - Can a user insert their own `point_transactions`?
  - Race conditions in `claimReward` (e.g., claiming multiple rewards rapidly before points are "checked").

## Deliverables
- [ ] Security audit report.
- [ ] (Optional) PR for any identified hardening measures.
