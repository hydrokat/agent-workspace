# Sprint Scope Locked: {{sprint_id}}

- **Sprint**: {{sprint_id}}
- **Status**: LOCKED
- **Locked At**: {{timestamp}}
- **Reason**: {{reason}}

## Lock Governance Invariant

Sprint scope is frozen for the active 1-week cycle. No new normal feature phases or spec additions may be added until the current sprint closes and the next sprint opens.

### Emergency Hotfix Bypass Policy

Only critical Bug Fixes or emergency patches may bypass the lock:
```bash
python3 .agents/skills/pm-new-sprint/scripts/sprints.py add-phase {{sprint_id}} <phase> --hotfix --reason "<justification>"
```
Any hotfix addition is permanently recorded in the sprint's audit log.
