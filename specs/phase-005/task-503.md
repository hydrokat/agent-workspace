# Task 503: Password Reset Middleware & Flow

## Goal
Enforce the `must_reset_password` flag and provide a UI for users to update their credentials.

## Requirements
- Update `middleware.ts` to detect `must_reset_password` from the profile and redirect to `/reset-password` if true (and not already there).
- Create `/reset-password` page with a form to enter a new password.
- Implement `updatePassword` server action that clears the `must_reset_password` flag upon successful change.

## Implementation Details
- **Middleware**: Add check after session validation.
- **Page**: `/app/(auth)/reset-password/page.tsx`.
- **Action**: `lib/actions/auth.ts`.

## Verification
- Login with a user having `must_reset_password = true` and verify redirection.
- Complete the reset and verify flag is cleared and access is restored.
