# Task 504: Team Detail Page & Registry

## Goal
Provide a dedicated management view for individual teams.

## Requirements
- Create `/admin/teams/[id]` page.
- Display team metadata (Name, Total Points, etc.).
- List all current team members in a table/grid.
- Include "Add Member" trigger button.
- Follow Nexus design system (high-contrast, industrial).

## Implementation Details
- **Page**: `/app/admin/teams/[id]/page.tsx`.
- **Components**: `TeamHeader`, `MemberTable`.

## Verification
- Navigate to a team and verify all data loads correctly.
- Check responsiveness and design alignment with Phase 3/4 aesthetics.
