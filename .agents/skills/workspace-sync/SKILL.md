---
name: workspace-sync
description: Use when the user asks to regenerate Gemini, Claude, or Codex compatibility from the base workspace files. Ask which targets to sync first, allow multiple selections, then sync the selected compatibility outputs from AGENTS, agents, and skills sources of truth.
---

# Workspace Sync Skill

This skill syncs the workspace for one or more selected compatibility targets while keeping `AGENTS.md`, `.agents/agents`, and `.agents/skills` as the only source-of-truth files that should be edited directly.

## Workflow

### 1. Synchronization Trigger
When the user requests a sync (e.g., via `/workspace-sync`), ask which compatibility targets they want:
- `gemini`
- `claude`
- `codex`

The user may select multiple targets in one run.

Do not assume a default target if the user has not specified one in the request.

### 2. Execution
After the user chooses one or more targets, run the sync script in the workspace root with those targets as arguments:
`bash .agents/skills/workspace-sync/scripts/workspace-sync.sh gemini claude`

Example valid invocations:
- `bash .agents/skills/workspace-sync/scripts/workspace-sync.sh gemini`
- `bash .agents/skills/workspace-sync/scripts/workspace-sync.sh claude codex`
- `bash .agents/skills/workspace-sync/scripts/workspace-sync.sh gemini claude codex`

### 3. Verification
Confirm to the user that:
- For `gemini`: `GEMINI.md` is linked to `AGENTS.md` when safe to do so, agents have been transformed and copied to `.gemini/agents`, and skills have been copied to `.gemini/skills`.
- For `claude`: `CLAUDE.md` is linked to `AGENTS.md` when safe to do so, agents have been copied to `.claude/agents` in Claude-compatible format, and skills have been copied to `.claude/skills`.
- For `codex`: `AGENTS.md` remains the canonical instructions file, `.agents/agents` remains the canonical agent directory, and `.agents/skills` remains the canonical skill directory.

Only report the targets that were selected by the user.

## Rules
- **Ask First:** Always ask the user which compatibility targets to sync before running the script, unless the user already specified the targets explicitly.
- **Multiple Selection:** Support syncing multiple targets in a single execution.
- **Source of Truth:** `AGENTS.md`, `.agents/agents`, and `.agents/skills` remain the canonical source material.
- **Base Workflow:** The intended workflow is to edit only `AGENTS.md`, `.agents/agents`, or `.agents/skills`, then run `workspace-sync` to regenerate compatibility outputs.
- **Base File Safety:** Never overwrite `AGENTS.md`, `.agents/agents`, or any existing non-generated compatibility document such as `GEMINI.md` or `CLAUDE.md`.
- **Generated Outputs:** Treat `.gemini/` and `.claude/` compatibility artifacts as generated outputs. They may be refreshed during sync and should not be edited manually.
- **Gemini Transformation:** Replace conceptual tool definitions with Gemini-compatible wildcard tool access in `.gemini/agents`.
- **Gemini Skill Sync:** Copy workspace skills into `.gemini/skills` so skill updates propagate from the base workspace.
- **Claude Transformation:** Omit the `tools` field so Claude can inherit all available tools in `.claude/agents`.
- **Claude Skill Sync:** Copy workspace skills into `.claude/skills` so skill updates propagate from the base workspace.
- **Codex Compatibility:** Treat Codex as the native workspace format; validate and preserve `AGENTS.md`, `.agents/agents`, and `.agents/skills` rather than transforming them into a separate vendor directory.
- **Non-Destructive Base Files:** Clear only generated compatibility outputs before re-syncing to prevent stale definitions.
- **Safe Linking:** Create compatibility document symlinks only when the target file is missing or already managed as a link to `AGENTS.md`. Otherwise, preserve the existing file and warn the user.
- **Constraint:** Do not modify the original `.agents/agents` directory content.
