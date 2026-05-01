# Admin Team & Task Management Workflows

Admins have full control over the organizational structure of the platform, including teams, tasks, and member participation.

## 1. Team Management
Teams are the primary grouping for members and tasks.

### CRUD Workflows
-   **Create Team**: Admins define a team name and assign a Team Lead (via `fk_team_lead`).
-   **Manage Members**: Admins review join requests and manage roles. 
    -   *Note: Due to a schema discrepancy, the `team_members` table lacks a `status` column. All joined members are currently treated as active in logic.*
-   **Update/Delete**: Admins can rename teams or delete them entirely.

### Key Actions (`lib/actions/team-members.ts`)
- `requestToJoinTeam`: (Member) Initiates a request.
- `approveMembership`: (Admin) Validates a member.
- `updateTeamMemberRole`: (Admin) Promotes/demotes members.

## 2. Task Lifecycle
Tasks are units of work assigned to teams with specific point values and role capacities.

### Workflow Stages
1.  **Definition**: Admin creates a task with `base_points` and role limits (`max_leads`, `max_contributors`, `max_assistants`).
2.  **Claiming**: Members request a role in the task. Status is `Pending`.
3.  **Deployment Approval**: Admin reviews task claims. Once `Approved`, the member is officially "deployed" to the task.
4.  **Submission**: The member submits a proof of work link. Status is `Pending`.
5.  **Completion Approval**: Admin reviews the proof. Approval triggers the point awarding engine.

### Capacity Enforcement
The system hard-enforces role limits during both the **Claiming** phase (UI check) and the **Approval** phase (Server-side check). If a role is full, new claims or approvals for that role are blocked.

## 3. Approval Queues
Admins should monitor the following queues in the `/admin` dashboard:

-   **Participation Queue**: Found at `/admin/approvals/participation`. Essential for getting members into tasks quickly.
-   **Submission Queue**: Found at `/admin/approvals/submissions`. Critical for awarding points and closing out tasks.

## 4. Points & Multipliers
Admins can adjust the difficulty/reward ratio via:
-   **Base Points**: Set per-task.
-   **Role Multipliers**: Defined globally in `role_point_configs`. (e.g., `Lead` = 1.5x, `Contributor` = 1.0x).

## Summary Table: Admin vs Member Actions

| Action | Admin | Member |
| :--- | :---: | :---: |
| Create Team | ✅ | ❌ |
| Join Team | ✅ (Auto) | 📝 (Request) |
| Create Task | ✅ | ❌ |
| Claim Task | ✅ | 📝 (Request) |
| Approve Proof | ✅ | ❌ |
| Submit Proof | ❌ | ✅ |
