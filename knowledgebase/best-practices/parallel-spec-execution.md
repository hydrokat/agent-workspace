# Parallel Spec Execution

How `/spec-implement` runs several tasks of one phase at the same time without them colliding. The plan is computed by `specs.py waves`; the agent only decides whether to run it in parallel.

## Planning rules

- Dependencies come from each task's `## Depends On`. A `t-###` entry is an edge. Any other entry (for example `p-001 t-011 / PR #13 merged`) is a gate: the task stays `gated` until the user clears it (`--cleared`).
- Tasks whose `## Affected Paths` overlap never share a wave. Overlap means equal paths, parent and child, a glob root, or a shared hot file (`CHANGELOG.md`, `package.json`, lockfiles, `test/pack.test.js`, matched by name at any depth). Order follows the dependency order, then the task number.
- A task with no `## Affected Paths` has an unknown footprint and runs alone. The delivery task runs alone, last. The plan never guesses a footprint.
- Blocked and in-review tasks, and everything that depends on them, are held out of the waves. Unrelated tasks still run (see `non-blocking-agent-questions.md`).
- Running work (In Progress or claimed through `coord.py`) is placed first, and conflicting tasks wait behind it.

## Execution rules

- Parallel mode only through the CLI's own subagent feature with worktree isolation, or through separate panes. Never drive a CLI from outside (see `tos-safe-orchestration.md`).
- One worktree and branch per task: `<working-branch>--t-###`, cut from the working branch's current HEAD. The task id comes from `waves`.
- Subagents implement, test, and commit on their task branch only. They never push, merge, edit outside their Affected Paths and their own `t-###.md`, or touch `impl.md`.
- The orchestrator alone runs `set-status` (so `impl.md` has one writer), merges task branches in task-number order, and runs the full suite after each merge.
- Start a wave only from a green baseline. A merge fails the check when the suite has a failure the baseline did not have. On a conflict or failure: stop that wave's merging, revert that merge, mark the task `Blocked`, keep the rest.
- Clean up each wave (worktrees, branches, claims) and re-run `waves` before the next one.

## Lesson

A dry run with one shared test file that already expected both tasks' results failed after the first merge. Tests belong to the task that adds them, and the baseline must be green first.
