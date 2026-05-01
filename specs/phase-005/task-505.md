# Task 505: Member Selection & Invitation UI

## Goal
Implement the user interface for searching and inviting personnel.

## Requirements
- Create an `AddMemberDialog` component.
- **Search Tab**: Input field that triggers `searchProfiles` and lists results with "Add" buttons.
- **Invite Tab**: Email input field for new accounts with a "Send Invitation" button.
- Handle loading and success/error states with clear feedback.

## Implementation Details
- **Components**: `AddMemberDialog`, `ProfileSearchResult`, `InvitationForm`.
- **State Management**: Use `useActionState` or similar for form handling.

## Verification
- Search for existing users and add them to the team.
- Submit a new email and verify the invitation flow triggers.
