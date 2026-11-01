---
name: pm-sprint-status
description: Use when the user asks for sprint status, progress, active development claims, blockers, or ETA, or invokes /pm-sprint-status. Reads live sprint progress and claims through sprints.py status and presents an honest dashboard with sequential vs parallel ETAs. Read-only.
---

# PM Sprint Status

This skill presents an honest, real-time dashboard of sprint progress, active claims/tasks, remaining scope ordered by MoSCoW and RICE, and realistic AI-assisted human ETAs.

## Step 0: Parse The Request

| Invocation | Scope |
|---|---|
| `/pm-sprint-status` | Active sprint (`open` or `locked`) |
| `/pm-sprint-status s-001` (also `001`, `1`, or full directory name) | That specific sprint |

## Step 1: Load The Data

```bash
python3 .agents/skills/pm-new-sprint/scripts/sprints.py status [<sprint>]
```

If no active sprint exists and none was specified, report that no active sprint is found and suggest `/pm-sprint-list` or `/pm-new-sprint`.

## Step 2: Map The Data To The Template

Fill `assets/status.md` from `sprints.py status`:

<!-- data-map -->
| Section | Source |
|---|---|
| Sprint & Status | `sprint`, `status` |
| Progress Bar | Calculated from done/total: `█` per tenth and `░` for remainder |
| In Development | `in_development` (`phase`, `task`, `title`, `claimed`) |
| Remaining Scope | `remaining_by_moscow` (`Must Have`, `Should Have`, `Could Have`, `Won't Have`) |
| ETA & Blockers | `eta` (`sequential_hours`, `sequential_days`, `parallel_waves_hours`, `parallel_waves_days`), blockers |
<!-- /data-map -->

## Step 3: Present

Print the dashboard in chat. If pending tasks remain, offer `/pm-sprint-implement`.

## Mandates

- **Read-Only**: Never edit sprint files or coordination claims.
- **Realism**: Always report the honest remaining time and flag blocked or overloaded items.
- **Script-First**: All counts, claims, and ETAs come directly from `sprints.py`.
