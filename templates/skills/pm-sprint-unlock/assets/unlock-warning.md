# Sprint Unlock Scope Warning: {{sprint_id}}

> [!WARNING]
> **Mid-Sprint Unlocking Is Strongly Discouraged**
>
> Unlocking an active sprint introduces significant delivery risks:
> 1. **Scope Creep**: Unplanned tasks jeopardize existing sprint commitments.
> 2. **Capacity Invalidation**: Exceeds the 1-week AI-assisted capacity budget (5.0 days).
> 3. **Review Bottlenecks**: Compresses code review, QA validation, and CI turnaround.

## Current Sprint Load
- **Sprint**: {{sprint_id}}
- **Committed Capacity**: {{committed_days}} / {{capacity_days}} Days
- **Active Phases**: {{phase_count}}

## Double-Check Confirmation Required
To proceed with unlocking, you must provide explicit confirmation and a concrete business rationale:
```bash
python3 .agents/skills/pm-new-sprint/scripts/sprints.py unlock {{sprint_id}} --confirm --reason "<detailed justification>"
```
If you do not wish to unlock, abort now. The sprint remains locked and protected.
