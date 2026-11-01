---
name: pm-sprint-explain
description: Use when the user asks what a sprint is about, what its goals and priorities are, or invokes /pm-sprint-explain, optionally with a sprint ID such as /pm-sprint-explain s-001. Reads the sprint through sprints.py and explains its goals, RICE/MoSCoW priorities, and deliverables. Read-only.
---

# PM Sprint Explain

This skill provides a concise, structured walk-through of a sprint's goals, scope, RICE and MoSCoW priorities, linked spec phases, and stakeholder deliverables.

## Step 0: Parse The Request

| Invocation | Scope |
|---|---|
| `/pm-sprint-explain` | Active sprint (`open` or `locked`) |
| `/pm-sprint-explain s-001` (also `001`, `1`, or full directory name) | That specific sprint |

## Step 1: Load The Data

```bash
python3 .agents/skills/pm-new-sprint/scripts/sprints.py resolve [<sprint>]
python3 .agents/skills/pm-new-sprint/scripts/sprints.py show <sprint>
```

If `resolve` exits `1`, no active sprint was found or the requested sprint does not exist. Say so clearly. If `sprints/` has no sprints, suggest `/pm-new-sprint`.

## Step 2: Map The Data To The Template

Fill `assets/explanation.md` using the fields from `sprints.py show`:

<!-- data-map -->
| Section | Source |
|---|---|
| Sprint ID, Title, Status | `sprint`, `title`, `status` |
| Window & Capacity | `start_date`, `end_date`, `committed_effort`, `capacity_days` |
| Prioritized Scope | `phases` (`phase`, `title`, `moscow`, `rice_score`, `effort`, `spec_link`) |
| Deliverables | `deliverables` (`business-requirements-document.html`, `functional-requirements-document.html`, `functional-specification-document.html`) |
<!-- /data-map -->

## Step 3: Present

Print the completed explanation in chat. If pending tasks remain, offer `/pm-sprint-implement` or `/pm-sprint-status`.

## Mandates

- **Read-Only**: Never edit sprint files or specs. Write a file only if the user explicitly asks to save.
- **Script-First**: All metadata, scores, and counts come from `sprints.py`.
