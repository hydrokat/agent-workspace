# Security Patterns: Admin Authorization

To ensure the integrity of administrative actions, a consistent security pattern must be applied across all server actions and API routes.

## The `isAdmin` Helper

The primary mechanism for verifying administrative privileges is the `isAdmin()` helper function, located in `lib/actions/teams.ts`.

### Implementation
```typescript
export async function isAdmin(): Promise<boolean> {
  const supabase = await createClient();
  const { data: { user } } = await supabase.auth.getUser();
  
  if (!user) return false;

  const { data: profile } = await supabase
    .from('profiles')
    .select('role')
    .eq('id', user.id)
    .single();

  return profile?.role === 'Admin';
}
```

## Pattern: Guarding Server Actions

Every server action that performs a restricted operation (Create, Update, Delete, or Approve) MUST begin with an `isAdmin()` check.

### Example
```typescript
export async function deleteTask(id: string) {
  if (!(await isAdmin())) {
    throw new Error("Unauthorized: Only Admins can delete tasks");
  }
  
  // Implementation...
}
```

## Multi-Layered Security

Administrative security is enforced at three levels:

1.  **UI Layer**: Navigation links to `/admin` are hidden from non-admin users.
2.  **Server Action Layer**: Explicit `isAdmin()` checks prevent unauthorized execution of backend logic.
3.  **Database Layer (RLS)**: Row Level Security policies in PostgreSQL ensure that even if a server action is bypassed, the database itself will reject the operation.

## Status Standards

Administrative workflows rely on a standard state machine for approvals:

-   **`Pending`**: The initial state for user-submitted requests (Join Team, Claim Task, Submit Proof).
-   **`Approved`**: The active/terminal state. For task submissions, this triggers point awarding.
-   **`Rejected`**: A terminal state for denied requests.

## Key Files
- `lib/actions/teams.ts`: Contains the `isAdmin` helper.
- `supabase/migrations/20240520000011_rls_policies.sql`: Database-level enforcement.
