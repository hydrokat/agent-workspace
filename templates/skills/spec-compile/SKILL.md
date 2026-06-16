---
name: spec-compile
description: "Compiles specs into completed and incomplete archives. A spec is complete when all its tasks are done, tested, and security audited. Triggers on /spec-compile with optional filter args — all (default), completed, incomplete, or a phase range like 001-010. Always updates the knowledgebase first, then clears old compiled output before writing fresh results."
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

## Step 2: Scan Specs

Read files under `specs/` for all phases in scope:
- `specs/phase-XXX/impl.md` — phase plan and task index
- `specs/phase-XXX/task-YYY.md` — individual task files

Skip `specs/compiled/` entirely (output directory, not input).

For range/single filters, only read the matching `phase-XXX/` directories. Log a warning if a requested phase directory does not exist.

## Step 3: Classify Each Task

A task is **complete** when it satisfies all three criteria. See [references/completion-criteria.md](references/completion-criteria.md) for exact detection patterns.

| Criterion | Required signal |
|---|---|
| Done | All `## Objectives` checkboxes are `[x]`, or status marker shows done |
| Tested | `## Testing` / `## Test Results` section has at least one `[x]` item |
| Security Audited | `## Security` / `## Security Audit` section has at least one `[x]` item |

A phase is **complete** when every one of its tasks is complete.

## Step 4: Clear Old Compiled Output

Before writing anything new, delete the output files that will be replaced:

| Filter mode | Files to delete |
|---|---|
| `all` | entire `specs/compiled/` directory |
| `completed` | `specs/compiled/completed.md` only |
| `incomplete` | `specs/compiled/incomplete.md` only |
| range / single | `specs/compiled/completed.md` and `specs/compiled/incomplete.md` |

Recreate `specs/compiled/` (or ensure it exists) after clearing.

## Step 5: Write Compiled Output

Write only the files determined in Step 0. See [references/output-format.md](references/output-format.md) for exact structure.

- **`specs/compiled/completed.md`** — phases (in scope) where every task passed all three criteria.
- **`specs/compiled/incomplete.md`** — phases (in scope) with at least one incomplete task, with a per-task breakdown of missing criteria.

For range/single compilations, add a scope note at the top of each file indicating which phases were included.

## Mandates

- Always run Step 1 (knowledgebase update) before clearing or writing compiled output.
- Never modify original spec files — read only.
- Clear before write — never append to stale compiled files.
- If `specs/` has no phases in scope, write a minimal output stating no matching specs were found.
- Report a one-line summary to the user after each compiled file is written: how many phases and tasks were included.
