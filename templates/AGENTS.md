# AGENTS.md - Project Workspace

This file serves as the foundational guide for contributors working within this workspace. It defines the architectural standards, operating expectations, and directory navigation required for consistent development.

## Workspace Overview

<!-- project:start -->
A reusable project workspace for planning, implementation, and documentation.
<!-- project:end -->

### System Architecture
n/a

### Core Directory Map
- **[`/.agents/agents`](./.agents/agents/)**: **Agent Definitions.** Vendor-agnostic role specifications for delegated work.
- **[`/.agents/skills`](./.agents/skills/)**: **Reusable Skills.** Task-focused instructions that can be shared across agents and contributors.
- **[`/codebase-symlinks`](./codebase-symlinks/)**: **Linked Codebases.** Symlinked sibling repositories for adjacent project context.
- **[`/knowledgebase`](./knowledgebase/)**: **Reference Material.** Contains guidelines, best practices, and historical context.
- **[`/specs`](./specs/)**: **Active Task Tracking.** Implementation plans (`impl.md`), task-specific breakdowns, and `in-progress.md` tracking all work currently underway.
- **[`/.tmp`](./.tmp/)**: **Agent Working Files.** Scratch space for in-progress agent output. Safe to delete at any time.
- **`src/`**: (Optional) Application or library source code.

<!-- managed:start -->
## Operating Principles

### 1. Research-First Approach
Before modifying code, contributors should:
- Consult the `knowledgebase/` for relevant patterns.
- If `codebase-symlinks/` exists, read `knowledgebase/context-history/codebase-map.md` before traversing sibling repos.
- Review the current `specs/phase-XXX/impl.md` to understand the broader context.
- Load `WORKFLOW.md` for debugging and development tasks.
- Review existing tests to prevent regressions.

### 2. Skill-First Execution
Before executing a task, contributors must look for a skill that is necessary or materially helpful for completing the task.

Skill lookup order:
- Search workspace scope first in `./.agents/skills/`.
- If no relevant workspace skill is found, search global scope for an applicable skill.
- If no relevant skill is found in workspace scope or global scope, use the `find-skills` skill to identify suggested skills and present those suggestions to the user for review before proceeding with any install.
- If no relevant skill exists in workspace scope and global scope, proceed with the task without a skill loaded.

Skill handling rules:
- Prefer the most specific relevant skill over a general one.
- Do not install a suggested skill before the user has had a chance to review it.
- If the task can be completed safely without a skill after both searches are exhausted, continue without blocking on installation.

### 3. Compatibility Sync
Treat `AGENTS.md`, `./.agents/agents/`, and `./.agents/skills/` as the only editable source-of-truth locations for workspace instructions, agents, and skills.

Compatibility sync rules:
- Do not manually edit generated compatibility artifacts under `./.gemini/` or `./.claude/`.
- Regenerate compatibility outputs by running the `workspace-sync` skill after updating the base files.
- Preserve base files over generated files if there is ever a conflict.

### 4. Documentation Sync
- **Implementation Plans:** Always update `specs/phase-XXX/impl.md` when a task status changes.
- **Task Tracking:** Create a new `task-XXX.md` for every non-trivial unit of work.
- **Progress Tracking:** Keep `specs/in-progress.md` current so the user can see what agents are actively working on.
  - List every task or spec currently in progress, including the ongoing task and its next step.
  - If a task belongs to a spec, indicate the spec name (e.g. `phase-002`).
  - If a task does not belong to a spec, label it `adhoc`.
  - Remove an entry as soon as its task is completed — this file reflects only active work, not history.
- **Knowledgebase:** If a new pattern or "lesson learned" emerges, document it in `knowledgebase/best-practices/`.

### 5. Engineering Standards
- **Surgical Updates:** Minimize noise; change only what is necessary to fulfill the objective.
- **Type Safety:** All implementation must prioritize strict typing and clear interfaces.
- **Validation:** No task is complete without verified automated tests or documented manual validation when tests are unavailable.

### 6. Script-First Execution
For repetitive or complex technical tasks:
- **Prefer Scripts:** If a script exists for the task, use it.
- **Create Skills:** If no script or skill is present for a repetitive task, create a specialized skill in `./.agents/skills/` to encapsulate the procedure.

### 7. Branch Protection
Never commit or make changes directly on `main`, `master`, or `trunk` unless the user explicitly instructs it.

Branch protection rules:
- Before implementing, check the current branch. If it is `main`, `master`, or `trunk`, create a worktree or switch to a new branch first.
- Prefer a worktree when the task should run in isolation from the current working tree (e.g. parallel work, experimentation, or when uncommitted changes must be preserved untouched).
- Otherwise, create a descriptive branch (e.g. `<type>/<short-description>`) and switch to it before making any changes.
- If already on a non-protected branch, continue on it rather than creating another one.
- Only push to or commit on `main`, `master`, or `trunk` when the user explicitly asks for it in that exact request.

### 8. Concurrent Agent Awareness
Other agents or contributors may be working on this codebase at the same time. Do not assume exclusive access.

Concurrent work rules:
- Before starting, run `git status --short` and check for in-progress work (uncommitted changes, unfamiliar branches, active worktrees) that is not yours.
- Check `specs/phase-XXX/task-YYY.md` and `impl.md` for tasks already marked `In Progress` or `Blocked` by another agent before claiming the same task.
- Never discard, overwrite, or force-push over changes you did not make without confirming with the user first.
- Prefer a dedicated branch or worktree per task so parallel agents do not collide on the same working tree.
- Pull or fetch the latest base branch before branching and before merging, and re-check for new conflicts introduced since you started.
- If a file, task, or branch appears to be actively owned by another agent, coordinate through the task file or ask the user rather than silently taking it over.

## Agent Model Recommendations

Suggested models per agent across supported providers. Pick based on available access and task complexity.
For Codex/OpenAI, prefer `gpt-5.5` for orchestration and reasoning-heavy work, `gpt-5.3-codex` for hands-on agentic coding, `gpt-5.4` for balanced professional tasks, and `gpt-5.4-mini` for fast, high-volume subagents.

| Agent | Claude | Antigravity | Codex (OpenAI) | Notes |
|---|---|---|---|---|
| `tech-lead-orchestrator` | Opus 4.7 | 2.5 Pro | `gpt-5.5` | Orchestration + architecture; top reasoning tier |
| `codebase_investigator` | Opus 4.7 | 2.5 Pro | `gpt-5.5` | System-wide investigation; deep reasoning + long context |
| `security-auditor` | Opus 4.7 | 2.5 Pro | `gpt-5.5` | Threat analysis; strongest reasoning required |
| `api-pentester` | Sonnet 4.6 | 2.0 Flash | `gpt-5.4` | Targeted security tests; balanced speed + tool-heavy analysis |
| `code-reviewer` | Sonnet 4.6 | 2.5 Pro | `gpt-5.5` | Broad diffs; high-context reasoning and review quality |
| `senior-backend-dev` | Sonnet 4.6 | 2.0 Flash | `gpt-5.3-codex` | Implementation; agentic coding and tool use |
| `frontend-dev` | Sonnet 4.6 | 2.0 Flash | `gpt-5.3-codex` | Component + UI work; hands-on coding in Codex |
| `qa-engineer` | Sonnet 4.6 | 2.0 Flash | `gpt-5.3-codex` | Test generation, debugging, and validation workflows |
| `business-analyst` | Sonnet 4.6 | 2.0 Flash | `gpt-5.4` | Requirements, specs, and structured documentation |
| `generalist` | Sonnet 4.6 | 2.0 Flash | `gpt-5.4-mini` | Batch/broad tasks; speed matters |
| `technical-writer` | Sonnet 4.6 | 2.0 Flash | `gpt-5.4` | Writing quality; balanced polish and cost |
| `terminal-agent` | Haiku 4.5 | 2.0 Flash Lite | `gpt-5.4-mini` | Fast shell execution; lightweight subagent fit |
<!-- managed:end -->

---
*Refer to this document as the single source of truth for workspace expectations.*
