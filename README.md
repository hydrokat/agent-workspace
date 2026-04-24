# agent-workspace

A reusable workspace for managing agent instructions, role-based agent definitions, workspace skills, and compatibility outputs for multiple AI runtimes.

# Table of Contents
- [Installation](#installation)
- [Usage](#usage)
- [Features](#features)
- [Contributing](#contributing)
- [License](#license)

## Installation

No package installation is required to use the workspace structure itself.

To get started:

```bash
git clone <your-repo-url> agent-workspace
cd agent-workspace
```

To initialize a fresh workspace structure as a human, use the workspace initializer:

```bash
bash .agents/skills/workspace-init/scripts/workspace-init.sh
```

To initialize a different directory:

```bash
bash .agents/skills/workspace-init/scripts/workspace-init.sh /path/to/new-workspace
```

The initializer is non-destructive:
- it creates missing directories and starter files
- it preserves any existing `AGENTS.md`, `WORKFLOW.md`, `knowledgebase/`, and `specs/` content

The base files you should edit directly are:
- `AGENTS.md`
- `.agents/agents/`
- `.agents/skills/`

Optional compatibility outputs are generated from those base files:
- `.gemini/`
- `.claude/`

## Usage

### 1. Start From The Base Workspace

Use these files and directories as the source of truth:
- `AGENTS.md` for workspace-wide rules and operating expectations
- `WORKFLOW.md` for the shared execution style and decision rules
- `.agents/agents/` for vendor-agnostic agent definitions
- `.agents/skills/` for workspace-local skills
- `knowledgebase/` for project context, guidelines, and reusable practices
- `specs/` for implementation plans and task breakdowns

### 2. Initialize The Workspace

There are two common ways to initialize the workspace:

If you are working through an agent interface:
- trigger `/workspace-init`

If you are working directly in a terminal:

```bash
bash .agents/skills/workspace-init/scripts/workspace-init.sh
```

To scaffold another directory:

```bash
bash .agents/skills/workspace-init/scripts/workspace-init.sh /path/to/workspace
```

The initializer creates the base structure for:
- `.agents/agents/`
- `.agents/skills/`
- `knowledgebase/`
- `knowledgebase/guidelines/`
- `knowledgebase/best-practices/`
- `knowledgebase/business-flows/`
- `knowledgebase/context-history/`
- `specs/`

### 3. Update Agents, Skills, Or Workspace Rules

The intended workflow is:
1. Edit `AGENTS.md`, `.agents/agents/`, or `.agents/skills/`.
2. Keep generated compatibility files out of manual editing.
3. Run the workspace sync flow when you want Gemini or Claude compatibility outputs refreshed.

### 4. Sync Compatibility Outputs

The workspace includes a `workspace-sync` skill that regenerates compatibility targets from the base files.

Valid sync targets are:
- `gemini`
- `claude`
- `codex`

Example:

```bash
bash .agents/skills/workspace-sync/scripts/workspace-sync.sh gemini claude
```

What each target does:
- `gemini` regenerates `GEMINI.md`, `.gemini/agents`, and `.gemini/skills` when safe to do so.
- `claude` regenerates `CLAUDE.md`, `.claude/agents`, and `.claude/skills` when safe to do so.
- `codex` validates the native workspace layout, where `AGENTS.md`, `.agents/agents`, and `.agents/skills` are already canonical.

### 5. Follow The Workspace Rules

Important expectations from `AGENTS.md`:
- Research first before making changes.
- Search for a relevant workspace skill first, then global skills.
- If no relevant skill exists, continue without blocking.
- Treat generated compatibility outputs as derived artifacts, not source files.

## Features

### Vendor-Agnostic Base Workspace
The workspace keeps agents and skills in a neutral base format under `.agents/`, then syncs compatibility outputs only when needed.

### Role-Based Agent Catalog
The workspace includes specialized agents for investigation, orchestration, backend, frontend, QA, security, documentation, and terminal-heavy work.

### Skill-First Execution Model
Contributors are expected to check for relevant skills before starting a task, which helps keep workflows consistent and reusable.

### Compatibility Sync
The `workspace-sync` skill regenerates Gemini and Claude compatibility outputs from the base files while preserving the canonical Codex-style workspace.

### Workspace Initialization
The `workspace-initializer` skill can bootstrap a new workspace structure for a human or agent without overwriting existing files.

### Documentation And Planning Structure
The workspace includes:
- `knowledgebase/` for shared context and standards
- `specs/` for implementation plans and task tracking
- `WORKFLOW.md` for execution guidelines

## Contributing

When contributing to this workspace:

1. Update the base files first:
   `AGENTS.md`, `.agents/agents/`, `.agents/skills/`, `knowledgebase/`, or `specs/`.
2. Avoid manually editing generated files under `.gemini/` or `.claude/`.
3. Re-run the compatibility sync only after the base files are in the desired state.
4. Keep changes surgical and aligned with the existing structure.
5. Document new patterns or decisions in the knowledgebase when they are reusable.

There is currently no separate `CONTRIBUTING.md`, so this README and `AGENTS.md` are the primary contribution guides.

## License

No license file is currently included in this workspace.
