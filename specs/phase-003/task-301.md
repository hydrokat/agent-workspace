# Task 301: Schema & RLS for Submissions & Transactions

## Objective
Ensure the database schema for task submissions and point transactions is fully optimized and secured with Row Level Security (RLS) policies.

## Agent Assignment
- **Primary Agent**: `senior-backend-dev`
- **Reviewer**: `security-auditor`

## Requirements
- Verify `task_submissions` table structure:
  - `id (UUID, PK)`
  - `task_participation_id (UUID, FK -> task_participations)`
  - `proof (TEXT)` (URL or description)
  - `status (VARCHAR)` - `pending | approved | rejected`
  - `admin_feedback (TEXT, optional)`
  - `created_at`, `updated_at`, `approved_at`
- Verify `point_transactions` table structure:
  - `id (UUID, PK)`
  - `user_id (UUID, FK -> profiles)`
  - `task_participation_id (UUID, FK -> task_participations, optional)`
  - `base_points (INTEGER)`
  - `multiplier (DECIMAL)`
  - `computed_points (INTEGER)`
  - `source (VARCHAR)` - `task | bonus | adjustment`
  - `created_at`
- Implement RLS Policies:
  - `task_submissions`:
    - Members can `INSERT` for their own `task_participation_id`.
    - Members can `SELECT` their own submissions.
    - Admins can `SELECT`, `UPDATE` (status/feedback) all submissions.
  - `point_transactions`:
    - Members can `SELECT` their own transactions.
    - Admins can `SELECT` all, `INSERT` (for adjustments/bonuses).
    - No one can `UPDATE` or `DELETE` transactions (audit log integrity).

## Deliverables
- [ ] SQL Migration file for schema refinements and RLS policies.
- [ ] Verified schema in Supabase.
