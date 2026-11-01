---
name: pm-sprint-unlock
description: Use when the user asks to unlock a locked sprint, modify frozen sprint scope, or invokes /pm-sprint-unlock. Explains why mid-sprint unlocking is not recommended, presents current sprint load, double-checks with the user requiring explicit confirmation and reason, and calls sprints.py unlock --confirm --reason.
---

# PM Sprint Unlock

This skill manages requests to unlock a locked sprint (`locked` -> `open`). Because unlocking introduces scope creep and compromises delivery commitments, the skill acts with strict PM realism to challenge the decision before double-checking with the user.

## Agent Mandates

- **Strict PM Skepticism**: Unlocking an active sprint is not recommended. Actively explain the risks (capacity overrun, review bottlenecks, broken 1-week timeline).
- **Mandatory Double-Check**: Never unlock automatically. Require explicit user confirmation (`--confirm`) and a non-empty business justification (`--reason`).

## Workflows

### 1. Resolve Active Sprint & Check Lock State
```bash
python3 .agents/skills/pm-new-sprint/scripts/sprints.py resolve
```
Verify that the sprint is currently in `locked` state. If already `open`, report that the sprint is already open.

### 2. Present Unlock Warning
Display `assets/unlock-warning.md` to the user:
- Show current committed capacity and phase count.
- Detail the risks to sprint commitments.
- Ask the user whether they truly wish to unlock the sprint, and require a documented rationale.

### 3. Execute Unlock Upon Explicit User Confirmation
If the user confirms with a valid reason:
```bash
python3 .agents/skills/pm-new-sprint/scripts/sprints.py unlock <sprint> --confirm --reason "<reason>"
```
Report that the sprint scope is now unlocked, and remind the user to re-lock as soon as modifications are complete.

If the user declines or provides no justification:
Abort the operation. The sprint remains safely locked.

## Manual Fallback
If `sprints.py` is unavailable:
1. Open `sprints/s-###-<slug>/sprint.md`.
2. Change `- **Status**: locked` to `- **Status**: open`.
3. Append a new timestamped bullet under `## Lock & Audit Log` documenting the user confirmation and reason.
