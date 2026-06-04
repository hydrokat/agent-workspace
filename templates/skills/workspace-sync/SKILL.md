---
name: workspace-sync
description: Use when the user asks to regenerate Antigravity, Claude, or Codex compatibility from the base workspace files. Runs the CLI update command which handles all provider sync, model injection, and verification.
---

# Workspace Sync Skill

This skill regenerates compatibility outputs for all selected providers from the canonical source files (`AGENTS.md`, `.agents/agents`, `.agents/skills`).

## How to Sync

Run the update command from the workspace root:

```bash
npx hydrokat/agent-workspace update
```

The CLI reads the saved manifest (providers, project name) and regenerates all selected provider outputs with model injection.

### Sync specific providers

```bash
npx hydrokat/agent-workspace update --providers claude,antigravity
```

### Verify without writing

```bash
npx hydrokat/agent-workspace doctor
```

## What Gets Regenerated

For each selected provider:
- Agent files with correct `model:` injected from `models.json`
- Skill directory copy (Claude only — Antigravity and Codex use `.agents/skills/` natively)
- Compat doc symlink (`CLAUDE.md` / `GEMINI.md`) when safe

The `AGENTS.md` managed block (operating principles + model table) is also refreshed on every update.

## Rules

- **Source of Truth:** `AGENTS.md`, `.agents/agents`, and `.agents/skills` remain the canonical source material. Do not hand-edit `.claude/` or `.gemini/` files — they will be overwritten on next update.
- **Managed files are overwritten:** Changed files are backed up to `*.bak` before overwriting.
- **Project content is never touched:** `specs/`, `knowledgebase/business-flows/`, and user knowledgebase entries are never modified.
- **Safe Linking:** Compat doc symlinks are only created when the target file is missing or already a managed link to `AGENTS.md`.
- **Antigravity skills:** Antigravity reads skills from `.agents/skills/` directly — no separate `.gemini/skills/` copy is created or needed.
