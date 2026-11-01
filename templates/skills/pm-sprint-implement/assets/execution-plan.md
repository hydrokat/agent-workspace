# Sprint Execution Plan: [SPRINT_ID] — [SPRINT_TITLE]

- **Status**: [STATUS]
- **Capacity**: [CAPACITY_DAYS] days (Committed: [COMMITTED_EFFORT] days)
- **Active Mode**: [Concurrent Multi-Agent Mode | Orchestrator Parallel Wave Mode | Sequential Mode]

## Sprint Phases Priority (MoSCoW & RICE)

| Phase | Priority | RICE | Status | Description |
|---|---|---|---|---|
| [PHASE_DIR] | [MOSCOW] | [RICE] | [STATUS] | [DESCRIPTION] |

## Active Work & Wave Ownership

| Wave | Phase | Task | Role | Agent/Session | Branch/Worktree | Status | Affected Paths |
|---|---|---|---|---|---|---|---|
| [WAVE] | [PHASE] | [TASK] | [ROLE] | [SESSION] | [BRANCH_WORKTREE] | [STATUS] | [PATHS] |

## Non-Overlapping Path & Isolation Validation

- **Boundary Isolation**: Workspace strictly confined to project root and `codebase-symlinks/`. No crossing to parent or unlinked siblings.
- **Wave Invariant**: Tasks running simultaneously must have disjoint `Affected Paths`. No shared hot files (`package.json`, lockfiles, `CHANGELOG.md`).
- **Coordination Gate**: Atomic claim required via `coord.py claim` before editing any file.
- **Single-Writer Safety**: Phase `impl.md` and `sprint.md` are updated only by the primary orchestrator session.
