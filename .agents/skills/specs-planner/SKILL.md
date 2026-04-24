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

## Objectives
- [ ] [Objective 1]
- [ ] [Objective 2]

## Status
- [ ] Pending
```

## Mandates
- **Zero-Padding**: Always use 3 digits for phases and tasks (e.g., `phase-001`, `task-005`).
- **Sync**: Every new task MUST be reflected in the corresponding `impl.md`.
- **Validation**: Ensure directories exist before writing files.
