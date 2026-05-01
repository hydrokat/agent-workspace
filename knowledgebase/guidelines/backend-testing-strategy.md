# Backend Testing Strategy

The backend logic, primarily residing in Server Actions, is verified through a rigorous unit testing suite using Jest.

## Testing Stack
- **Framework**: Jest
- **Compiler**: `ts-jest`
- **Mocks**: Manual mocking of the Supabase client and Server Action helpers.

## Core Testing Patterns

### 1. Mocking the Database
All tests must mock the Supabase client to avoid side effects and external dependencies. A helper `supabase-mock.helper.ts` is provided to create a chainable mock query builder.

```typescript
import { createMockSupabase } from '../supabase-mock.helper';

const { mockSupabase, queryBuilder } = createMockSupabase();
(createClient as jest.Mock).mockResolvedValue(mockSupabase);
```

### 2. Guard Logic Verification
Tests must explicitly verify that security guards (like `isAdmin`) are functioning and that unauthorized requests are rejected.

```typescript
it('should throw error if user is not Admin', async () => {
  (isAdmin as jest.Mock).mockResolvedValue(false);
  await expect(deleteTask('id')).rejects.toThrow("Unauthorized");
});
```

### 3. State Transition Validation
For actions involving status changes (e.g., approving a task claim), tests must verify:
1.  The database was updated with the correct status.
2.  `revalidatePath` was called to update the UI cache.
3.  Any side effects (like point awarding) were triggered.

## Status Standards & Standards

We use a unified status vocabulary across all entities (`team_members`, `task_participations`, `task_submissions`):

| Status | Meaning | System Action |
| :--- | :--- | :--- |
| **`Pending`** | Awaiting Admin review. | Default state on creation. |
| **`Approved`** | Verified and accepted. | Triggers points (if submission). |
| **`Rejected`** | Denied by Admin. | Terminal state. |

## Point Calculation Logic (Phase 2 Finding)
Point awarding is calculated at the moment of **Submission Approval**:
`Final Points = Base Task Points * Role Multiplier`

Admins can find these configurations in the `role_point_configs` table.

## Key Files
- `lib/actions/__tests__/*.test.ts`: Test implementations.
- `lib/actions/supabase-mock.helper.ts`: Shared mocking utility.
