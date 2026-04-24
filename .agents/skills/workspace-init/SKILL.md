---
name: workspace-initializer
description: Initializes a new agent-optimized workspace with knowledgebase/ (and subdirectories), and specs/ directories.
metadata:
  category: setup
  triggers: workspace-init, workspace-initializer, init workspace, bootstrap workspace, initialize workspace
---

# Workspace Initializer

Use this skill when the user invokes `/workspace-init` or asks to bootstrap a new agent-optimized workspace.

This skill initializes the base workspace structure and creates starter files only when they do not already exist.

## What It Initializes

- `knowledgebase/`
- `knowledgebase/guidelines/`
- `knowledgebase/best-practices/`
- `knowledgebase/business-flows/`
- `knowledgebase/context-history/`
- `specs/`
- `.agents/agents/`
- `.agents/skills/`

## Starter Files

When missing, this skill can also create:
- `AGENTS.md`
- `WORKFLOW.md`
- `knowledgebase/README.md`
- `knowledgebase/guidelines/README.md`
- `knowledgebase/best-practices/README.md`
- `knowledgebase/business-flows/README.md`
- `knowledgebase/context-history/README.md`
- `specs/README.md`

## Workflow

### 1. Choose Target Directory
- Default to the current working directory unless the user specifies another target.

### 2. Run Initialization
Execute:

```bash
bash .agents/skills/workspace-init/scripts/workspace-init.sh
```

To initialize another directory:

```bash
bash .agents/skills/workspace-init/scripts/workspace-init.sh /path/to/workspace
```

### 3. Preserve Existing Work
- Never overwrite existing files.
- Create only missing directories and starter files.
- Treat existing `AGENTS.md`, `WORKFLOW.md`, `knowledgebase/`, and `specs/` content as authoritative.

### 4. Follow-Up
After initialization:
- use `/knowledge` to refine the knowledgebase content
- use the specs planner skill to create the first phase plan
- use `/workspace-sync` if Gemini or Claude compatibility outputs are needed

## Mandates

- **Trigger**: This skill should be easy to invoke with `/workspace-init`.
- **Non-Destructive**: Initialize missing structure only; do not overwrite existing files.
- **Base First**: Create the base workspace files and directories before any compatibility sync.
- **Keep It Minimal**: Create only the starter structure needed to begin using the workspace.
