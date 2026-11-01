---
name: pm-commands
description: Use when the user asks what sprint commands exist, which /pm-* commands are usable, or invokes /pm-commands. Reads current sprint state through sprints.py commands and displays all nine /pm-* commands with real-time usability badges. Read-only.
---

# PM Commands Catalog

This skill displays the complete catalog of all nine `/pm-*` commands, highlighting which commands are **usable right now** based on the workspace's active sprint state.

## Step 0: Parse The Request

| Invocation | Scope |
|---|---|
| `/pm-commands` | All `/pm-*` commands with real-time state annotations |

## Step 1: Load The Data

```bash
python3 .agents/skills/pm-new-sprint/scripts/sprints.py commands
```

## Step 2: Map The Data To The Template

Fill `assets/commands.md` from `sprints.py commands`:

<!-- data-map -->
| Section | Source |
|---|---|
| Current Sprint State | `active_status` (`None` / `open` / `locked`) |
| Commands Table | `commands` (`command`, `title`, `description`, `usable`, `reason`) |
<!-- /data-map -->

Mark each command with:
- `[USABLE]` in green/bold when `usable` is `true`.
- `[UNAVAILABLE]` when `usable` is `false`, showing the requirement/precondition in the reason column.

## Step 3: Present

Print the commands table in chat. Highlight the primary recommended next action given the active state.

## Mandates

- **Read-Only**: Never edit sprint or spec files.
- **Script-First**: Command usability and active status come directly from `sprints.py`.
