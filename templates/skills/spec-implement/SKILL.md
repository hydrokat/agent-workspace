---
name: spec-implement
description: "Implements planned work from /specs by reading phase plans and task files, assigning each task to the right agent role and skills, using caveman mode whenever possible, enforcing strict code-comment discipline, executing the work, updating task progress continuously, and keeping knowledgebase references and updates in sync. Trigger with /spec-implement, optionally scoped to a phase or task such as /spec-implement phase-002 or /spec-implement phase-002 task-003."
---

# Spec Implement

## Overview

Use this skill to turn documented specs into completed work while preserving traceability between `specs/`, agents, skills, code changes, validation, and `knowledgebase/`.

## Step 0: Parse Scope

Determine the requested implementation scope before editing files.

| Invocation | Scope |
|---|---|
| `/spec-implement` | Active or latest incomplete phase |
| `/spec-implement phase-002` | All incomplete tasks in `specs/phase-002/` |
| `/spec-implement phase-002 task-003` | Only `specs/phase-002/task-003.md` |
| `/spec-implement task-003` | `task-003.md` in the active or latest incomplete phase |

If scope is ambiguous, inspect `specs/` and choose the latest phase with incomplete tasks. Tell the user which scope was selected.

## Step 1: Prepare The Branch

Never implement directly on `main`, `master`, or `trunk`.

1. Run `git status --short` and `git branch --show-current`.
2. If the current branch is `main`, `master`, or `trunk`:
   - If the working tree is clean, create and switch to a new branch before making any changes.
   - If the working tree is dirty with unrelated changes, do not discard them. Ask whether to leave them alone, commit them separately, or stash them before branching.
3. If the current branch is already a non-protected feature branch, continue on it rather than creating another one.
4. Name the branch `<type>/<short-description>`, where type is `feature`, `bugfix`, `hotfix`, `refactor`, `chore`, `docs`, or `test`, derived from the selected scope (e.g. `feature/phase-002-task-003`).

## Step 2: Read Spec Context

Before implementation:
1. Read the selected `specs/phase-XXX/impl.md`.
2. Read every selected `specs/phase-XXX/task-YYY.md`.
3. Search `knowledgebase/` for references relevant to the task domain, architecture, business flow, and previous lessons.
4. Read `AGENTS.md`, `WORKFLOW.md` if present, and relevant agent definitions under `.agents/agents/`.
5. Inspect available workspace skills under `.agents/skills/` before falling back to global skills.

Do not begin implementation until the current spec intent, task objectives, existing knowledgebase guidance, and available specialist roles are understood.

## Step 3: Normalize Task Metadata

Every selected task file must include or be updated to include these sections before or during execution:

```markdown
## Agent Assignment
- **Primary Agent**: [agent-name]
- **Supporting Agents**: [agent-name, if needed]
- **Skills**: [caveman, task-specific skill-name, if needed]

## Knowledgebase References
- [relative/path.md] — [why it matters]

## Progress
- [ ] Read spec and task context
- [ ] Assigned appropriate agent and skills
- [ ] Implemented changes
- [ ] Updated knowledgebase
- [ ] Validated changes

## Knowledgebase Updates
- [ ] [relative/path.md] — [summary of update needed or completed]
```

If no relevant knowledgebase reference exists, create one in the appropriate `knowledgebase/` directory before or during implementation and reference it from the task.

## Step 4: Assign Agents And Skills

Assign each task to the most appropriate role. Prefer workspace agent definitions when present.

Common routing:
- `tech-lead-orchestrator`: cross-cutting architecture, sequencing, integration decisions.
- `codebase_investigator`: unfamiliar code, dependency tracing, regression investigation.
- `senior-backend-dev`: backend implementation, database, APIs, services.
- `frontend-dev`: UI, state, components, styling, accessibility.
- `qa-engineer`: tests, repros, validation plans, regression coverage.
- `security-auditor`: auth, authorization, sensitive data, injection, dependency risk.
- `business-analyst`: requirements interpretation, workflow impact, acceptance criteria.
- `technical-writer`: knowledgebase updates, implementation notes, documentation cleanup.

If subagent tooling is available, delegate bounded work to the assigned agent. If it is not available, explicitly act in that role and state the role in progress notes. Load all task-relevant skills before delegating or executing work.

### Caveman Skill Requirement

Use the `caveman` skill whenever possible during spec implementation.

- Add `caveman` to the task's `Skills` metadata unless the skill is unavailable or the current step falls under the caveman skill's own clarity/safety exceptions.
- Use caveman style for progress updates, implementation notes, and routine technical summaries when it does not reduce correctness or safety.
- Do not use caveman style inside code, generated documentation comments, API contracts, user-facing copy, or any file where terse wording would damage clarity.
- If caveman cannot be used for a task or step, record why in the task's implementation notes.

## Step 5: Code Comment Discipline

Treat source code comments as documentation only. Do not use comments for thinking, planning, narration, or explaining your own process.

Allowed comments:
- Documentation-generator comments such as PHPDoc, JSDoc, Swagger/OpenAPI annotations, or equivalent API/reference documentation.
- Short code documentation comments only when they clarify non-obvious domain constraints, invariants, compatibility requirements, or security-sensitive behavior.

Forbidden comments:
- Scratch thoughts, step-by-step reasoning, or "thinking out loud" inside code.
- Comments that narrate obvious code behavior.
- TODO comments used as a substitute for implementing or documenting the real next step in the task file.
- Comments that explain why the agent chose an approach when that explanation belongs in `task-YYY.md`, `impl.md`, a commit message, or knowledgebase entry.

When editing code:
1. Prefer clear names, small functions, and direct structure over explanatory comments.
2. Put implementation notes, tradeoffs, and follow-up work in the task file or knowledgebase, not in source comments.
3. Before marking a task complete, remove unnecessary comments and keep only documentation comments that add durable value.

## Step 6: Execute One Task At A Time

For each task:
1. Mark the task as in progress in both `task-YYY.md` and `impl.md`.
2. Implement only the scoped objective.
3. Update task progress immediately after each meaningful milestone.
4. Keep the `Knowledgebase References` section accurate as new references are discovered.
5. Update the knowledgebase with decisions, reusable patterns, business flow changes, or lessons learned.
6. Record implementation notes in the task file with changed files and key decisions.
7. Run the narrowest useful validation first, then broader validation when appropriate.
8. Record validation results in the task file.
9. Request or run a security review when the task touches risky surfaces.
10. Run the code comment discipline cleanup before review or completion.
11. Mark the task complete only when objectives, knowledgebase updates, validation, security review, and comment cleanup are done or explicitly documented as not applicable.

Do not batch multiple task completions without updating progress between them.

## Step 7: Update Knowledgebase

Every task must either update the knowledgebase or explicitly document why no knowledgebase update was needed.

Use the `knowledge` skill workflow when making substantive updates:
- `knowledgebase/guidelines/` for operating rules or standards.
- `knowledgebase/best-practices/` for reusable implementation patterns.
- `knowledgebase/business-flows/` for product or user workflow changes.
- `knowledgebase/context-history/` for decisions, rationale, migrations, and historical context.

Write high-signal entries only. Prefer updating existing files over creating duplicates.

## Step 8: Maintain Progress

Always synchronize progress in both places:
- `specs/phase-XXX/task-YYY.md`: detailed progress, assignment, references, implementation notes, validation, and knowledgebase updates.
- `specs/phase-XXX/impl.md`: task list status and phase-level timeline/status.

Use these status terms consistently:
- `Pending`: not started.
- `In Progress`: active work.
- `Blocked`: cannot proceed without external input or dependency.
- `In Review`: implementation complete, awaiting validation/review.
- `Done`: implemented, validated, security-reviewed or documented as not applicable, and knowledgebase updated.

## Mandates

- Never implement directly on `main`, `master`, or `trunk`; always create or switch to a feature branch first.
- Always read the relevant spec and task files before implementation.
- Always assign each task to an appropriate agent role and relevant skills.
- Always use the `caveman` skill whenever possible, and document the reason when it is unavailable or inappropriate.
- Always include knowledgebase references in each task.
- Always update the knowledgebase or document why no update was needed.
- Always update task progress after each meaningful milestone.
- Never mark a task `Done` without validation evidence in the task file.
- Never mark a task `Done` if `impl.md` and `task-YYY.md` disagree.
- Never think inside code comments. Source comments are for durable code documentation only.
- Never leave unnecessary code comments. Keep documentation-generator comments only when they add value, such as PHPDoc, JSDoc, Swagger/OpenAPI, or equivalent.
- Keep changes surgical and scoped to the selected task.
