---
name: specs-planner
description: Use when the user asks to create implementation plans, start a new phase, break down work into /specs/ tasks, or invoke /spec-plan or /specs-planner. Captures branch strategy, requirement type, codebase structure, existing implementation patterns, regression gates, task files, delivery gates, PR review loops, pipeline loops, cleanup, and merge readiness.
---

# Specs Planner

This skill ensures that all implementation plans and tasks are documented uniformly within the `/specs/` directory.

## Directory Structure

All plans must follow this hierarchy:
- `specs/phase-XXX/impl.md`: The master plan for a phase.
- `specs/phase-XXX/task-YYY.md`: Individual task breakdowns.

## Workflows

### 0. Choose Branches Before Planning
Before creating or updating a phase, determine the branch strategy with the user.

1. Inspect existing branch names, recent spec files, and knowledgebase notes for the repository's branch naming pattern.
2. Suggest a working branch name that follows the observed pattern. If no pattern is clear, suggest `<type>/<short-description>` using a type such as `feature`, `bugfix`, `hotfix`, `refactor`, `chore`, `docs`, or `test`.
3. Ask the user to choose what branch to work on. Present the suggested branch name and allow the user to enter a different branch name.
4. Ask the user to choose which branch the work should merge into. Suggest the repository's primary branch, preferring `main`, then `trunk`, then `master`, then the configured default branch. Allow the user to enter a different target branch.
5. Learn the selected branch naming pattern for future planning by recording it in the generated `impl.md`. If the repository has `knowledgebase/context-history/` or `knowledgebase/guidelines/`, add or update a concise branch naming note there as well.

### 1. Inspect The Existing Codebase
Before creating or changing implementation tasks, inspect the current codebase so the plan follows the project's real structure and patterns.

1. Read the repository overview files, such as `README.md`, `AGENTS.md`, `WORKFLOW.md`, and relevant `knowledgebase/` entries when they exist.
2. Inspect the directories, modules, tests, routes, models, components, services, and configuration files related to the requested work.
3. Identify existing implementation patterns that should guide the change, including naming, layering, validation, error handling, dependency use, test style, and documentation style.
4. Include a code structure or directory structure diagram in the plan. Use a fenced `text`, Mermaid, PlantUML, or similar diagram format that makes the relevant files and relationships easy to scan.
5. Record the existing code paths and patterns each task should follow. If the affected area cannot be determined from the request, note the assumption or ask the user before planning risky work.

### 2. Classify Requirements
Classify every implementation requirement before tasking it:

- `New Feature`: adds a capability or behavior that does not currently exist.
- `Enhancement`: improves, extends, optimizes, or refines existing behavior without changing its core intent.
- `Bug Fix`: corrects broken, unintended, inconsistent, insecure, or regressed behavior.

Use the classification to choose branch type, testing strategy, regression coverage, rollout notes, and review focus. When a task contains mixed work, split it or clearly label the primary classification and any secondary impacts.

### 3. Creating a New Phase
When a user asks to "create a plan" for a new milestone:
1. Identify the next phase number (e.g., if `phase-001` exists, use `phase-002`).
2. Complete **Choose Branches Before Planning**.
3. Complete **Inspect The Existing Codebase**.
4. Complete **Classify Requirements**.
5. Break the milestone into implementation tasks.
6. Always add a final delivery task after all implementation tasks using the **Delivery Task Template**.
7. Create `specs/phase-XXX/impl.md` using the **Implementation Plan Template**.
8. Create task files `specs/phase-XXX/task-YYY.md` using the **Task Template** or **Delivery Task Template**.
9. Update `GEMINI.md` to point to the new active phase when that file exists.

### 4. Adding a Task to an Existing Phase
When a user asks to "add a task" or "break down" part of a phase:
1. Identify the active phase directory.
2. Determine the next task number (e.g., `task-002.md`).
3. If the phase does not already record working branch, merge target, and branch naming pattern, complete **Choose Branches Before Planning** and update `impl.md`.
4. Inspect the existing codebase area affected by the new task and update the phase's code structure diagram or existing pattern notes when needed.
5. Classify the new requirement as `New Feature`, `Enhancement`, or `Bug Fix`.
6. Insert implementation tasks before the final delivery task. The final delivery task must remain last.
7. Create each implementation task file using the **Task Template**.
8. Renumber the final delivery task if needed so it is the last task.
9. Update the `impl.md` task list to include the new tasks and the final delivery task.

## Templates

### Implementation Plan Template (`impl.md`)
````markdown
# Implementation Plan: [Phase Name]

## Branch Strategy
- **Working Branch**: [selected branch name]
- **Merge Target**: [selected target branch]
- **Observed Branch Pattern**: [pattern to reuse moving forward, e.g. feature/short-description]
- **Branch Pattern Source**: [existing branches, user choice, repo docs, or N/A]

## Phase Objectives
[High-level goals for this phase]

## Architecture
[Key architectural decisions or components]

## Existing Codebase Guide
- **Relevant Code Paths**:
  - `[path]`: [why this file or directory guides the implementation]
- **Observed Patterns To Follow**:
  - [Naming, layering, test, validation, error handling, dependency, or documentation pattern]
- **Regression-Sensitive Areas**:
  - [Existing behavior, API, workflow, data shape, or test suite that must not regress]

## Code Structure Diagram
```text
[Use a directory tree, Mermaid diagram, PlantUML diagram, or other concise diagram showing the affected code structure.]
```

## Timeline
- **Task 001**: [Task Title] (Status)
- **Task NNN**: Delivery: Commit, PR, Review, CI, Merge (Pending)

## Tasks
- [ ] Task 001: [Requirement Type] [Task Title]
- [ ] Task NNN: Delivery: Commit, PR, Review, CI, Merge
````

### Task Template (`task-XXX.md`)
````markdown
# Task [XXX]: [Task Title]

## Description
[Detailed description of the work]

## Requirement Classification
- **Type**: [New Feature | Enhancement | Bug Fix]
- **Rationale**: [Why this classification applies]
- **Regression Risk**: [Low | Medium | High] - [existing behavior that must not regress]

## Knowledge References
- [ ] Relevant `knowledgebase/` docs/patterns identified and reviewed
  - [Path or note on what was consulted]

## Existing Codebase Guide
- [ ] Existing implementation inspected before coding
  - **Relevant Code Paths**: [files/directories/modules/tests to follow]
  - **Patterns To Reuse**: [existing code style, layering, APIs, tests, documentation comments, or conventions]

## Code Structure Diagram
```text
[Show the directory tree or relationship diagram for files/modules this task will touch.]
```

## Objectives
- [ ] [Objective 1]
- [ ] [Objective 2]

## Branches
- **Working Branch**: [branch from impl.md]
- **Merge Target**: [target branch from impl.md]

## Testing (TDD)
- [ ] Unit tests written before implementation
- [ ] Unit tests passing
- [ ] Regression tests or validation added for affected existing behavior
- [ ] Existing relevant test suites passing

## Security
- [ ] Security audited
- [ ] Security audit passed

## Code Quality
- [ ] Implementation follows existing codebase patterns
- [ ] No unnecessary comments added; code is self-documenting where practical
- [ ] Documentation-generator comments are present only where useful, such as JSDoc, PHPDoc, Swagger/OpenAPI, or equivalent

## Knowledge Updates
- [ ] New patterns/lessons learned documented in `knowledgebase/best-practices/` (or N/A if none)

## Status
- [ ] Pending
````

### Delivery Task Template (`task-NNN.md`)
```markdown
# Task [NNN]: Delivery: Commit, PR, Review, CI, Merge

## Description
Finalize the phase by committing the completed work, creating a pull request, resolving review feedback, waiting for pipelines, fixing failures, and merging only after review and pipeline gates pass.

## Branches
- **Working Branch**: [branch from impl.md]
- **Merge Target**: [target branch from impl.md]

## Objectives
- [ ] Confirm all implementation tasks in this phase are complete.
- [ ] Review final diff and exclude unrelated changes.
- [ ] Confirm each requirement is classified as `New Feature`, `Enhancement`, or `Bug Fix`.
- [ ] Confirm code structure diagrams and existing codebase guide notes are current.
- [ ] Confirm regression tests or validation cover affected existing behavior, and no regression has been introduced.
- [ ] Run a final cleanup pass before code review: remove unnecessary comments, keep code self-documenting where practical, and preserve only useful documentation-generator comments such as JSDoc, PHPDoc, Swagger/OpenAPI, or equivalent.
- [ ] Commit changes using the `commit` skill.
- [ ] Create a pull request targeting the merge branch using the `pull-request` skill.
- [ ] Run `code-review`, including regression risk and cleanup/self-documenting-code checks.
- [ ] If code review requests changes, run `code-review-remediation`, update the PR, and repeat code review until it passes.
- [ ] Wait for required pipelines/checks to finish.
- [ ] If any required pipeline/check fails, fix the failure, update the PR, and repeat until required pipelines/checks pass.
- [ ] Merge the pull request into the selected merge target.

## Status
- [ ] Pending
```

## Mandates
- **Zero-Padding**: Always use 3 digits for phases and tasks (e.g., `phase-001`, `task-005`).
- **Sync**: Every new task MUST be reflected in the corresponding `impl.md`.
- **Validation**: Ensure directories exist before writing files.
- **Branch Choice**: Always ask the user to choose the working branch and merge target before creating or updating implementation tasks. Suggest values, but allow user-entered branch names.
- **Branch Pattern Learning**: Always capture the chosen branch naming pattern in `impl.md`, and update a concise knowledgebase note when a knowledgebase exists.
- **Codebase First**: Always inspect the existing codebase and use its structure, patterns, tests, and conventions as the guide for implementation tasks.
- **Structure Diagram**: Every phase plan and code-changing task MUST include a code structure or directory structure diagram in fenced `text`, Mermaid, PlantUML, or similar diagram form.
- **Requirement Classification**: Every code-changing task MUST classify its requirement as `New Feature`, `Enhancement`, or `Bug Fix`, with a short rationale and regression risk.
- **No Regressions**: Development tasks MUST identify affected existing behavior and require tests or validation that prove the change does not introduce regressions.
- **Delivery Task Last**: The final task in every phase MUST be the delivery task: commit > create PR > code review and remediation loop until review passes > pipeline wait and fix loop until pipelines pass > merge PR.
- **Test-First**: A task is not "Pending → In Progress → Done" without its unit tests written *before* implementation code and passing before the task is marked complete.
- **Security Gate**: A task involving code changes MUST be security audited, with the audit passing, before it can be marked complete. Use the `security-auditor` agent or `security-review` skill for the audit.
- **Knowledge Loop**: Every task MUST record which `knowledgebase/` references were consulted going in, and what (if anything) should be added back to `knowledgebase/best-practices/` coming out.
- **Review Cleanup**: The final delivery task's code review phase MUST include a cleanup pass that removes unnecessary comments, favors self-documenting code, and permits documentation-generator comments when they add value.
