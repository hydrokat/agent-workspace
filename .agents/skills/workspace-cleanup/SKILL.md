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

### 1. Scan

Run the scanner. It reads `AGENTS.md`, `.gitignore`, git state, and `specs/in-progress.md` itself, applies the "Do not flag" rules above, and saves its result for the delete step:

```bash
python3 .agents/skills/workspace-cleanup/scripts/scan_workspace.py [--stale-days N]
```

It prints JSON: `counts` by category and `candidates`, each with `path`, `kind` (`file` or `dir`), `category`, `reason`, `needs_review`, and last-`modified` date. The categories are `os-editor-cruft`, `stale-backup`, `empty-directory`, `orphaned-generated-output`, `stale-scratch` (older than 14 days by default and not referenced by `specs/in-progress.md`), and `unrecognized-file` (always `needs_review`).

If the script is missing or fails, say so and fall back to the manual scan: read `AGENTS.md`'s Core Directory Map and System Architecture, `.gitignore`, `git status --short`, and `specs/in-progress.md`, then walk the workspace for each category above.

### 2. Review The Candidates

Read the candidate list against what you know of the workspace. Move anything you are unsure about to "needs review". Add nothing the scanner did not find without telling the user why.

### 3. Present Findings

Group and list every candidate before taking any action, using the layout in `assets/candidates.md` (drop categories with no candidates, and show `needs_review` items under "Needs review"). If no candidates are found, say so clearly and stop.

### 4. Ask Before Deleting

- Never delete anything automatically.
- Ask the user which items to delete — individually, by category, or all flagged items.
- Only delete items the user explicitly approves.
- If uncertain whether a file is safe to remove, list it as "needs review" and ask rather than assume.

### 5. Delete And Report

- Delete only the approved items, with `python3 .agents/skills/workspace-cleanup/scripts/scan_workspace.py --delete <path> [<path> …]`. The script deletes a path only if it was in the last scan, is unchanged since, is inside the workspace, and is not protected (`specs/`, `knowledgebase/` other than `*.bak` files, `.git`, `AGENTS.md`). It never follows symlinks, and removes an empty directory only while it is still empty. Refused paths are reported under `refused`; tell the user why. Without the script, delete the approved items by hand after the same checks.
- Report what was deleted, what was left in place and why, and any residual candidates that still need manual review.

## Mandates

- **Trigger**: This skill should be easy to invoke with `/workspace-cleanup`.
- **Non-Destructive By Default**: Never delete without explicit per-item or per-category user approval.
- **Explain Every Flag**: Every listed candidate must include a reason it was flagged.
- **Preserve Ambiguous Files**: When uncertain, list the file as "needs review" instead of assuming it is safe to delete.
- **Respect Ignore Rules**: Skip files already covered by `.gitignore` unless they've grown unexpectedly large, duplicated, or old.
- **Respect Ownership**: Never flag or delete `specs/`, `knowledgebase/business-flows/`, `knowledgebase/context-history/`, or other user-owned content.
- **Script-First**: Scan and delete with `scan_workspace.py`, falling back to the manual steps only when the script is unavailable. The approval gate is unchanged: the script never deletes on its own.
