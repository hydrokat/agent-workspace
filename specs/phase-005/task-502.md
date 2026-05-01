# Task 502: Server Actions: Profile Search & Team Invitation

## Goal
Implement the core logic for searching profiles and adding members to teams, handling both existing profiles and new email invitations. **This task must only be executed after Task 501's migration is fully deployed.**

## Requirements
- Implement `searchProfiles(query: string)` to search by name or email, restricted to admin users.
- Implement `addTeamMember(teamId, profileId)` for existing users.
- Implement `createAndAddTeamMember(teamId, email)` for new users.
  - Generate a secure random password.
  - Create Supabase Auth user via Admin API.
  - Set `must_reset_password = true` on the created profile (relies on Task 501).
  - Link the user to the team in `team_members`.
- Ensure atomic operations using transactions or RPC if possible.

## Implementation Details
- **Location**: `lib/actions/profiles.ts` (for search) and `lib/actions/team-members.ts` (for invitations).
- **Security**: Use `supabase/admin.ts` (Service Role) client to perform privileged actions such as fetching user data across RLS boundaries or creating new Auth accounts.

## Verification
- Unit test `searchProfiles` with various queries.
- Unit test `addTeamMember` with valid/invalid IDs.
- Unit test `createAndAddTeamMember` and verify auth user creation + profile flag.
