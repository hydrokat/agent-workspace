---
name: factory-orchestrator
description: "Guides a Factory Mode Orchestrator session through ideation, feasibility, non-blocking questions, direction selection, and planning via scripts/factory.py. Load when TANDEM_ROLE=orchestrator and TANDEM_FACTORY_IDEA are set."
---

# Factory Orchestrator

## Overview

When `TANDEM_ROLE=orchestrator` and `TANDEM_FACTORY_IDEA` are set, guide the idea from raw description to a reviewable plan using `python3 .agents/skills/factory-orchestrator/scripts/factory.py <subcommand>`. All state transitions live in `tandem`; never build shell strings or bypass `factory.py`.

## Procedure

1. **Orient.** Run `python3 .agents/skills/factory-orchestrator/scripts/factory.py show`. If the idea has no description, ask the user what outcome they want, write it to a temporary file, and run `factory.py describe --title "<title>" --text-file <path>`.
2. **Feasibility.** Follow the feasibility procedure of the `/idea` skill (which builds on `/feasibility` and `locate.py`; reuse it rather than copying its steps). Write the study to a temporary file and store it with `factory.py feasibility --file <path> --verdict <verdict> --risk <risk>`.
3. **Questions.** Ask only what changes the plan. Before asking, run `factory.py question-check`. Ask with `python3 .agents/skills/agent-coordination/scripts/coord.py ask --task "idea:<idea-id>" --blocking-scope "<scope>" "<question>"`. Keep working on everything a question does not block. When the ceiling is reached, or the answer would not change the plan, record an assumption with `factory.py assume --text "<text>"` instead.
4. **Direction.** When the user has chosen a feasible direction, run `factory.py select-direction`.
5. **Plan.** Write a plan with the five required sections (`Scope`, `Acceptance criteria`, `Risks`, `Estimates`, `Workflow recommendation`) to a temporary file and store it with `factory.py plan --file <path> [--workflow <workflow>]`. Revise and store again when answers arrive.
6. **Hand over.** Tell the user the plan is ready to review and that only they can mark it ready, in the Factory tab or the web UI.
7. **Team Control (Post-Gate 2).** Once a run is queued or running (when `orchestrator_may_choose` is enabled):
   - Consult the cost-aware recommendation first: `factory.py recommend --run <run> --stage <stage> [--complexity low|medium|high] [--required-tier economy|standard|premium]`. It returns the cheapest capable model among signed-in CLIs (tier from the stage floor, slice complexity, and your hints) with its tier, cost basis, and candidate count. Prefer the lowest capable tier; raise `--complexity` only when the slice really needs a stronger model.
   - Choose per-stage workers with `factory.py assign --run <run> --stage <stage> --cli <cli> --model <model> [--role <role>]`. User explicit assignments always take precedence. When you do not assign, the default is the recommendation.
   - After a failed job, `factory.py relaunch` automatically moves default assignments up one tier (cheapest next model) and never changes assignments made by the user or by you; use `recommend` and `assign` to change those deliberately.
   - Monitor worker progress with `python3 .agents/skills/factory-orchestrator/scripts/team_watch.py --run <run>` (reads `activity.json`, `result.json`, and `agent list --json` first; falls back to `agent capture` only when files are missing or stale).
   - Send short clarifying guidance to a live worker by writing a file and calling `factory.py send --run <run> --job <job> --file <path>`.
   - Relaunch wedged jobs with `factory.py relaunch --run <run> --job <job>` and terminate idle/unwanted worker panes with `factory.py terminate --run <run> --job <job> --confirm <job>`.

## Hard Rules

- Never run `tandem factory idea ready` or `revoke-ready`. Only the user applies the tag.
- Never call `tmux` directly or access credential files; all worker communication goes through `factory.py` / `tandem factory agent`.
- Do not start implementation, create branches, or edit the codebase during ideation.
- Never put secrets into a study, plan, assumption, or question.
- State every assumption explicitly with `factory.py assume`; do not hide one inside the plan.

## Commands

| Command | Purpose |
|---|---|
| `show` | Show the current idea and its events |
| `describe --title <t> --text-file <f>` | Set or update idea title and description |
| `feasibility --file <f> [--verdict <v>] [--risk <r>]` | Store a feasibility study artifact |
| `question-check` | Check whether the open-question ceiling is reached |
| `assume --text <t>` | Record an explicit assumption on the idea |
| `select-direction` | Advance the idea from Ideation to Planning |
| `plan --file <f> [--workflow <w>]` | Store a plan artifact |
| `recommend --run <r> --stage <s> [--complexity <c>] [--required-tier <t>]` | Least-cost capable CLI/model for a stage, with tier, cost basis, and candidates |
| `assign --run <r> --stage <s> --cli <c> --model <m>` | Assign CLI/model for a stage (respects user priority) |
| `agents --run <r>` | List jobs, pane status, and activity for a run |
| `send --run <r> --job <j> --file <f>` | Send a message file to a running worker pane |
| `capture --run <r> --job <j> [--lines N]` | Capture recent worker pane lines when needed |
| `terminate --run <r> --job <j> --confirm <j>` | Terminate a worker pane cleanly |
| `relaunch --run <r> --job <j>` | Relaunch a worker pane with current assignment |
