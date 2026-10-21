---
name: specs-planner
description: Use when the user asks to create implementation plans, start a new phase, break down work into /specs/ tasks, or invoke /spec-plan, /spec-planner, or /specs-planner. Captures branch strategy, repo-scoped p-### phase directories, t-### task files, involved repositories, requirement type, codebase structure, UI mockups for frontend/UI work, existing implementation patterns, regression gates, task files, delivery gates, PR review loops, pipeline loops, cleanup, and merge readiness.
---

# Specs Planner

This skill ensures that all implementation plans and tasks are documented uniformly within the `/specs/` directory.

## Directory Structure

All plans must follow this hierarchy:
- `specs/p-###-[repo-slug]/impl.md`: The master plan for a single-repository phase.
- `specs/p-###-multi/impl.md`: The master plan for a multi-repository phase.
- `specs/p-###-[repo-slug]/t-###.md`: Individual task breakdowns.

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

### 2. Identify Repository Scope And Phase Name
Before creating or changing a phase, determine which repositories are involved and use them in the phase naming and `impl.md` metadata.

1. Inspect the current repository name, linked codebases, relevant paths, and user request to identify every repository involved.
2. For single-repository work, name the phase directory `p-###-[name-of-repo]`, where `[name-of-repo]` is a lowercase repo slug derived from the repository name.
3. For multi-repository work, name the phase directory `p-###-multi`.
4. Record all involved repositories in `impl.md` under **Repository Scope**, including each repo's role in the phase.
5. Do not use legacy `phase-XXX` directory names for new phases. Existing legacy directories may be read for context only.

### 3. Classify Requirements
Classify every implementation requirement before tasking it:

- `New Feature`: adds a capability or behavior that does not currently exist.
- `Enhancement`: improves, extends, optimizes, or refines existing behavior without changing its core intent.
- `Bug Fix`: corrects broken, unintended, inconsistent, insecure, or regressed behavior.

Use the classification to choose branch type, testing strategy, regression coverage, rollout notes, and review focus. When a task contains mixed work, split it or clearly label the primary classification and any secondary impacts.

### 4. Add UI Mockups For Frontend/UI Work
When a phase or task creates or changes user-facing frontend/UI behavior, include UI mockups before implementation tasking is finalized.

1. Identify whether the requested work touches screens, flows, components, content hierarchy, navigation, forms, tables, dashboards, visual states, responsive layout, accessibility affordances, or interaction behavior.
2. If frontend/UI work is involved, add a **UI Mockups** section to `impl.md` and every affected task file. Backend-only, infrastructure-only, docs-only, and internal tooling tasks may mark this section `N/A`.
3. Keep mockups low-fidelity but implementation-useful. Use fenced `text`, Mermaid, or another readable diagram format to show layout, major regions, hierarchy, flows, states, and responsive differences.
4. Include the expected states relevant to the task, such as loading, empty, error, success, disabled, validation, permissions, and destructive-confirmation states.
5. Include mobile and desktop variations when the feature must be responsive.
6. Add notes for accessibility and interaction requirements when they affect implementation, such as focus order, keyboard behavior, labels, contrast, reduced motion, or screen-reader text.
7. If visual requirements are uncertain, document assumptions in the mockup section instead of leaving the UI undefined.

### 4b. Check Active & Planned Work
Before breaking work into tasks, make sure the plan does not conflict with work already running or planned.
1. Run `python3 .agents/skills/agent-coordination/scripts/coord.py overview` and read running sessions, open claims, and open queries.
2. Read `specs/in-progress.md` and the `Pending`, `In Progress`, and `Blocked` tasks across all of `specs/`.
3. Compare each new task's **Affected Paths** with the paths of claimed and planned tasks. On overlap, either sequence the tasks with **Depends On** or ask the user.
4. If `coord.py` is unavailable or fails, say so and rely on `in-progress.md` and the specs only.

### 5. Creating a New Phase
When a user asks to "create a plan" for a new milestone:
1. Identify the next phase number: `python3 .agents/skills/spec-planner/scripts/specs.py next-phase` (it counts `p-###-*` and legacy `phase-XXX` directories).
2. Complete **Choose Branches Before Planning**.
3. Complete **Inspect The Existing Codebase**.
4. Complete **Identify Repository Scope And Phase Name**.
5. Complete **Classify Requirements**.
6. Complete **Add UI Mockups For Frontend/UI Work**.
6b. Complete **Check Active & Planned Work**.
7. Break the milestone into implementation tasks.
8. Create the phase and its final delivery task from the templates:
   `python3 .agents/skills/spec-planner/scripts/specs.py new-phase (--repo <repo-slug> | --multi) --title "<phase title>" --branch <working-branch> --target <merge-branch> [--pattern "<branch pattern>"] [--pattern-source "<source>"]`
   It writes `specs/p-###-[repo-or-multi]/impl.md` from `assets/impl.md` and the delivery task `t-001.md` from `assets/delivery-task.md`, with the numbers, branches, and scope filled in.
9. Add each implementation task: `python3 .agents/skills/spec-planner/scripts/specs.py new-task <phase> --title "<title>" --type "<New Feature|Enhancement|Bug Fix>" [--depends t-###…] [--paths <path>…]`. It renders `assets/task.md`, inserts the task before the delivery task, renumbers the delivery task so it stays last, and updates the `impl.md` Timeline and Tasks lists.
10. Fill every `[...]` placeholder in `impl.md` and the task files with the real plan content (description, classification, knowledge references, codebase guide, structure diagram, UI mockups, objectives, and the rest). Leave nothing in brackets.
11. Generate the plan's parallelism: `python3 .agents/skills/spec-planner/scripts/specs.py waves <phase> --write` fills the `## Parallel Waves` section of `impl.md` from each task's `## Depends On` and `## Affected Paths`. Re-run it whenever tasks, dependencies, or paths change.
12. Run `python3 .agents/skills/spec-planner/scripts/specs.py lint <phase>` and fix every reported problem (missing sections, unfilled placeholders, status or title disagreement between `impl.md` and the task files, delivery task not last) until it reports `"ok": true`.
13. Update `GEMINI.md` to point to the new active phase when that file exists.

If `specs.py` or its `assets/` are unavailable or fail, say so and do the same work by hand: copy the files in `assets/` (or the templates under **Templates**), replace each `{{field}}` yourself, keep the delivery task last, and keep `impl.md` in sync.

### 6. Adding a Task to an Existing Phase
When a user asks to "add a task" or "break down" part of a phase:
1. Identify the active phase directory: `python3 .agents/skills/spec-planner/scripts/specs.py resolve [<phase>]`.
2. If the phase does not already record working branch, merge target, and branch naming pattern, complete **Choose Branches Before Planning** and update `impl.md`.
3. If the phase does not already record repository scope, complete **Identify Repository Scope And Phase Name** and update `impl.md`.
4. Inspect the existing codebase area affected by the new task and update the phase's code structure diagram or existing pattern notes when needed.
5. Classify the new requirement as `New Feature`, `Enhancement`, or `Bug Fix`.
6. Complete **Add UI Mockups For Frontend/UI Work** for any frontend/UI tasks.
7. Create each task with `specs.py new-task` (see Workflow 5, step 9). The script picks the next task number, inserts the task before the final delivery task, renumbers the delivery task, and updates the `impl.md` Timeline and Tasks lists.
8. Fill the task's `[...]` placeholders, run `specs.py waves <phase> --write` to refresh the `## Parallel Waves` section, then run `specs.py lint <phase>` until it reports `"ok": true`.

If the script is unavailable, fall back to the manual steps: determine the next `t-###` number, copy `assets/task.md`, insert the task before the delivery task, renumber the delivery task, and update the `impl.md` task lists.

## Templates

The templates live in `assets/` next to this file. `specs.py` renders them; they can also be copied by hand when the script is unavailable.

| File | Creates | Script-filled fields (`{{field}}`) |
|---|---|---|
| `assets/impl.md` | `impl.md` (Implementation Plan, including the generated `## Parallel Waves` section) | `title`, `branch`, `target`, `pattern`, `pattern_source`, `phase_dir`, `scope_type`, `primary_repo`, `timeline`, `tasks` |
| `assets/task.md` | `t-###.md` (Task) | `task_id`, `title`, `type`, `paths`, `depends`, `branch`, `target` |
| `assets/delivery-task.md` | the final delivery task | `task_id`, `branch`, `target` |

Everything else in a template is a `[...]` placeholder for the agent to replace with real content. `specs.py lint` fails while a placeholder or a `{{field}}` is left in a phase file.

Every task file includes `## Affected Paths` and `## Depends On` so overlapping work can be detected. Every phase plan includes Branch Strategy, Repository Scope, Code Structure Diagram, Timeline, and Tasks. The delivery task always comes last and, when the agent workspace itself is a git repository, also delivers the workspace changes.

## Mandates
- **Script-First**: Create phases and tasks with `specs.py new-phase` / `new-task` and finish with `specs.py lint`, falling back to the `assets/` templates by hand only when the script is unavailable.
- **Zero-Padding**: Always use 3 digits for phases and tasks (e.g., `p-001-my-repo`, `p-002-multi`, `t-005.md`).
- **Phase Directory Naming**: New phase directories MUST use `p-###-[name-of-repo]` for single-repo work and `p-###-multi` for multi-repo work. Do not create new `phase-XXX` directories.
- **Task File Naming**: New task files MUST use `t-###.md`. Do not create new `task-YYY.md` files.
- **Repository Scope**: Every `impl.md` MUST include the repositories involved and each repository's role in the phase.
- **Sync**: Every new task MUST be reflected in the corresponding `impl.md`.
- **Validation**: Ensure directories exist before writing files.
- **Branch Choice**: Always ask the user to choose the working branch and merge target before creating or updating implementation tasks. Suggest values, but allow user-entered branch names.
- **Branch Pattern Learning**: Always capture the chosen branch naming pattern in `impl.md`, and update a concise knowledgebase note when a knowledgebase exists.
- **Codebase First**: Always inspect the existing codebase and use its structure, patterns, tests, and conventions as the guide for implementation tasks.
- **Structure Diagram**: Every phase plan and code-changing task MUST include a code structure or directory structure diagram in fenced `text`, Mermaid, PlantUML, or similar diagram form.
- **UI Mockups**: Frontend/UI phases and tasks MUST include low-fidelity UI mockups covering primary screens or components, responsive variants, important states, interaction notes, and accessibility notes. Non-UI work may mark UI mockups as `N/A`.
- **Requirement Classification**: Every code-changing task MUST classify its requirement as `New Feature`, `Enhancement`, or `Bug Fix`, with a short rationale and regression risk.
- **No Regressions**: Development tasks MUST identify affected existing behavior and require tests or validation that prove the change does not introduce regressions.
- **Active Work Check**: Always complete **Check Active & Planned Work** before tasking, and give every code-changing task `## Affected Paths` and `## Depends On`.
- **Delivery Task Last**: The final task in every phase MUST be the delivery task: commit > create PR > code review and remediation loop until review passes > pipeline wait and fix loop until pipelines pass > merge PR.
- **Workspace Is Delivered Too**: When the agent workspace is itself a git repository, the delivery task MUST also commit (and, when it has a remote, PR and merge) the workspace's own changes so specs and knowledgebase stay git-managed. Without a remote, commit on a branch and ask the user how to integrate.
- **Test-First**: A task is not "Pending → In Progress → Done" without its unit tests written *before* implementation code and passing before the task is marked complete.
- **Security Gate**: A task involving code changes MUST be security audited, with the audit passing, before it can be marked complete. Use the `security-auditor` agent or `security-review` skill for the audit.
- **Knowledge Loop**: Every task MUST record which `knowledgebase/` references were consulted going in, and what (if anything) should be added back to `knowledgebase/best-practices/` coming out.
- **Review Cleanup**: The final delivery task's code review phase MUST include a cleanup pass (`/code-cleanup --diff <merge-target>`) that removes unnecessary comments, favors self-documenting code, and permits documentation-generator comments when they add value.
