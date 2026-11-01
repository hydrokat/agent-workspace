---
name: pm-new-sprint
description: Use when the user asks to plan, create, prioritize, or size a new 1-week sprint, create a BRD/FRD/FSD, or invokes /pm-new-sprint. Enforces a 1-week AI-assisted human capacity budget (5.0 days), prioritizes tasks with RICE and MoSCoW, auto-includes non-skippable items, and scaffolds self-contained HTML stakeholder documents.
---

# PM New Sprint

This skill plans, prioritizes, and sizes a new 1-week sprint with RICE and MoSCoW scoring, auto-includes non-skippable work, and scaffolds `sprints/s-###-<slug>/` containing `sprint.md` plus three self-contained stakeholder HTML deliverables (`business-requirements-document.html`, `functional-requirements-document.html`, `functional-specification-document.html`).

## Agent Mandates

- **Strict PM/BA/TL Persona**: Operate with realistic, detail-oriented skepticism. Never accept optimistic deadlines or overloaded sprint scopes.
- **1-Week AI-Assisted Capacity Budget**: Treat 5.0 effort-days (max 3 phases, max 15 implementation tasks) as a hard constraint. Must-Have items must remain under 70% of capacity (3.5 days). Push back against scope creep.
- **Project Boundary Isolation**: All sprints, phases, and deliverables must belong strictly to this project's workspace and explicitly linked `codebase-symlinks/`.

## Workflows

### 1. Discover Pending Work
Run the discovery command:
```bash
python3 .agents/skills/pm-new-sprint/scripts/sprints.py discover
```
Inspect the output:
- `mandatory`: Non-skippable items (Bug Fixes, active carry-overs, critical dependencies). Auto-include these in the sprint.
- `recommended`: Sorted to top of existing spec choices.
- `optional`: Other pending specs.

### 2. Prioritize Scope with RICE and MoSCoW
1. Score each proposed phase with RICE:
   - Reach > 0
   - Impact in {3.0 (Massive), 2.0 (High), 1.0 (Medium), 0.5 (Low), 0.25 (Minimal)}
   - Confidence in {1.0 (High), 0.8 (Medium), 0.5 (Low)}
   - Effort in AI-assisted person-days
   - `RICE Score = round((Reach * Impact * Confidence) / Effort, 2)`
2. Assign MoSCoW priority: `Must Have`, `Should Have`, `Could Have`, `Won't Have`.
3. Ask the user which skippable tasks to defer to the backlog (`Won't Have`).
4. If total effort exceeds 5.0 days, actively warn the user and recommend deferring items.

### 3. Create Sprint
Run `sprints.py create`:
```bash
python3 .agents/skills/pm-new-sprint/scripts/sprints.py create --title "<Title>" --slug "<slug>" --phase "<phase>" --moscow "<moscow>" --reach <R> --impact <I> --confidence <C> --effort <E>
```
If an active sprint is already open or locked, creating a second active sprint is refused; use `--status planned` to queue upcoming sprints.

### 4. Replace Prose Placeholders
The generated `sprint.md`, `business-requirements-document.html`, `functional-requirements-document.html`, and `functional-specification-document.html` contain `[placeholder]` tokens for the agent to replace with real project content:
- In `sprint.md`: Populate business objectives and context.
- In `business-requirements-document.html`: Detail stakeholder needs, measurable KPIs, and non-goals.
- In `functional-requirements-document.html`: Detail user personas, user flows, and functional requirements (`FR-001...`).
- In `functional-specification-document.html`: Detail system architecture, API/CLI contracts, error handling, and security.

### 5. Lint
Validate sprint integrity and security:
```bash
python3 .agents/skills/pm-new-sprint/scripts/sprints.py lint <sprint>
```
Fix any problems until `"ok": true`.

## Manual Fallback
If `sprints.py` is unavailable:
1. Determine next sprint number by listing `sprints/s-###-*` directories.
2. Create `sprints/s-###-<slug>/` directory.
3. Copy `templates/skills/pm-new-sprint/assets/sprint.md` and HTML templates, populating fields manually.
4. Calculate RICE manually: `(Reach * Impact * Confidence) / Effort`.
