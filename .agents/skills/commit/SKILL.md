---
name: commit
description: Use when the user asks to create, prepare, review, or split Git commits. Enforces focused commits, one context per commit, and the workspace commit message format with task type, description, Co-Authored by, and Supervised by lines.
---

# Commit

Use this skill to turn completed workspace changes into clean, focused Git commits.

## Workflow

1. Inspect the pending changes with `git status --short` and targeted diffs.
2. Group changes by context: one feature, bug fix, refactor, doc update, config change, or test change per commit.
3. If unrelated contexts are present, split them into multiple commits using explicit path selection or partial staging.
4. Derive `task_type` from the commit context, using concise lowercase types such as `feature`, `fix`, `refactor`, `docs`, `test`, `chore`, `config`, or `style`.
5. Get the supervising username from `git config user.name` unless the user explicitly provides another name.
6. Use the active AI agent name for `Co-Authored by` unless the user explicitly provides another name.
7. Show the proposed commit grouping and message before committing when the user did not explicitly authorize making commits.

## Message Format

Format every commit message exactly as:

```text
task_type: short description of task

Commit description here. Multi-line
is allowed

Co-Authored by: AI Agent name
Supervised by: Username of git user
```

## Context Rules

- Keep each commit within one context.
- Split commits when files or hunks represent different user-facing tasks, risk profiles, or review concerns.
- Do not mix formatting-only changes with behavioral changes unless formatting is required for that exact change.
- Do not mix generated compatibility output with source skill or agent edits unless the generated output only reflects that same source edit.
- Avoid broad `git add .` when multiple contexts are present; stage exact paths or hunks instead.
- If a clean split is impossible because a file contains interleaved contexts, ask before committing or explain the tradeoff clearly.

## Commit Description Guidance

- Explain what changed and why it matters.
- Mention validation performed when relevant.
- Keep the first line short and imperative enough for Git history scanning.
- Use multiple body lines only when they add review value.
