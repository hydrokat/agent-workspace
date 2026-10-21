---
name: spec-implement
description: "Implements planned work from /specs by reading p-### phase plans and t-### task files, assigning each task to the right agent role and skills, using caveman mode whenever possible, enforcing strict code-comment discipline, executing the work, updating task progress continuously, and keeping knowledgebase references and updates in sync. Trigger with /spec-implement, optionally scoped to a phase or task such as /spec-implement p-002-agent-workspace or /spec-implement p-002-agent-workspace t-003."
---

# Spec Implement

## Overview

Use this skill to turn documented specs into completed work while preserving traceability between `specs/`, agents, skills, code changes, validation, and `knowledgebase/`.

## Step 0: Parse Scope

Determine the requested implementation scope before editing files.

| Invocation | Scope |
|---|---|
| `/spec-implement` | Active or latest incomplete phase |
| `/spec-implement p-002-agent-workspace` | All incomplete tasks in `specs/p-002-agent-workspace/` |
| `/spec-implement p-002-agent-workspace t-003` | Only `specs/p-002-agent-workspace/t-003.md` |
| `/spec-implement t-003` | `t-003.md` in the active or latest incomplete phase |

Resolve the phase argument with `python3 .agents/skills/spec-planner/scripts/specs.py resolve [<phase>]` (it accepts `p-002`, `002`, `2`, the full directory name, or nothing for the latest phase with an incomplete task). If scope is ambiguous, inspect `specs/` and choose the latest `p-###-*` phase with incomplete tasks. Legacy `phase-XXX/task-YYY.md` specs may be read when they already exist, but do not create new legacy spec paths. Tell the user which scope was selected.

## Step 1: Prepare The Branch

Never implement directly on `main`, `master`, or `trunk`.

1. Run `git status --short` and `git branch --show-current`.
2. If the current branch is `main`, `master`, or `trunk`:
   - If the working tree is clean, create and switch to a new branch before making any changes.
   - If the working tree is dirty with unrelated changes, do not discard them. Ask whether to leave them alone, commit them separately, or stash them before branching.
3. If the current branch is already a non-protected feature branch, continue on it rather than creating another one.
4. Name the branch `<type>/<short-description>`, where type is `feature`, `bugfix`, `hotfix`, `refactor`, `chore`, `docs`, or `test`, derived from the selected scope (e.g. `feature/p-002-agent-workspace-t-003`).

## Step 2: Read Spec Context

Before implementation:
1. Read the selected `specs/p-###-[repo-or-multi]/impl.md`.
2. Read every selected `specs/p-###-[repo-or-multi]/t-###.md`.
3. Search `knowledgebase/` for references relevant to the task domain, architecture, business flow, and previous lessons.
4. Read `AGENTS.md`, `WORKFLOW.md` if present, and relevant agent definitions under `.agents/agents/`.
5. Inspect available workspace skills under `.agents/skills/` before falling back to global skills.

Do not begin implementation until the current spec intent, task objectives, existing knowledgebase guidance, and available specialist roles are understood.

## Step 3: Normalize Task Metadata

Every selected task file must include these sections before or during execution: **Agent Assignment**, **Knowledgebase References**, **Progress**, and **Knowledgebase Updates**. Add the missing ones with:

```bash
python3 .agents/skills/spec-planner/scripts/specs.py normalize <phase> <task>
```

The script appends only the sections the file does not already have (a `Knowledge References` or `Knowledge Updates` section counts), copied from `assets/task-metadata.md`, before `## Status`. It never rewrites existing content. Then replace every `[...]` placeholder with real content: the primary agent, supporting agents, and skills (including `caveman` when available), the knowledgebase files that matter and why, and the updates needed.

If no relevant knowledgebase reference exists, create one in the appropriate `knowledgebase/` directory before or during implementation and reference it from the task.

If `specs.py` is unavailable or fails, say so and add the sections by hand from `assets/task-metadata.md`.

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
- Comments that explain why the agent chose an approach when that explanation belongs in `t-###.md`, `impl.md`, a commit message, or knowledgebase entry.

When editing code:
1. Prefer clear names, small functions, and direct structure over explanatory comments.
2. Put implementation notes, tradeoffs, and follow-up work in the task file or knowledgebase, not in source comments.
3. Before marking a task complete, run `/code-cleanup --diff <merge-target>` (from the `code-cleanup` skill): it lists the unnecessary comments on the lines you changed, asks which to delete, and deletes only the approved ones. Keep only documentation comments that add durable value. If `code-cleanup` is not installed, remove unnecessary comments by hand under the same rules.

## Step 6a: Plan Parallel Waves

Skip this step when only one task is selected. Otherwise work out which of the selected tasks can run at the same time before starting any of them:

```bash
python3 .agents/skills/spec-planner/scripts/specs.py waves <phase> [--max-parallel N] [--cleared "<gate text>"]
```

The script orders the unfinished tasks into waves from each task's `## Depends On` and `## Affected Paths`. Tasks with overlapping paths (equal, parent and child, or a shared hot file such as `CHANGELOG.md`, `package.json`, a lockfile, or `test/pack.test.js`) never share a wave. A task with no `## Affected Paths` runs alone. The delivery task runs alone, last. A `## Depends On` entry that is not a task id (for example "PR #13 merged") is a **gate**: the task is `gated` until the user confirms it. It reports each task as `ready`, `waiting`, `gated`, `running`, `blocked`, `in-review`, or `done`, with the reason (`depends:t-002`, `paths:<path>`, `gate:<text>`, `unknown-footprint`), and exits `1` naming a dependency cycle. Tasks claimed through `agent-coordination` or marked In Progress are `running`.

1. Show the plan as a table: wave, tasks, and why each task is not in an earlier wave.
2. For every gate, ask the user whether it is cleared. Re-run `waves` with `--cleared "<gate text>"` for the ones that are.
3. Ask once: run the waves **in parallel** as shown, or **sequentially**. Do not ask again per wave unless a merge conflicts or tests fail.

**Parallel mode** is allowed only when this CLI can spawn subagents with worktree isolation (for example Claude Code's Agent tool with `isolation: "worktree"`). Never drive another CLI from outside or by keystrokes. For each wave:
1. The orchestrator (this session) stays on the working branch. Each task gets its own worktree and branch `<working-branch>--t-###` (task id from `waves`, never typed by hand), cut from the working branch's current HEAD.
2. Start one subagent per task, in the role from the task's Agent Assignment. Each subagent claims its task (`coord.py claim` with its worktree and paths), follows Steps 2 to 8 for that task only, and commits on its task branch. It never pushes, never merges, and never edits files outside its task's `## Affected Paths` plus its own `t-###.md`. It does not change `impl.md` or run `set-status`; it reports a summary and its test results instead.
3. Only the orchestrator edits `impl.md`, through `specs.py set-status`, so parallel tasks never write the same file.
4. Before starting a wave, run the full test suite on the working branch; do not start a wave on a failing baseline. When the wave finishes, merge the task branches into the working branch in task-number order and run the full test suite after each merge. A merge fails the check when the suite has a failure the baseline did not have. If a merge conflicts or the check fails, stop merging that wave, abort or revert that merge, mark the task `Blocked` with the reason, and keep the tasks already merged.
5. Remove the task worktrees and branches, release the claims (`coord.py release`), re-run `waves`, and start the next wave.

**Sequential mode** is the fallback when subagents are unavailable (Codex, agy, or any CLI without them) or the user prefers it. Run the tasks one at a time in `order` from `waves`, using Step 6 for each. Under tandem, several implementer panes can each run `/spec-implement`: `coord.py claim` stops two panes from taking the same task, and `waves` shows claimed tasks as `running`.

A blocked or failed task only holds back the tasks that depend on it. Unrelated tasks in later waves still run.

## Step 6: Execute One Task At A Time

Before the first task, run `python3 .agents/skills/agent-coordination/scripts/coord.py whoami` and `register`. If `coord.py` is unavailable or fails, say so and track work in `specs/in-progress.md` only.

For each task:
0. Claim it: `coord.py claim <phase> <task> --branch <branch> --worktree <path> --paths <affected paths>`. On a conflict, pick another unblocked task. Never take over a stale claim without asking the user via `coord.py ask`. If `coord.py overview` shows another implementer session is active, work in a dedicated worktree or branch.
1. Mark the task as in progress in both `t-###.md` and `impl.md`: `python3 .agents/skills/spec-planner/scripts/specs.py set-status <phase> <task> "In Progress"`.
2. Implement only the scoped objective.
3. Update task progress immediately after each meaningful milestone, run `coord.py heartbeat --task "<phase> <task>"`, and check `coord.py answers --mine`. Apply each answer, then `coord.py ack <id>`.
4. Keep the `Knowledgebase References` section accurate as new references are discovered.
5. Update the knowledgebase with decisions, reusable patterns, business flow changes, or lessons learned.
6. Record implementation notes in the task file with changed files and key decisions.
7. Run the narrowest useful validation first, then broader validation when appropriate.
8. Record validation results in the task file.
9. Request or run a security review when the task touches risky surfaces.
10. Run the code comment discipline cleanup (`/code-cleanup --diff <merge-target>`, see Step 5) before review or completion.
11. For a delivery task, deliver every repository involved. If the agent workspace is itself a git repository, include it (specs, knowledgebase, `AGENTS.md`) on a non-protected branch; open and merge a PR only when it has a remote, otherwise commit on the branch and ask the user how to integrate.
12. Mark the task complete only when objectives, knowledgebase updates, validation, security review, and comment cleanup are done or explicitly documented as not applicable. Then run `specs.py set-status <phase> <task> Done`; it refuses (exit 1, `missing`) until the task's objectives, testing, and security checklists are fully checked.

Questions and blocking:
- Ask with `coord.py ask --task ... --blocking-scope "<what is blocked>" "<question>"` and keep working on everything outside that scope.
- Mark `Blocked` only the blocked objective, never the whole phase.
- If the user answers directly in the CLI, record it with `coord.py answer <id> "<text>" --via cli`, then `ack` it, so the Queries tab updates.
- Run `coord.py release <phase> <task>` when the task is `Done`, `Blocked`, or abandoned.

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

Change a task's status only with `python3 .agents/skills/spec-planner/scripts/specs.py set-status <phase> <task> <Pending|In Progress|Blocked|In Review|Done>`. One call updates the task's `## Status`, the `impl.md` Timeline label, and the `impl.md` Tasks checkbox together, so the two files cannot disagree. If the script is unavailable or fails, say so and edit the same three places by hand. Always keep progress synchronized in both places:
- `specs/p-###-[repo-or-multi]/t-###.md`: detailed progress, assignment, references, implementation notes, validation, and knowledgebase updates.
- `specs/p-###-[repo-or-multi]/impl.md`: task list status and phase-level timeline/status.

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
- Never mark a task `Done` if `impl.md` and `t-###.md` disagree.
- Never run tasks with overlapping `## Affected Paths` in the same wave, and never run a task with unknown paths beside another task.
- Run tasks in parallel only through this CLI's own subagent feature with worktree isolation, or through separate panes. Never push, merge, or edit `impl.md` from a subagent.
- Change statuses with `specs.py set-status`, and add missing task metadata with `specs.py normalize`, falling back to manual edits only when the script is unavailable.
- Never think inside code comments. Source comments are for durable code documentation only.
- Never leave unnecessary code comments. Keep documentation-generator comments only when they add value, such as PHPDoc, JSDoc, Swagger/OpenAPI, or equivalent.
- Always claim a task through `agent-coordination` before implementing it when coordination is available, and release it afterwards.
- Never let an unanswered question stop unrelated work.
- Keep changes surgical and scoped to the selected task.
