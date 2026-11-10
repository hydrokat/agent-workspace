---
name: symlink
description: Use when the user asks to link, symlink, or add a directory or sibling repository to codebase-symlinks/, or invokes /symlink, optionally with a path such as /symlink ../api-service or /symlink ~/code/web --name web. Validates the directory, creates a relative symlink in codebase-symlinks/ with symlink.py, and refreshes knowledgebase/context-history/codebase-map.md so agents may work in the linked codebase.
---

# Symlink

## Overview

Use this skill to link one directory into `codebase-symlinks/` so it becomes an explicitly linked codebase under the workspace's boundary rules. The link is relative, so the workspace and its siblings can move together. The codebase map is refreshed after every link.

## Workflow

### 1. Resolve The Directory

- If the user gave a path, use it as given. Relative paths resolve against the current directory, and `~` is expanded.
- If the user gave no path, list the candidates and ask which one to link:

```bash
python3 .agents/skills/symlink/scripts/symlink.py --siblings
```

It prints JSON `siblings`, each with `name`, `path`, `purpose`, and `linked_as` (the existing link name, or `null`). Show the unlinked ones and wait for the user's choice.

### 2. Link

```bash
python3 .agents/skills/symlink/scripts/symlink.py <path> [--name <link-name>] [--dry-run]
```

The script prints JSON with a `status`:

- `created`: the link was made at `link` (pointing to `relative_target`) and the map at `map` was refreshed.
- `preserved`: the same link already existed; the map was refreshed.
- `already-linked`: the directory is already linked under another name, reported in `link`. Nothing changed.
- `would-create`: `--dry-run` only; nothing changed.
- `refused` (exit code 1): `reason` says why. The script refuses a missing path, a file, the workspace itself, a parent of the workspace (a symlink loop), a directory already inside the workspace, an invalid `--name`, and a name already taken by a different link or a real path. It never overwrites or removes anything.

On a name clash, offer a `--name` and run again only after the user agrees.

If the script is missing or fails, say so and fall back to the manual steps: check the same refusal cases, run `ln -s <relative path to target> codebase-symlinks/<name>` from the workspace root, then run `python3 .agents/skills/workspace-init/scripts/link_codebases.py` and answer `q` to refresh the map without linking anything else.

### 3. Report

- Tell the user the link path, the target, and that the codebase map was refreshed.
- Read the new row in `knowledgebase/context-history/codebase-map.md`. If its purpose line is vague (for example only the directory name), suggest a short, specific one and update it when the user agrees.
- If `warning` is present, pass it on.

## Mandates

- **Trigger**: This skill should be easy to invoke with `/symlink`.
- **User-Named Targets Only**: Link only a directory the user named or chose from the sibling list. Never link a directory on your own initiative.
- **Non-Destructive**: Never replace, retarget, or delete an existing entry in `codebase-symlinks/`.
- **Script-First**: Link with `symlink.py`, falling back to the manual steps only when the script is unavailable.
- **Boundary Rules Still Apply**: A link makes the codebase reachable; read `knowledgebase/context-history/codebase-map.md` before working in it, as `AGENTS.md` requires.
