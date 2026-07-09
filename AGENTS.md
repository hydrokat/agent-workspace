# AGENTS.md - Project Workspace

This file serves as the foundational guide for contributors working within this workspace. It defines the architectural standards, operating expectations, and directory navigation required for consistent development.

## Workspace Overview

A reusable project workspace for planning, implementation, and documentation.

### System Architecture
n/a

### Core Directory Map
- **[`/.agents/agents`](./.agents/agents/)**: **Agent Definitions.** Vendor-agnostic role specifications for delegated work.
- **[`/.agents/skills`](./.agents/skills/)**: **Reusable Skills.** Task-focused instructions that can be shared across agents and contributors.
- **[`/knowledgebase`](./knowledgebase/)**: **Reference Material.** Contains guidelines, best practices, and historical context.
- **[`/specs`](./specs/)**: **Active Task Tracking.** Implementation plans (`impl.md`) and task-specific breakdowns.
- **`src/`**: (Optional) Application or library source code.

## Operating Principles

### 1. Research-First Approach
Before modifying code, contributors should:
- Consult the `knowledgebase/` for relevant patterns.
- Review the current `specs/phase-018-fix-reset-post-vouching/impl.md` to understand the broader context.
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
- **Knowledgebase:** If a new pattern or "lesson learned" emerges, document it in `knowledgebase/best-practices/`.

### 5. Engineering Standards
- **Surgical Updates:** Minimize noise; change only what is necessary to fulfill the objective.
- **Type Safety:** All implementation must prioritize strict typing and clear interfaces.
- **Server-Side Data Only**: **ALL** database interactions (Supabase queries/mutations) MUST be performed server-side via Server Components or Server Actions. Direct client-side database interaction is strictly prohibited.
- **Validation:** No task is complete without verified automated tests or documented manual validation when tests are unavailable.

---
*Refer to this document as the single source of truth for workspace expectations.*
