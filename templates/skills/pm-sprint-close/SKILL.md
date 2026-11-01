---
name: pm-sprint-close
description: Use when the user asks to close the current sprint, wrap up sprint deliverables, or invokes /pm-sprint-close. Inspects remaining tasks, provides realistic PM recommendation (finish implementation vs carry-over), and executes sprints.py close [--carry-over].
---

# PM Sprint Close

This skill evaluates sprint completion and closes the active sprint (`open`/`locked` -> `closed`). When incomplete deliverables remain, it presents a grounded PM/Tech Lead recommendation and options to carry over work or finish implementation.

## Agent Mandates

- **Strict Delivery Accountability**: Never silently drop unfinished tasks. Every incomplete task must either be implemented or explicitly carried over to the backlog of the next sprint.
- **Realistic Recommendations**: When few tasks remain (<= 3), recommend running `/pm-sprint-implement` to complete the sprint. When substantial work remains or priority shifts dictate closure, recommend `--carry-over`.

## Workflows

### 1. Inspect Sprint Completion & Attempt Close
```bash
python3 .agents/skills/pm-new-sprint/scripts/sprints.py close <sprint>
```
If all tasks are `Done`, the sprint transitions to `closed` immediately.

### 2. Handle Incomplete Deliverables
If `sprints.py close` exits with status `1`, incomplete tasks remain:
- Review the `incomplete_tasks` list and `recommendation` from the output.
- Present `assets/close-report.md` detailing what is done and what remains.
- Ask the user to choose:
  1. **Option A (Implement First)**: Run `/pm-sprint-implement` to finish remaining tasks before closing.
  2. **Option B (Carry-Over)**: Close now and automatically carry unfinished items into the next sprint's discovery via:
     ```bash
     python3 .agents/skills/pm-new-sprint/scripts/sprints.py close <sprint> --carry-over
     ```

### 3. Report Outcome
Display the final close summary with timestamps, deliverable links, and carried-over task counts.

## Manual Fallback
If `sprints.py` is unavailable:
1. Open `sprints/s-###-<slug>/sprint.md`.
2. Inspect `specs/` referenced in the sprint table to verify all task checkboxes are marked `[x]`.
3. If unfinished tasks exist and carry-over is approved, note them under `## Deferred Backlog (Won't Have)`.
4. Update `- **Status**: closed` and append an audit log entry under `## Lock & Audit Log`.
