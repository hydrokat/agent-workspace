---
name: pm-sprint-lock
description: Use when the user asks to lock the active sprint scope, freeze sprint commitments, or invokes /pm-sprint-lock. Resolves the active sprint, checks lint readiness, transitions status from open to locked via sprints.py lock, and enforces the hotfix-only bypass policy.
---

# PM Sprint Lock

This skill locks the active sprint (`open` -> `locked`) to protect commitments from mid-sprint scope creep.

## Agent Mandates

- **Strict PM Governance**: Refuse ad-hoc feature additions to a locked sprint.
- **Enforce Hotfix Policy**: Explain clearly that only emergency Bug Fixes with verified rationale can bypass the lock via `--hotfix --reason`.

## Workflows

### 1. Resolve Active Sprint
```bash
python3 .agents/skills/pm-new-sprint/scripts/sprints.py resolve
```
Verify the sprint is in `open` state. If already `locked`, report that it is already locked.

### 2. Check Readiness (Lint)
```bash
python3 .agents/skills/pm-new-sprint/scripts/sprints.py lint <sprint>
```
Ensure all required sections and HTML deliverables are sound before locking.

### 3. Lock Scope
```bash
python3 .agents/skills/pm-new-sprint/scripts/sprints.py lock <sprint> --reason "<reason>"
```

### 4. Present Lock Summary
Display `assets/lock-summary.md` with:
- Active sprint ID
- Locked status badge
- Lock timestamp and rationale
- Clear reminder of the hotfix-only bypass policy

## Manual Fallback
If `sprints.py` is unavailable:
1. Open `sprints/s-###-<slug>/sprint.md`.
2. Change `- **Status**: open` to `- **Status**: locked`.
3. Append a new timestamped bullet under `## Lock & Audit Log`.
