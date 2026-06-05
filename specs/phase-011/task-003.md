# Task 003: `/admin/admins` page + invite dialog UI

## Description
Build a new admin-management surface listing current admins and providing an "Invite Admin" dialog.
The page is a Server Component (data via `listAdmins()`); the dialog is a Client Component calling the
`inviteAdmin` Server Action. Visuals conform to the Nexus aesthetic ("Cold, Dark, and Fast").

## Reference
- `knowledgebase/guidelines/nexus-ui.md`, `knowledgebase/guidelines/ui-design-guideline.md`
- Pattern to mirror: `components/admin/add-member-dialog.tsx` (dialog + invite tab + states)
- Existing admin pages: `app/admin/config`, `app/admin/teams`
- i18n: `lib/i18n/` (route all user-facing copy through dictionaries, both `en` and `en-simple`)

## Where
- New: `app/admin/admins/page.tsx` (Server Component; `isAdmin()` guard, list render)
- New: `components/admin/invite-admin-dialog.tsx` (Client Component)
- Add nav entry to the admin section as appropriate.

## Objectives
- [x] Server Component page: enforce admin access (redirect/forbidden for non-admins), render the
      admin list from `listAdmins()` (name, email, status — show "Pending Setup" when
      `must_complete_onboarding`).
- [x] Invite dialog: email input, validation, submit → `inviteAdmin`; success + error states; pending spinner.
- [x] On success show confirmation that an invitation email was sent (no password is shown — this differs
      from `add-member-dialog`'s temp-password flow).
- [x] All copy sourced from i18n dictionaries (`en` + `en-simple` parity).
- [x] Responsive at 375 / 768 / 1280px; `rounded-none` geometry; high-contrast palette.

## Acceptance Criteria
- Page is reachable only by admins; lists existing admins with their setup state.
- Invite dialog sends an invitation and surfaces success/error clearly without leaking internals.
- No hardcoded user-facing strings; dictionary parity maintained.

## Validation
- Manual UI walkthrough at all breakpoints.
- Component/dictionary tests covered in Task 006.

## Status
- [x] Completed
