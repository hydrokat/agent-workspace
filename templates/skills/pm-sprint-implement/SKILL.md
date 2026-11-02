---
name: pm-sprint-implement
description: "Executes an entire sprint in MoSCoW and RICE priority order—either concurrently across multi-agent sessions without collision or in parallel waves across disjoint task paths—enforcing atomic task claims, worktree isolation, TDD gates, caveman mode, comment discipline, and sprint close readiness. Trigger with /pm-sprint-implement, optionally specifying a sprint id."
---

# PM Sprint Implement

## Overview

Use this skill to execute an entire sprint across multiple `specs/p-###-*` phases in MoSCoW (`Must Have` → `Should Have` → `Could Have`) and RICE priority order without collisions. It supports both multi-agent autonomous execution (multiple agents or panes picking work independently) and single-session orchestrator wave execution.

Act as a strict, realistic `tech-lead-orchestrator` and `project-manager`. Never proceed on ungrounded optimism; enforce verified automated tests, strict atomic coordination, and boundary isolation at every step.

## Step 0: Resolve Active Sprint Scope

Determine the target sprint before touching any code:

```bash
python3 .agents/skills/pm-new-sprint/scripts/sprints.py resolve [<sprint>]
```

1. If `<sprint>` is provided (e.g. `s-001`, `001`, `1`, or full slug `s-001-user-auth`), resolve that sprint.
2. If omitted, `resolve` automatically selects the active `open` or `locked` sprint.
3. If no active sprint exists, inform the user and suggest running `/pm-new-sprint` or falling back to `/spec-implement`.
4. Inspect the sprint status: both `open` and `locked` sprints can be implemented. If `locked`, work strictly within the committed phases—do not add new phases without an authorized hotfix bypass.

## Step 1: Session Registration & Coordination Overview

Before claiming or inspecting work, initialize agent coordination:

```bash
python3 .agents/skills/agent-coordination/scripts/coord.py whoami
python3 .agents/skills/agent-coordination/scripts/coord.py register --model "<model>"
python3 .agents/skills/agent-coordination/scripts/coord.py overview
```

1. If `coord.py` is unavailable or fails, say so explicitly and fall back to manual tracking in `specs/in-progress.md`.
2. Inspect `coord.py overview`: note all active sessions, existing claims, and current worktrees. Identify whether other implementers are actively working in the workspace.

## Step 2: Choose Execution Mode

Render `assets/execution-plan.md` to communicate active scope and select the appropriate execution mode:

### Mode A: Concurrent Multi-Agent Mode
Use this mode when multiple agents or tandem panes are concurrently running `/pm-sprint-implement` or `/spec-implement`:

1. Query next available work:
   ```bash
   python3 .agents/skills/pm-new-sprint/scripts/sprints.py next-work [<sprint>]
   ```
   `sprints.py next-work` automatically skips tasks that are already `Done`, tasks whose dependencies are incomplete, and tasks currently claimed by any active session in `coord.py`.
2. Attempt an atomic claim before touching any files:
   ```bash
   python3 .agents/skills/agent-coordination/scripts/coord.py claim <phase> <task> --branch <branch> --worktree <worktree> --paths <affected_paths...>
   ```
3. If the claim succeeds (exit code `0`), proceed to Step 3.
4. If the claim fails (exit code `1` due to race condition with another agent), immediately re-run `sprints.py next-work` to claim the next eligible candidate.
5. If another implementer session is active, work in a dedicated git worktree (`git worktree add -b <branch>--<task>`) to isolate working trees.
6. Record ownership in `specs/in-progress.md` declaring role, session ID, task, and branch/worktree. Mark the task `In Progress` via:
   ```bash
   python3 .agents/skills/spec-planner/scripts/specs.py set-status <phase> <task> "In Progress"
   ```
7. Never take over stale claims without confirming with the user via `coord.py ask`.

### Mode B: Orchestrator Parallel Wave Mode
Use this mode when this CLI can spawn isolated subagents with worktree isolation (e.g. Claude Code Agent tool with `isolation: "worktree"`):

1. Query available parallel candidates:
   ```bash
   python3 .agents/skills/pm-new-sprint/scripts/sprints.py next-work [<sprint>]
   ```
   and inspect `specs.py waves <phase>` for disjoint dependency waves within each phase.
2. Verify that candidates have completely non-overlapping `Affected Paths`. No shared hot files (`package.json`, lockfiles, `CHANGELOG.md`) may run in the same wave.
3. For each disjoint task, cut a dedicated task worktree and branch `<working-branch>--t-###` from the current HEAD.
4. Spawn a subagent for each task in its assigned role. The subagent claims its task via `coord.py claim`, executes Step 3, runs task validation, and commits on its task branch.
5. Invariants for subagents:
   - Subagents never push or merge branches.
   - Subagents never edit `impl.md` or `sprint.md`.
   - Single-Writer Safety: only the orchestrator session updates `impl.md` (via `specs.py set-status`) and `sprint.md`.
6. When subagents complete, merge each task branch into the working branch sequentially, running the full regression test suite after each merge.
7. Clean up task worktrees and branches, release claims with `coord.py release <phase> <task>`, and proceed to the next wave.

### Mode C: Sequential Fallback Mode
When subagents and concurrent panes are not available:
1. Fetch the next task via `sprints.py next-work`.
2. Execute the task completely following Step 3.
3. Repeat until all workable tasks in the sprint are complete.

## Step 3: Enforce Quality Gates Per Task

Every task executed under `/pm-sprint-implement` must satisfy the full `/spec-implement` quality gates:

1. **Task Normalization & Roles**: Ensure the task file has all required metadata via `python3 .agents/skills/spec-planner/scripts/specs.py normalize <phase> <task>`. Explicitly act in or delegate to the assigned role (e.g. `senior-backend-dev`, `frontend-dev`, `qa-engineer`, `security-auditor`).
2. **Caveman Style**: Use the `caveman` skill whenever possible for progress reports, implementation notes, and operational summaries to preserve token bandwidth.
3. **TDD & Automated Verification**: Write failing unit/integration tests before writing implementation code. Ensure tests pass before proceeding. Run every test command (including the regression suite after each merge) through `python3 .agents/skills/test-failures/scripts/run_tests.py -- <test command>` and read only the failures it reports.
4. **Code Comment Discipline**: Treat comments as documentation only. Do not narrate obvious code or write scratchpad thinking in comments. Run `/code-cleanup --diff <merge-target>` to remove unnecessary comments before task completion.
5. **Security & Knowledgebase**:
   - Run a security audit if the task touches auth, inputs, data handling, or permissions.
   - Update `knowledgebase/` with reusable patterns or lessons learned, or document why no update was required in the task file.
6. **Heartbeat & Questions**:
   - Report progress and send heartbeats at milestones:
     ```bash
     python3 .agents/skills/agent-coordination/scripts/coord.py heartbeat --task "<phase> <task>"
     ```
   - Check non-blocking queries:
     ```bash
     python3 .agents/skills/agent-coordination/scripts/coord.py answers --mine
     ```
   - Apply answers and acknowledge with `coord.py ack <id>`.
7. **Task Completion**:
   - Set status to `Done`:
     ```bash
     python3 .agents/skills/spec-planner/scripts/specs.py set-status <phase> <task> Done
     ```
   - Release the claim:
     ```bash
     python3 .agents/skills/agent-coordination/scripts/coord.py release <phase> <task>
     ```
   - Remove the completed entry from `specs/in-progress.md`.

## Step 4: Sprint Progression Loop & Close Readiness

1. After completing a task, immediately call `sprints.py next-work [<sprint>]`.
2. If further workable tasks exist in the current sprint, continue executing in MoSCoW order (`Must Have` → `Should Have` → `Could Have`).
3. If no workable tasks remain because peer agents actively hold claims, report current active session claims and gracefully yield without colliding.
4. When all tasks across all committed phases in the sprint reach `Done`:
   - Run sprint linting:
     ```bash
     python3 .agents/skills/pm-new-sprint/scripts/sprints.py lint [<sprint>]
     ```
   - Check sprint status:
     ```bash
     python3 .agents/skills/pm-new-sprint/scripts/sprints.py status [<sprint>]
     ```
   - Present a completion summary to the user and recommend running `/pm-sprint-close` to finalize deliverables and produce stakeholder close reports.

## Mandates

- **Project Scope & Boundary Isolation**: Strictly enforce project boundaries. The agent workspace belongs to a specific project only; work strictly within this workspace repository and explicitly linked `codebase-symlinks/`. Never cross into unlinked sibling or parent directories.
- **Atomic Coordination**: Never modify source code without first acquiring an atomic claim via `coord.py claim`.
- **Branch Protection**: Never commit or implement directly on `main`, `master`, or `trunk`. Always work on dedicated feature branches or worktrees.
- **Single-Writer Invariant**: Phase `impl.md` and `sprint.md` are updated solely by the primary orchestrator session. Subagents never write to plan files.
- **Zero Collision**: Never take over an active claim held by another session without explicit user confirmation via `coord.py ask`.
- **Quality Gates**: Every task requires automated test validation, code comment cleanup, knowledgebase updates, and clean `specs.py set-status` transitions before release.
