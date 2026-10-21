---
name: spec-compile
description: "Compiles specs into completed and incomplete archives. A spec is complete when all its t-### tasks are done, tested, and security audited. Triggers on /spec-compile with optional filter args — all (default), completed, incomplete, or a phase range like 001-010. Always updates the knowledgebase first, then clears old compiled output before writing fresh results."
---

# Spec Compile

## Overview

Scans phases in `specs/`, updates the knowledgebase, clears previous compiled output, then writes `specs/compiled/completed.md` (all-passing phases) and/or `specs/compiled/incomplete.md` (phases with gaps), scoped to whatever filter the user passed.

## Step 0: Parse the Argument

Extract the filter from the invocation before doing any file work.

| Invocation | Filter mode | Phases scanned | Files written |
|---|---|---|---|
| `/spec-compile` | `all` | all phases | both |
| `/spec-compile all` | `all` | all phases | both |
| `/spec-compile completed` | `completed` | all phases | `completed.md` only |
| `/spec-compile incomplete` | `incomplete` | all phases | `incomplete.md` only |
| `/spec-compile 001-010` | range `001`–`010` | phases 001 to 010 | both |
| `/spec-compile 005` | single `005` | phase 005 only | both |

Range format is always zero-padded numbers separated by `-`. A single number targets one phase. Any unrecognised argument defaults to `all` and notes the fallback to the user.

## Step 1: Update the Knowledgebase

Before touching compiled output, update the knowledgebase to reflect current project state. Scope the update to phases that will be scanned (per the filter).

- Read each phase's `impl.md` and task files for decisions, completed work, and lessons learned.
- Write to `knowledgebase/context-history/`, `best-practices/`, `business-flows/`, or `guidelines/` as appropriate.
- Use `business-analyst` to identify what changed and `technical-writer` to produce clean documentation.
- Only update knowledgebase entries that have new or changed information — do not rewrite unchanged content.

## Step 2: Compile

Steps 2 to 5 are one deterministic command. Run it after Step 1:

```bash
python3 .agents/skills/spec-planner/scripts/specs.py compile [all|completed|incomplete|NNN|NNN-NNN]
```

It reads the `p-###-[repo-or-multi]/impl.md` and `t-###.md` files for the phases in scope (skipping `specs/compiled/`; legacy `phase-XXX/task-YYY.md` specs are read, never created or rewritten), classifies every task, clears the old output, and writes the new files. It prints JSON: `filter`, `scope`, `written`, `phases` (`in_scope`, `complete`, `incomplete`), `tasks`, `note` (set when the argument was unrecognised and fell back to `all`), and `warnings` (for example a requested phase directory that does not exist).

### Classification

A task is **complete** when it satisfies all three criteria. See [references/completion-criteria.md](references/completion-criteria.md) for the exact detection patterns the script applies.

| Criterion | Required signal |
|---|---|
| Done | All `## Objectives` checkboxes are `[x]`, or status marker shows done |
| Tested | `## Testing` / `## Test Results` section has at least one `[x]` item |
| Security Audited | `## Security` / `## Security Audit` section has at least one `[x]` item |

A phase is **complete** when every one of its tasks is complete. A phase with no task files is incomplete. A task file that cannot be read is reported with unknown (❓) criteria and its path.

### Output

| Filter mode | Cleared before writing | Written |
|---|---|---|
| `all` | the entire `specs/compiled/` directory | `completed.md` and `incomplete.md` |
| `completed` | `specs/compiled/completed.md` | `completed.md` |
| `incomplete` | `specs/compiled/incomplete.md` | `incomplete.md` |
| range / single | `completed.md` and `incomplete.md` | both, with a scope note |

The files are rendered from `assets/completed.md` and `assets/incomplete.md`; [references/output-format.md](references/output-format.md) describes the layout. `specs/compiled/` must be a real directory; the script refuses to clear a link.

### Manual Fallback

If `specs.py` is unavailable or fails, say so and do the same work by hand: read the specs in scope, classify each task with the criteria above, delete the output files named in the table, and write the files following `references/output-format.md`.

## Mandates

- Always run Step 1 (knowledgebase update) before clearing or writing compiled output.
- Never modify original spec files — read only.
- Clear before write — never append to stale compiled files.
- Compile with `specs.py compile`, falling back to the manual steps only when the script is unavailable.
- If `specs/` has no phases in scope, the output states that no matching specs were found.
- Report a one-line summary to the user after each compiled file is written: how many phases and tasks were included.
