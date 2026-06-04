---
name: workspace-initializer
description: Initializes a new agent-optimized workspace with knowledgebase/ (and subdirectories), specs/ directories, and optional sibling codebase symlinks.
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

When the user opts in to sibling codebase linking, it also creates:
- `codebase-symlinks/`
- `knowledgebase/context-history/codebase-map.md`

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

### 3. Optional Sibling Codebases
- Ask the user whether they want to generate codebase symlinks.
- If they say no, stop after the base initializer.
- If they say yes, run:

```bash
python3 .agents/skills/workspace-init/scripts/link_codebases.py
```

The script scans one directory up from the workspace, lists sibling directories except the workspace itself, lets the user select one or more targets or `all`, creates symlinks in `codebase-symlinks/`, and refreshes `knowledgebase/context-history/codebase-map.md`.

### 4. Preserve Existing Work
- Never overwrite existing files.
- Create only missing directories and starter files.
- Treat existing `AGENTS.md`, `WORKFLOW.md`, `knowledgebase/`, and `specs/` content as authoritative.

### 5. Follow-Up
After initialization:
- use `/knowledge` to refine the knowledgebase content
- use the specs planner skill to create the first phase plan
- use `/workspace-sync` if Gemini or Claude compatibility outputs are needed
- review `knowledgebase/context-history/codebase-map.md` and tighten the purpose line for any linked repo whose role is still unclear

## Mandates

- **Trigger**: This skill should be easy to invoke with `/workspace-init`.
- **Non-Destructive**: Initialize missing structure only; do not overwrite existing files.
- **Base First**: Create the base workspace files and directories before any compatibility sync.
- **Ask First**: Ask before linking sibling codebases.
- **Keep It Minimal**: Create only the starter structure needed to begin using the workspace.
