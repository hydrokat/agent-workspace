---
name: pm-backlogs
description: Use when the user asks for the backlog, unscheduled specs or tasks, leftover work, or invokes /pm-backlogs. Scans backlog items through sprints.py backlog and guides folding into an active sprint or planning a new sprint.
---

# PM Backlogs

This skill scans the workspace for incomplete specs and tasks that are not yet committed to the active sprint, presenting the backlog and guiding the user to either fold chosen items into an open sprint or plan a future sprint.

## Step 0: Parse The Request

| Invocation | Scope |
|---|---|
| `/pm-backlogs` | Scan all backlog items (specs and tasks) against the active sprint |
| `/pm-backlogs specs` | Scan only spec-level backlog summary |
| `/pm-backlogs tasks` | Scan flat task-level backlog |
| `/pm-backlogs all <sprint>` | Scan backlog evaluating against a specific target sprint |

Default scope is `all`.

## Step 1: Load The Data

Run the backlog scanner:

```bash
python3 .agents/skills/pm-new-sprint/scripts/sprints.py backlog [--scope specs|tasks|all] [<sprint>]
```

If the returned backlog has zero phases and zero tasks, report that no unscheduled or leftover items remain, and stop.

## Step 2: Map The Data To The Template

Fill `assets/backlog.md` from `sprints.py backlog`:

<!-- data-map -->
| Section | Source |
|---|---|
| Target Sprint Status | `target` (`sprint`, `status`, `capacity`, `committed`, `free`, `can_add`) |
| Unscheduled Scope | `phases` and `tasks` with `source: unscheduled` |
| Outside-Subset Scope | `phases` and `tasks` with `source: outside-subset` |
| Deferred Scope | `phases` and `tasks` with `source: deferred` |
<!-- /data-map -->

Print the formatted backlog summary in chat along with the target sprint's remaining capacity (`free` days).

## Step 3: Select Scope

Prompt the user using multiple-choice questions in rounds of at most four options plus 'Show more', accepting free text such as `p-014` or `p-014 t-003,t-005`:
- Selecting an entire phase commits all incomplete tasks of that phase.
- Selecting specific task IDs creates a phase task subset.

## Step 4: Choose Destination

Ask the user where the selected items should go:
1. **Fold into current sprint**: Available when `target.can_add` is `true`.
   - If the active sprint is `locked`, explain that locked sprints are frozen against scope creep and require a formal hotfix reason (`--hotfix --reason "<reason>"`).
2. **Plan a new sprint**: Hands off the selection to `/pm-new-sprint`.
3. **Nothing / Cancel**: Exit without modifying any files.

## Step 5: Prioritize & Check Capacity

For each selected phase:
- Determine MoSCoW priority (default: `Should Have`).
- Use default RICE scores: Reach 100, Impact 1.0, Confidence 0.8, Effort calculated at 0.5 days per task (or from scan).
- Enforce the sprint capacity gate:
  - Standard budget: 5.0 days AI-assisted human capacity.
  - Limit: at most 3 phases and 15 tasks.
  - Must Have tasks must not exceed 70% of total capacity.
  - If the selection exceeds capacity, display the overload warning and require an explicit "yes, overload" response from the user before proceeding.

## Step 6: Single Confirmation & Execution

Present a single, clear confirmation summary detailing:
- Phases and specific task subsets to be added
- Destination sprint and new committed effort
- Any warnings (such as `delivery_excluded: true` when a subset omits the final delivery task)

Upon explicit user confirmation:
- **Folding into sprint**:
  For each selected phase/subset, run:
  ```bash
  python3 .agents/skills/pm-new-sprint/scripts/sprints.py add-phase <sprint> <phase> [--tasks <task_ids...>] [--with-deps] [--moscow <moscow>]
  ```
- **Planning new sprint**:
  Hand off to `pm-new-sprint`, creating the sprint with `--status planned` if another sprint is currently open or locked.

## Step 7: Verify & Follow-up

1. Validate sprint consistency:
   ```bash
   python3 .agents/skills/pm-new-sprint/scripts/sprints.py lint <sprint>
   ```
2. Report the updated sprint commitments and suggest running `/pm-sprint-status`.

## Mandates

- **Read-Only Until Confirmation**: Perform no write operations before the user's explicit confirmation in Step 6.
- **Spec & Task Integrity**: Never edit spec files (`impl.md`) or task files (`t-###.md`). Only `sprints.py` writes `sprint.md` (single-writer principle).
- **Script-First**: All backlog counts, dependency chains, and effort calculations come directly from `sprints.py`.
- **Boundary Isolation**: Operate strictly within the current workspace and linked codebase.
- **Manual Fallback**: If `sprints.py` is unavailable, explain the situation and never edit `sprint.md` manually without user approval.
