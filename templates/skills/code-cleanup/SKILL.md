---
name: code-cleanup
description: Use when the user asks to clean up code comments, remove unnecessary or redundant comments, strip commented-out code, make code self-documenting, or invokes /code-cleanup. Scans the codebase (or the given paths) for comments that do not earn their place, lists them, asks which to delete, then deletes only the approved ones with a deterministic script and checks the result with the project's tests.
---

# Code Cleanup

## Overview

Use this skill to find source comments that add no durable value and let the user decide what to delete. Scanning and deleting are deterministic (`scripts/comments.py`). Deciding which comments are still useful is the judgment step. Never delete a comment without an explicit answer from the user.

## Step 0: Parse Scope

| Invocation | Scope |
|---|---|
| `/code-cleanup` | Whole codebase. See repos below |
| `/code-cleanup <paths…>` | Only those files or directories |
| `/code-cleanup --diff <base>` | Only comments on lines changed since `<base>` (used by `spec-implement` and the delivery task) |

Repos: `comments.py` works on one git repo at a time (`--root <dir>`). In an agent workspace with `codebase-symlinks/`, scan each linked repo and the workspace's own `src/` when present, one repo at a time, each with its own branch check and its own approval. Outside a workspace, scan the current repo.

Only tracked files are scanned. Vendored, generated, lock, minified, and binary files are skipped. Unsupported languages are reported as skipped.

## Step 1: Scan

```bash
python3 .agents/skills/code-cleanup/scripts/comments.py [--root <repo>] scan [paths…] [--diff <base>] [--include-other] --out <scratch>/scan.json --limit 50
```

Write `--out` files to a scratch location outside the repo, never inside it.

The result lists candidates with `id`, `file`, `start`/`end`, `kind`, `reason`, and exact `text`, plus counts by kind and what was kept automatically. Page with `--offset` when `total` exceeds `--limit`.

Kept automatically and never listed: documentation-generator comments (JSDoc, TSDoc, PHPDoc, Javadoc, rustdoc, Go doc comments, Swagger/OpenAPI), shebangs, encoding lines, license headers, tool directives (`eslint-disable`, `@ts-*`, `noqa`, `type: ignore`, `pragma`, `fmt: off`, `prettier-ignore`, `nolint`, `shellcheck`, and similar), and TODOs that name an owner or ticket.

Kinds: `commented-out-code`, `restates-code`, `narration`, `divider`, `empty`, `stale-todo`. `other` (explanatory comments the scanner cannot judge) is listed only with `--include-other`; use it when the user wants a full review.

If `comments.py` is missing or fails, say so and review comments manually with the same rules, listing them before any edit.

## Step 2: Review (Judgment)

Drop from the list any candidate that is useful under the `spec-implement` Step 5 rules. Keep comments that explain:
- a non-obvious *why*, an invariant, or a domain constraint,
- a compatibility requirement or workaround,
- security-sensitive behavior.

Remove from the list, and tell the user you did so, any candidate that fits these. If unsure, move it to **needs review** instead of deleting it.

## Step 3: Present

Fill `assets/candidates.md`: group by kind, then file. Show counts, what was kept, and what was skipped. If there are no candidates, say so and stop.

## Step 4: Ask

Ask which to delete: all, by kind, by file, by id, or none. Do nothing until the user answers. An unclear answer means no deletion.

## Step 5: Protect The Branch

Run `git branch --show-current` in each repo. On `main`, `master`, or `trunk`, create a `chore/code-cleanup-<date>` branch or worktree first (AGENTS.md rule 7). `comments.py remove` also refuses on those branches.

## Step 6: Delete

Write the plan outside the repo:

```json
{"scan": "<scratch>/scan.json", "ids": ["c3", "c7"], "kinds": [], "files": [], "all": false, "exclude": []}
```

A candidate is deleted when its id, kind, or file is named, or `all` is true, and its id is not in `exclude`. Drop the candidates the review step removed by listing them in `exclude`.

```bash
python3 .agents/skills/code-cleanup/scripts/comments.py [--root <repo>] remove --plan <scratch>/plan.json
```

The script re-reads every target and compares it with the scan. If a file changed since the scan, that file is left untouched and the script exits `1`: re-scan and ask again. A whole-line comment removes the line. A trailing comment is cut back to the code. A blank line is collapsed only when the removal would leave two in a row. Python results must still parse.

## Step 7: Verify And Report

1. Show `git diff --stat` and, when helpful, `git diff` for the touched files.
2. Run the project's tests. Find the runner with `python3 .agents/skills/explain/scripts/locate.py stack` when it is installed, else from the repo manifests.
3. If tests fail, report it, show the failing output, and offer to revert with `git checkout -- <files>`.
4. Report what was deleted, what was kept and why, and what still needs review. Leave committing to the user or the `commit` skill.

## Mandates

- **Trigger**: Easy to invoke with `/code-cleanup`.
- **Approval First**: Never delete without an explicit answer to the list. No answer means no deletion.
- **Script-First**: Scan and delete with `comments.py`, never by hand-editing, unless the script is unavailable.
- **Preserve Value**: Keep documentation comments, directives, license headers, and comments that explain a non-obvious why. When unsure, list as "needs review".
- **Branch Protection**: Never edit on `main`, `master`, or `trunk`.
- **Verify**: Show the diff and run tests after deleting. Offer to revert on failure.
- **Stay In Scope**: Touch only comments the user approved. Do not reformat or refactor.
