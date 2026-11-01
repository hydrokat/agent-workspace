---
name: pm-sprint-list
description: Use when the user asks to list sprints, view active or planned sprints, or invokes /pm-sprint-list. Reads sprint list and spec phases through sprints.py list and displays active, planned, and closed sprints. Read-only.
---

# PM Sprint List

This skill lists all sprints in the workspace: the active sprint (`open` or `locked`), upcoming `planned` sprints, and closed sprint history.

## Step 0: Parse The Request

| Invocation | Scope |
|---|---|
| `/pm-sprint-list` | Active sprint and pending/planned sprints |
| `/pm-sprint-list --all` | Full sprint history including closed sprints |

## Step 1: Load The Data

```bash
python3 .agents/skills/pm-new-sprint/scripts/sprints.py list
```

If no sprints exist in `sprints/`, report that no sprints have been created yet and suggest `/pm-new-sprint`.

## Step 2: Map The Data To The Template

Fill `assets/list.md` from `sprints.py list`:

<!-- data-map -->
| Section | Source |
|---|---|
| Active Sprint | Sprint where `status` is `open` or `locked` |
| Planned Sprints | Sprints where `status` is `planned` |
| Closed Sprints | Sprints where `status` is `closed` (shown with `--all` or when requested) |
| Metrics | `phases_count`, `committed_effort`, `capacity_days` |
<!-- /data-map -->

## Step 3: Present

Print the sprint list in chat. If an active sprint exists, suggest `/pm-sprint-explain` or `/pm-sprint-status`. If no active sprint exists, suggest `/pm-new-sprint`.

## Mandates

- **Read-Only**: Never edit sprint or spec files.
- **Script-First**: All sprint states and phase counts come from `sprints.py`.
