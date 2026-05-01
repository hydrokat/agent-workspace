# Task 101: Supabase Schema Migration

## Description
Design and apply the initial PostgreSQL schema to Supabase. Focus on flexibility and minimal rigidity.

## Objectives
- [ ] Create `committees` table (id, name, description, lead_id).
- [ ] Create `profiles` table (id, full_name, role, committee_id, points).
- [ ] Create `tasks` table (id, title, description, base_points, committee_id, max_lead, max_contributor, max_assistant).
- [ ] Create `task_participants` table (task_id, user_id, role, status).
- [ ] Create `enum` for user roles (admin, goon, apprentice).
- [ ] Create `enum` for task roles (lead, contributor, assistant).

> **Note (2026-05-01)**: Schema names changed during implementation. `committees` → `teams`, `task_participants` → `task_participations`, enum values updated to `'Admin'/'Volunteer'` (user roles) and `'Lead'/'Contributor'/'Assistant'` (task roles). `profiles.committee_id` → `profiles` has no team FK (membership via `team_members` join table). These spec checkboxes reflect the original plan, not current schema.

## Agent Assignment
- **Agent**: `senior-backend-dev`
- **Model**: Gemini 2.0 Pro (Planning) / Flash (Execution)

## Status
- [ ] Pending
