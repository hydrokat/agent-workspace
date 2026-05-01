# Business Flow: Core Contribution Loop

This flow describes the lifecycle of an event contribution, from task creation to reward redemption.

## 1. Task Creation & Setup
- **Actor**: Admin
- **Action**: Create committees and tasks.
- **Details**: Tasks include base points, committee ownership, and role capacities (Lead, Contributor, Assistant).

## 2. Task Claiming
- **Actor**: Member
- **Action**: Claim a task with a specific role.
- **Roles**:
    - **Lead**: 100% points
    - **Contributor**: 60% points
    - **Assistant**: 30% points
- **Prerequisite**: Admin must approve the participation.

## 3. Work & Submission
- **Actor**: Member
- **Action**: Perform work and submit proof.
- **Output**: Proof of work (link, description, attachment).

## 4. Approval & Points Awarding
- **Actor**: Admin
- **Action**: Review submission and approve.
- **Result**: Points are automatically calculated based on the role and awarded to the member.

## 5. Reward Redemption
- **Actor**: Member
- **Action**: Redeem points for ticket discounts/free tickets once thresholds are met.
- **Thresholds**:
    - 20-39: 30% off
    - 40-69: 50% off
    - 70-99: 80% off
    - 100+: Free ticket
- **Result**: Admin approves the claim, and the reward is issued.
