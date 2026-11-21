---
name: idea
description: Use when the user wants to take a raw feature or product idea through feasibility assessment and spec planning, or invokes /idea. Auto-detects Factory Mode (or --factory) to create/reuse a Factory idea and drive it through feasibility, Gate 1 (select-direction), and the 5-section Factory plan up to Gate 2; outside Factory Mode, chains /feasibility --save, gates on the verdict via idea.py, and invokes /spec-planner. Supports --stage feasibility|plan|all, --report <path>, --factory, and unattended --auto mode.
---

# Idea

## Overview

`/idea` takes a raw idea through feasibility and planning with a deterministic verdict gate in between:
- **Classic Mode** (default outside Factory Mode): chains `/feasibility --save` and `/spec-planner`, producing a saved feasibility report under `specs/feasibility/` and a phase under `specs/p-###-…/`.
- **Factory Mode** (auto-detected when `TANDEM_FACTORY=1`, `TANDEM_ROLE=orchestrator`, `TANDEM_FACTORY_IDEA`, or an active Factory session is running, or when `--factory` is passed): creates a new Factory idea (or reuses a bound blank `Inbox` placeholder) via `factory.py ensure-idea`, stores the feasibility study, advances Gate 1 (`select-direction`), stores the 5-section Factory plan in `Planning` state, and stops at user-only **Gate 2** (`Ready for development`), after which the Factory runner and Orchestrator worker team execute the workflow stages.

```text
/idea <idea text> [--auto] [--factory] [--stage feasibility|plan|all] [--report <path>]
```

- `<idea text>`: What to build, in the user's words. Required unless `--stage plan --report <path>` is given.
- `--factory`: Force the Factory Mode ideation-to-plan pipeline.
- `--stage`: `feasibility` runs Steps 1–3 and stops with status `assessed`; `plan` runs Step 4 from a saved `--report`; `all` (default) runs everything.
- `--report <path>`: Saved feasibility report under `specs/feasibility/` to plan from (required with `--stage plan`).
- `--auto`: Unattended mode. Never asks questions, applies documented defaults, and ends with one `idea_result` JSON line.

## Step 0: Parse & Detect Mode

If neither `<idea text>` nor `--report` is provided:
- Interactive: ask the user what the idea is.
- `--auto`: stop immediately and emit `idea_result` with status `invalid-input`.

Detect the active execution mode (`classic` or `factory`):

```bash
python3 .agents/skills/idea/scripts/idea.py mode [--factory]
```

When `<idea text>` is provided, compute the slug (never interpolate idea text unquoted):

```bash
python3 .agents/skills/idea/scripts/idea.py slug "<idea text>"
```

If `idea.py` is unavailable, say so and derive a lowercase kebab-case slug (max 60 chars, fallback `idea`) and today's `YYYY-MM-DD` date by hand.

When `mode == "factory"`, first create or reuse the Factory idea in `.agent-workspace/factory/` by writing `<idea text>` to a temporary description file and running:

```bash
python3 .agents/skills/factory-orchestrator/scripts/factory.py ensure-idea --title "<concise title (<=120 chars)>" --text-file "<temp-desc-path>"
```

Capture the returned `idea_id` (`i-xxxxxxxx`) and pass `--idea "<idea_id>"` to all subsequent `factory.py` commands.

## Step 1: Feasibility (Skip When `--stage plan`)

Invoke `/feasibility` with the idea text and `--save`. Always save the report so Step 2, `--stage plan`, and callers can read it from disk at `specs/feasibility/YYYY-MM-DD-<slug>.md`.

## Step 2: Read Verdict From Disk

Read the machine-readable verdict from the saved report rather than memory:

```bash
python3 .agents/skills/idea/scripts/idea.py verdict "specs/feasibility/<report>.md"
```

If `idea.py` is unavailable, say so and parse the `<!-- feasibility: {...} -->` comment under the report title. If the report has no valid feasibility line, stop with status `failed`.

When `mode == "factory"`, map the verdict, effort, and unknowns count into Factory Mode's `factory_verdict` (`feasible`, `feasible-with-risks`, `infeasible`) and `factory_risk` (`low`, `medium`, `high`), and store the feasibility artifact on the Factory idea:

```bash
python3 .agents/skills/idea/scripts/idea.py factory-map --verdict "<verdict>" --effort "<effort>" --unknowns <unknowns>
python3 .agents/skills/factory-orchestrator/scripts/factory.py --idea "<idea_id>" feasibility --file "specs/feasibility/<report>.md" --verdict "<factory_verdict>" --risk "<factory_risk>"
```

## Step 3: Gate

Run the gate policy script:

```bash
python3 .agents/skills/idea/scripts/idea.py gate --verdict "<verdict>" --effort "<effort>" [--auto]
```

Act on the returned `decision`:
- `proceed`: continue (for `feasible-with-caveats` in `--auto` mode, copy the caveats into the plan's assumptions; in Factory Mode, record each caveat with `python3 .agents/skills/factory-orchestrator/scripts/factory.py --idea "<idea_id>" assume --text "<caveat>"`).
- `ask` (`feasible-with-caveats` interactive): show the caveats and ask `"Plan it with these caveats?"`. If no, stop and emit `idea_result` with status `declined`. If yes, in Factory Mode record each caveat via `factory.py --idea "<idea_id>" assume --text "<caveat>"`.
- `stop` with `needs-spike`:
  - Interactive: list each unknown and how to resolve it, and offer to plan the spike as its own small phase.
  - `--auto`: stop and emit `idea_result` with status `needs-spike`.
- `stop` with `not-feasible`:
  - Interactive: show the blocker and the nearest feasible alternative, and offer to rerun `/idea` on the alternative.
  - `--auto`: stop and emit `idea_result` with status `not-feasible`.

Effort does not block, but for `L` or `XL` pass a note to `/spec-planner` (in Classic Mode) to expect more than one phase.

If `--stage feasibility` was requested, stop here and emit `idea_result` with status `assessed`.

## Step 4: Plan

### Classic Mode (`mode == "classic"`)

Invoke `/spec-planner` with:
- the idea text,
- the saved feasibility report path, and
- the report's **Recommended** approach (plus caveats as assumptions when applicable).

`/spec-planner` runs its full workflow without skipping any step. When `--auto` is set, invoke `/spec-planner` in **Unattended Mode**: it must ask nothing, use its suggested branch and primary merge target, leave new phases in the backlog without invoking `/pm-new-sprint`, record scope-changing ambiguities under `Open Questions` in `impl.md` with affected tasks marked `Blocked`, and list every default taken under `Decisions Taken Unattended` in `impl.md`.

### Factory Mode (`mode == "factory"`)

Do **not** invoke `/spec-planner` or create branches during Factory ideation (the Factory workflow's `plan` stage worker runs `specs.py` after Gate 2). Instead:

1. If `--stage plan --report <path>` was used and the Factory idea has no feasibility artifact yet, run `idea.py verdict` + `idea.py factory-map` on `<path>` and store it with `factory.py --idea "<idea_id>" feasibility --file "<path>" --verdict "<factory_verdict>" --risk "<factory_risk>"`.
2. Advance the Factory idea from `Ideation` to `Planning` (Gate 1):
   ```bash
   python3 .agents/skills/factory-orchestrator/scripts/factory.py --idea "<idea_id>" select-direction
   ```
3. Write a Factory plan markdown file with the five required `##` sections (`## Scope`, `## Acceptance criteria`, `## Risks`, `## Estimates`, `## Workflow recommendation`) based on the feasibility report's **Recommended** approach and any recorded assumptions, and store it on the Factory idea:
   ```bash
   python3 .agents/skills/factory-orchestrator/scripts/factory.py --idea "<idea_id>" plan --file "<temp-plan-path>" --workflow default-delivery
   ```
4. Stop at **Gate 2 (`Planning`)** and tell the user the plan is ready for review and that only they can mark it ready (in the TUI Factory tab, `tandem-web`, or via `tandem factory idea ready <idea_id> --confirm <idea_id>`), after which the Factory runner and Orchestrator team will execute the stages (`plan` -> `create-tests` -> `develop` -> `test` -> `review` -> `demo` -> PR).

## Step 5: Result

Print a short human summary using `assets/result.md`, then emit the single `idea_result` JSON line as the **last line of output**:

```bash
python3 .agents/skills/idea/scripts/idea.py result --status "<status>" [--report "<path>"] [--verdict "<verdict>"] [--effort "<effort>"] [--phase "<phase>" ...] [--idea "<idea_id>"] [--error "<text>"]
```

Valid statuses: `planned`, `assessed`, `needs-spike`, `not-feasible`, `declined`, `invalid-input`, `failed`.

## Mandates

- **Trigger**: `/idea`.
- **Chain, do not copy**: call `/feasibility` and `/spec-planner` (in Classic Mode) or `factory.py` (in Factory Mode); never restate their steps.
- **Gate is the script's**: the decision comes from `idea.py gate`.
- **Gate 2 stays user-only**: in Factory Mode, never call `tandem factory idea ready` or `revoke-ready`; stop at `Planning` so the user approves Gate 2.
- **No ledger**: write nothing except what the skills and `factory.py` write.
- **Unattended means silent**: with `--auto`, never ask; record defaults; stop instead of guessing.
- **Last line is the result**: always end with the `idea_result` JSON line, on success and on failure.
- **Hard blockers stay hard**: a `not-feasible` verdict is never planned, in any mode.
