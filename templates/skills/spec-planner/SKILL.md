---
name: specs-planner
description: Automates the creation of implementation plans and task files within the /specs/ directory. Use when the user asks to "create a plan", "start a new phase", or "break down a task", or "/spec-plan" to ensure consistent folder structure and Markdown formatting.
---

# Specs Planner

This skill ensures that all implementation plans and tasks are documented uniformly within the `/specs/` directory.

## Directory Structure

All plans must follow this hierarchy:
- `specs/phase-XXX/impl.md`: The master plan for a phase.
- `specs/phase-XXX/task-YYY.md`: Individual task breakdowns.

## Workflows

### 1. Creating a New Phase
When a user asks to "create a plan" for a new milestone:
1. Identify the next phase number (e.g., if `phase-001` exists, use `phase-002`).
2. Create `specs/phase-XXX/impl.md` using the **Implementation Plan Template**.
3. Create the first task `specs/phase-XXX/task-001.md` using the **Task Template**.
4. Update `GEMINI.md` to point to the new active phase.

### 2. Adding a Task to an Existing Phase
When a user asks to "add a task" or "break down" part of a phase:
1. Identify the active phase directory.
2. Determine the next task number (e.g., `task-002.md`).
3. Create the file using the **Task Template**.
4. Update the `impl.md` task list to include the new task.

## Templates

### Implementation Plan Template (`impl.md`)
```markdown
# Implementation Plan: [Phase Name]

## Phase Objectives
[High-level goals for this phase]

## Architecture
[Key architectural decisions or components]

## Timeline
- **Task 001**: [Task Title] (Status)

## Tasks
- [ ] Task 001: [Task Title]
```

### Task Template (`task-XXX.md`)
```markdown
# Task [XXX]: [Task Title]

## Description
[Detailed description of the work]

## Knowledge References
- [ ] Relevant `knowledgebase/` docs/patterns identified and reviewed
  - [Path or note on what was consulted]

## Objectives
- [ ] [Objective 1]
- [ ] [Objective 2]

## Testing (TDD)
- [ ] Unit tests written before implementation
- [ ] Unit tests passing

## Security
- [ ] Security audited
- [ ] Security audit passed

## Knowledge Updates
- [ ] New patterns/lessons learned documented in `knowledgebase/best-practices/` (or N/A if none)

## Status
- [ ] Pending
```

## Mandates
- **Zero-Padding**: Always use 3 digits for phases and tasks (e.g., `phase-001`, `task-005`).
- **Sync**: Every new task MUST be reflected in the corresponding `impl.md`.
- **Validation**: Ensure directories exist before writing files.
- **Test-First**: A task is not "Pending → In Progress → Done" without its unit tests written *before* implementation code and passing before the task is marked complete.
- **Security Gate**: A task involving code changes MUST be security audited, with the audit passing, before it can be marked complete. Use the `security-auditor` agent or `security-review` skill for the audit.
- **Knowledge Loop**: Every task MUST record which `knowledgebase/` references were consulted going in, and what (if anything) should be added back to `knowledgebase/best-practices/` coming out.
