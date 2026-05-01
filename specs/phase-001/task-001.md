# Task 001: Database Schema Design

## Description
Design and implement the initial PostgreSQL schema in Supabase to support the HTN Points System.

## Objectives
- [ ] Create `profiles` table (id, full_name, role, committee_id, points).
- [ ] Create `committees` table (id, name, lead_id).
- [ ] Create `tasks` table (id, title, description, committee_id, base_points, role_limits).
- [ ] Create `task_claims` table (id, task_id, profile_id, role, status).
- [ ] Create `submissions` table (id, task_claim_id, proof_url, notes, status).
- [ ] Create `rewards` table (id, name, point_cost, tier).
- [ ] Set up Row Level Security (RLS) policies for each table.

## Status
- [ ] Pending
