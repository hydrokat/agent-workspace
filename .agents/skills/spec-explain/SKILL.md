---
name: spec-explain
description: Use when the user asks what a spec phase is about, what it fixes or develops, its status or scope, or invokes /spec-explain, optionally with a phase such as /spec-explain p-002 or /spec-explain p-002 t-003. Reads the phase through specs.py and explains its bug fixes, new features and enhancements, reasons, scope, and progress. Read-only.
---

# Spec Explain

## Overview

Use this skill to explain a phase under `specs/` so a reader can tell what it fixes, what it develops, why, where, and how far along it is, without opening every task file. Every fact (counts, statuses, types, paths) comes from `specs.py`. The agent writes only the prose summaries.

## Step 0: Parse The Request

| Invocation | Scope |
|---|---|
| `/spec-explain` | The active phase (latest with an incomplete task) |
| `/spec-explain p-002` (also `002`, `2`, or the full directory name) | That phase |
| `/spec-explain p-002 t-003` | One task in that phase |

## Step 1: Load The Data

```bash
python3 .agents/skills/spec-planner/scripts/specs.py resolve [<phase>]
python3 .agents/skills/spec-planner/scripts/specs.py show <phase> [--task <t-###>]
```

If `resolve` exits `1`, the phase was not found or no phase is incomplete. Say so and list the valid phases from its `valid` field. If `specs/` has no phases, say so and suggest `/spec-plan`. If it exits `2`, the argument is invalid: say what a valid argument looks like.

If `specs.py` is missing or fails, say so and read `specs/<phase>/impl.md` and the task files by hand, following the same sections below.

Also read `knowledgebase/context-history/key-decisions.md` for decisions that belong in **Why**. Read a task file only when its `description` is too thin to explain.

## Step 2: Map The Data To The Template

Fill `assets/explanation.md` from these fields. Do not state a fact that is not in the output or the files above.

<!-- data-map -->
| Section | Source |
|---|---|
| Title, summary | `dir`, `title`, `objectives`, `scope_type` |
| What it fixes | tasks with `type` Bug Fix: `title`, `description`, `status` |
| What it develops | tasks with `type` New Feature or Enhancement: `title`, `description`, `status` |
| Why | `objectives`, `decisions`, `architecture`, key-decisions note |
| Where | `repositories`, `primary_repo`, `affected_paths`, `branches` |
| State | `counts`, `status`, `depends_on`, `external_gates`, `open_questions`, `warnings` |
<!-- /data-map -->

Rules:
- A Delivery task is not a fix or a development item. Mention it only in **State**.
- If no task has `type` Bug Fix, write `— none —` under **What it fixes**, and note any fix described inside a mixed task.
- For **Why**, include dropped or deferred items and their reasons when a decision or the key-decisions note records them.
- The **State** bar is `█` per done tenth and `░` for the rest, then `done/total`. **Next** is the first task that is not Done.
- With `--task`, explain only that task: description, type, status, checklists (`objectives`, `testing`, `security`), `affected_paths`, `depends_on`, and gates.
- Report any `warnings` from `specs.py` as gaps in the specs.

## Step 3: Present

Print the filled template in chat. Offer next steps: `/spec-implement <phase>` for pending work, or `/explain <module>` for how a changed area works.

## Mandates

- **Trigger**: Easy to invoke with `/spec-explain`.
- **Read-Only**: Never edit specs or other files. Write a file only if the user asks to save.
- **Script-First**: Counts, statuses, types, and paths come from `specs.py`, not from free reading.
- **Honest Gaps**: Say when a section has no data instead of inventing it.
- **Clear Not-Found**: When the phase is missing, list the valid phases.
