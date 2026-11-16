---
name: idea
description: Use when the user wants to take a raw feature or product idea through feasibility assessment and spec planning, or invokes /idea. Chains /feasibility --save, gates on the verdict via idea.py, and invokes /spec-planner with the recommended approach. Supports --stage feasibility|plan|all, --report <path>, and unattended --auto mode.
---

# Idea

## Overview

`/idea` chains `/feasibility` and `/spec-planner` with a deterministic verdict gate in between. It keeps no ledger of its own: its only durable outputs are the saved feasibility report under `specs/feasibility/` and the phase under `specs/p-###-…/`.

```text
/idea <idea text> [--auto] [--stage feasibility|plan|all] [--report <path>]
```

- `<idea text>`: What to build, in the user's words. Required unless `--stage plan --report <path>` is given.
- `--stage`: `feasibility` runs Steps 1–3 and stops with status `assessed`; `plan` runs Step 4 from a saved `--report`; `all` (default) runs everything.
- `--report <path>`: Saved feasibility report under `specs/feasibility/` to plan from (required with `--stage plan`).
- `--auto`: Unattended mode. Never asks questions, applies documented defaults, and ends with one `idea_result` JSON line.

## Step 0: Parse

If neither `<idea text>` nor `--report` is provided:
- Interactive: ask the user what the idea is.
- `--auto`: stop immediately and emit `idea_result` with status `invalid-input`.

When `<idea text>` is provided, compute the slug (never interpolate idea text unquoted):

```bash
python3 .agents/skills/idea/scripts/idea.py slug "<idea text>"
```

If `idea.py` is unavailable, say so and derive a lowercase kebab-case slug (max 60 chars, fallback `idea`) and today's `YYYY-MM-DD` date by hand.

## Step 1: Feasibility (Skip When `--stage plan`)

Invoke `/feasibility` with the idea text and `--save`. Always save the report so Step 2, `--stage plan`, and callers can read it from disk at `specs/feasibility/YYYY-MM-DD-<slug>.md`.

## Step 2: Read Verdict From Disk

Read the machine-readable verdict from the saved report rather than memory:

```bash
python3 .agents/skills/idea/scripts/idea.py verdict "specs/feasibility/<report>.md"
```

If `idea.py` is unavailable, say so and parse the `<!-- feasibility: {...} -->` comment under the report title. If the report has no valid feasibility line, stop with status `failed`.

## Step 3: Gate

Run the gate policy script:

```bash
python3 .agents/skills/idea/scripts/idea.py gate --verdict "<verdict>" --effort "<effort>" [--auto]
```

Act on the returned `decision`:
- `proceed`: continue (for `feasible-with-caveats` in `--auto` mode, copy the caveats into the plan's assumptions).
- `ask` (`feasible-with-caveats` interactive): show the caveats and ask `"Plan it with these caveats?"`. If no, stop and emit `idea_result` with status `declined`.
- `stop` with `needs-spike`:
  - Interactive: list each unknown and how to resolve it, and offer to plan the spike as its own small phase.
  - `--auto`: stop and emit `idea_result` with status `needs-spike`.
- `stop` with `not-feasible`:
  - Interactive: show the blocker and the nearest feasible alternative, and offer to rerun `/idea` on the alternative.
  - `--auto`: stop and emit `idea_result` with status `not-feasible`.

Effort does not block, but for `L` or `XL` pass a note to `/spec-planner` to expect more than one phase.

If `--stage feasibility` was requested, stop here and emit `idea_result` with status `assessed`.

## Step 4: Plan

Invoke `/spec-planner` with:
- the idea text,
- the saved feasibility report path, and
- the report's **Recommended** approach (plus caveats as assumptions when applicable).

`/spec-planner` runs its full workflow without skipping any step. When `--auto` is set, invoke `/spec-planner` in **Unattended Mode**: it must ask nothing, use its suggested branch and primary merge target, leave new phases in the backlog without invoking `/pm-new-sprint`, record scope-changing ambiguities under `Open Questions` in `impl.md` with affected tasks marked `Blocked`, and list every default taken under `Decisions Taken Unattended` in `impl.md`.

## Step 5: Result

Print a short human summary using `assets/result.md`, then emit the single `idea_result` JSON line as the **last line of output**:

```bash
python3 .agents/skills/idea/scripts/idea.py result --status "<status>" [--report "<path>"] [--verdict "<verdict>"] [--effort "<effort>"] [--phase "<phase>" ...] [--error "<text>"]
```

Valid statuses: `planned`, `assessed`, `needs-spike`, `not-feasible`, `declined`, `invalid-input`, `failed`.

## Mandates

- **Trigger**: `/idea`.
- **Chain, do not copy**: call `/feasibility` and `/spec-planner`; never restate their steps.
- **Gate is the script's**: the decision comes from `idea.py gate`.
- **No ledger**: write nothing except what the two skills write.
- **Unattended means silent**: with `--auto`, never ask; record defaults; stop instead of guessing.
- **Last line is the result**: always end with the `idea_result` JSON line, on success and on failure.
- **Hard blockers stay hard**: a `not-feasible` verdict is never planned, in any mode.
