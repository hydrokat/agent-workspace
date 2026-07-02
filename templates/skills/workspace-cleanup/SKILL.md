---
name: workspace-cleanup
description: Use when the user asks to clean up the workspace, find stray or leftover files, tidy up the project directory, or invokes /workspace-cleanup. Scans the workspace for files and directories that may not belong (OS/editor cruft, stale backups, empty or orphaned directories, unrecognized files outside the expected structure), lists every candidate with a reason, and asks the user which ones to delete before removing anything.
---

# Workspace Cleanup

## Overview

Use this skill to find files and directories in the workspace that look out of place — leftovers from editors, stale backups, orphaned generated output, or files that don't match the workspace's expected structure — and let the user decide what to remove. Never delete anything without explicit approval.

## What Counts As "May Not Belong"

- **OS/editor cruft**: `.DS_Store`, `Thumbs.db`, `*.swp`, `*.swo`, `*~`, stray `.idea/` or `.vscode/` content not intentionally committed.
- **Stale backups**: `*.bak` files left behind by the `update` command or manual edits.
- **Empty directories**: directories with no files and no purpose documented in `AGENTS.md`.
- **Orphaned generated output**: files under `.claude/` or `.gemini/` with no corresponding source in `.agents/agents/` or `.agents/skills/`.
- **Stale scratch files**: old files in `.tmp/` that no longer correspond to any active work in `specs/in-progress.md`.
- **Unrecognized files**: files or directories at the workspace root or inside managed directories that are not covered by `AGENTS.md`'s Core Directory Map, `.gitignore`, or a recognizable source/build layout.

Do not flag:
- Anything tracked in version control and unmodified.
- `specs/`, `knowledgebase/business-flows/`, `knowledgebase/context-history/`, and other explicitly user-owned content.
- Anything documented in `AGENTS.md`'s Core Directory Map or `System Architecture` section.
- Dependency directories already covered by `.gitignore` (e.g. `node_modules/`) unless they are unexpectedly large, duplicated, or clearly abandoned.

## Workflow

### 1. Gather Context

- Read `AGENTS.md`'s Core Directory Map and System Architecture to learn the expected structure.
- Read `.gitignore` to know what's already excluded from version control.
- Run `git status --short` to distinguish tracked, modified, and untracked files.
- Read `specs/in-progress.md` if present, to cross-check whether `.tmp/` content or in-progress files are still active.

### 2. Scan For Candidates

Walk the workspace (root and managed directories) and check for each category listed above. For every candidate, capture:
- Relative path.
- Category (OS/editor cruft, stale backup, orphaned generated output, empty directory, stale scratch file, unrecognized file).
- Why it was flagged.
- Last-modified date when it helps the user judge staleness.

Skip anything covered by the "Do not flag" list.

### 3. Present Findings

Group and list every candidate before taking any action:

```markdown
## Workspace Cleanup Candidates

### OS/editor cruft
- `path/to/file` — reason, last modified

### Stale backups
- `path/to/file.bak` — reason, last modified

### Orphaned generated output
- `.claude/skills/old-skill/` — no matching `.agents/skills/old-skill/`

### Empty directories
- `path/to/dir/` — empty, not documented in AGENTS.md

### Unrecognized files
- `path/to/file` — not covered by Core Directory Map or .gitignore
```

If no candidates are found, say so clearly and stop.

### 4. Ask Before Deleting

- Never delete anything automatically.
- Ask the user which items to delete — individually, by category, or all flagged items.
- Only delete items the user explicitly approves.
- If uncertain whether a file is safe to remove, list it as "needs review" and ask rather than assume.

### 5. Delete And Report

- Delete only the approved items.
- Report what was deleted, what was left in place and why, and any residual candidates that still need manual review.

## Mandates

- **Trigger**: This skill should be easy to invoke with `/workspace-cleanup`.
- **Non-Destructive By Default**: Never delete without explicit per-item or per-category user approval.
- **Explain Every Flag**: Every listed candidate must include a reason it was flagged.
- **Preserve Ambiguous Files**: When uncertain, list the file as "needs review" instead of assuming it is safe to delete.
- **Respect Ignore Rules**: Skip files already covered by `.gitignore` unless they've grown unexpectedly large, duplicated, or old.
- **Respect Ownership**: Never flag or delete `specs/`, `knowledgebase/business-flows/`, `knowledgebase/context-history/`, or other user-owned content.
