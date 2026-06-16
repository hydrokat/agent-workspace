# Task 007: "My Tasks" tab in the task list

## Description
Add a **"My Tasks"** tab to the task list that displays all tasks **assigned to the current
user** (i.e. tasks the user participates in, regardless of role/status).

## Existing Implementation & Guidelines
- `app/tasks/page.tsx` — server component; already loads `getMyParticipations()` (from
  `lib/actions/task-participants.ts`) and passes `myParticipations` to the client. No new
  server fetch is required.
- `app/tasks/task-board-client.tsx` — already uses `Tabs`/`TabsList`/`TabsTrigger`/`TabsContent`
  with tabs **All Tasks**, **Available**, **Unit Specific**, and already derives
  `myTaskIds = new Set(myParticipations.map(p => p.task_id))`. The "Available" tab already
  filters `!myTaskIds.has(t.id)`, so "My Tasks" is the complement.
- `TaskCard` already accepts `isClaimed={myTaskIds.has(task.id)}`.
- Guidelines: client-side filtering of already-server-provided data is fine here (no new DB
  access); keep styling consistent with the existing `TabsTrigger` classes.

## Approach
1. Add a `<TabsTrigger value="my-tasks">My Tasks</TabsTrigger>` matching the existing tab
   styling.
2. Add a matching `<TabsContent value="my-tasks">` rendering
   `filteredTasks.filter((t) => myTaskIds.has(t.id))` as `TaskCard`s (respecting the current
   `searchQuery` filter, like the other tabs).
3. Handle the **empty state** (user has no assigned tasks) with copy consistent with the board's
   existing empty/again styling.
4. (Optional, decide in UI) Within "My Tasks", visually group or label by participation status
   (Pending / Approved) using data already in `myParticipations` — only if it doesn't add a new
   fetch.

## Objectives
- [ ] "My Tasks" tab present in the task board, styled consistently
- [ ] Lists exactly the tasks the current user participates in (`myTaskIds`)
- [ ] Respects the existing search filter; sensible empty state
- [ ] No new server fetch (reuses `myParticipations` already passed to the client)

## Unit Testing
- [ ] Jest (Docker): given participations + tasks, the My Tasks filter returns only
      participated tasks; empty when none; search narrows results. (Component/logic test using
      the existing testing-library setup.)

## Security Audit
- [ ] No new data exposure — uses only the current user's already-authorized `myParticipations`
      and the same task list other tabs render (no other-user data, no new endpoint).
- [ ] No client-side DB access introduced.

## Status
- [ ] Pending
