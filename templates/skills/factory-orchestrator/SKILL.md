---
name: factory-orchestrator
description: "Guides a Factory Mode Orchestrator session through ideation (/idea or chat), feasibility, non-blocking questions, direction selection, planning, and post-Gate-2 team control via scripts/factory.py. Load when TANDEM_ROLE=orchestrator is set."
---

# Factory Orchestrator

## Overview

When `TANDEM_ROLE=orchestrator` is set (with or without `TANDEM_FACTORY_IDEA` pre-populated), guide each idea from raw description to a reviewable plan and post-Gate-2 execution using `python3 .agents/skills/factory-orchestrator/scripts/factory.py <subcommand>`. All state transitions live in `tandem`; never build shell strings or bypass `factory.py`.

If the user invokes `/idea <idea text>`, run the `/idea` skill in Factory Mode (`idea.py mode` -> `factory.py ensure-idea` -> `/feasibility --save` -> `factory.py feasibility` -> `idea.py gate` / `factory.py assume` -> `factory.py select-direction` -> `factory.py plan` -> `idea.py result`), which automatically reuses a bound blank `Inbox` placeholder or creates and binds a new Factory idea and drives Steps 1–6 below up to Gate 2.

## Procedure

1. **Orient / Create Idea.** Run `python3 .agents/skills/factory-orchestrator/scripts/factory.py show` when an idea is already bound. If the bound idea already has a description (the user wrote it when starting the idea), treat it as the idea text and do not overwrite it; `ensure-idea` keeps it and reuses that idea. If no idea is bound yet, if the bound idea has no description, or if the user asks to start a new idea (via `/idea <idea text>` or in chat), write the description to a temporary file and run `factory.py ensure-idea --title "<title>" --text-file <path>` (which reuses a blank `Inbox` placeholder via `describe`, or creates and binds a new Factory idea via `new --bind-current --start-discovery`). Pass `--idea <idea-id>` on subsequent commands whenever working with a newly created idea.
2. **Feasibility.** Follow the feasibility procedure of the `/idea` skill (which builds on `/feasibility` and `locate.py`; reuse it rather than copying its steps). Write the study to a temporary file (or `specs/feasibility/YYYY-MM-DD-<slug>.md` via `/feasibility --save`) and store it with `factory.py [--idea <id>] feasibility --file <path> --verdict <verdict> --risk <risk>`.
3. **Questions.** Ask only what changes the plan. Before asking, run `factory.py [--idea <id>] question-check`. Ask with `python3 .agents/skills/agent-coordination/scripts/coord.py ask --task "idea:<idea-id>" --blocking-scope "<scope>" "<question>"`. Keep working on everything a question does not block. When the ceiling is reached, or the answer would not change the plan, record an assumption with `factory.py [--idea <id>] assume --text "<text>"` instead.
4. **Direction.** When the user has chosen a feasible direction (or `idea.py gate` returns `proceed`), run `factory.py [--idea <id>] select-direction`.
5. **Plan.** Write a plan with the five required sections (`Scope`, `Acceptance criteria`, `Risks`, `Estimates`, `Workflow recommendation`) to a temporary file and store it with `factory.py [--idea <id>] plan --file <path> [--workflow <workflow>]`. Revise and store again when answers arrive.
6. **Hand over (Gate 2).** Tell the user the plan is ready to review and that only they can mark it ready, in the Factory tab, the web UI, or via `tandem factory idea ready <idea-id> --confirm <idea-id>`.
7. **Team Control (Post-Gate 2).** Once a run is queued or running (when `orchestrator_may_choose` is enabled):
   - Consult the cost-aware recommendation first: `factory.py recommend --run <run> --stage <stage> [--complexity low|medium|high] [--required-tier economy|standard|premium]`. It returns the cheapest capable model among signed-in CLIs (tier from the stage floor, slice complexity, and your hints) with its tier, cost basis, and candidate count. Prefer the lowest capable tier; raise `--complexity` only when the slice really needs a stronger model.
   - Choose per-stage workers with `factory.py assign --run <run> --stage <stage> --cli <cli> --model <model> [--role <role>]`. User explicit assignments always take precedence. When you do not assign, the default is the recommendation.
   - After a failed job, `factory.py relaunch` automatically moves default assignments up one tier (cheapest next model) and never changes assignments made by the user or by you; use `recommend` and `assign` to change those deliberately.
   - Monitor worker progress with `python3 .agents/skills/factory-orchestrator/scripts/team_watch.py --run <run>` (reads `activity.json`, `result.json`, and `agent list --json` first; falls back to `agent capture` only when files are missing or stale).
   - Send short clarifying guidance to a live worker by writing a file and calling `factory.py send --run <run> --job <job> --file <path>`.
   - Relaunch wedged jobs with `factory.py relaunch --run <run> --job <job>` and terminate idle/unwanted worker panes with `factory.py terminate --run <run> --job <job> --confirm <job>`.

## Hard Rules

- Never acknowledge a risky sign-in method for the user. If `tandem factory login risk --json` shows a CLI as `risky`, or a run pauses with `auth risk not acknowledged`, tell the user which CLI and why and give them `tandem factory login acknowledge-risk <cli> --method <method> --confirm <cli>` (or the TUI/web dialog); only they can confirm it. Prefer assigning a CLI whose risk is `ok` when one is ready.
- Workers are always interactive tmux panes. Never launch an AI CLI with `-p`/`--print`/`run` or any other non-interactive mode, and do not suggest it.

- Never run `tandem factory idea ready` or `revoke-ready`. Only the user applies the tag.
- Never call `tmux` directly or access credential files; all worker communication goes through `factory.py` / `tandem factory agent`.
- Do not start implementation, create branches, or edit the codebase during ideation.
- Never put secrets into a study, plan, assumption, or question.
- State every assumption explicitly with `factory.py assume`; do not hide one inside the plan.

## Commands

| Command | Purpose |
|---|---|
| `show [--idea <id>]` | Show the current (or specified) idea and its events |
| `ensure-idea --title <t> --text-file <f> [--idea <id>]` | Reuse a bound blank Inbox placeholder via `describe`, keep a bound Inbox idea that already has a user-written description, or create and bind a new Factory idea |
| `new --title <t> --text-file <f> [--no-bind]` | Create a new Factory idea in Ideation and bind the current orchestrator |
| `describe --title <t> --text-file <f> [--idea <id>]` | Set or update idea title and description |
| `feasibility --file <f> [--verdict <v>] [--risk <r>] [--idea <id>]` | Store a feasibility study artifact |
| `question-check [--idea <id>]` | Check whether the open-question ceiling is reached |
| `assume --text <t> [--idea <id>]` | Record an explicit assumption on the idea |
| `select-direction [--idea <id>]` | Advance the idea from Ideation to Planning |
| `plan --file <f> [--workflow <w>] [--idea <id>]` | Store a plan artifact |
| `recommend --run <r> --stage <s> [--complexity <c>] [--required-tier <t>]` | Least-cost capable CLI/model for a stage, with tier, cost basis, and candidates |
| `assign --run <r> --stage <s> --cli <c> --model <m>` | Assign CLI/model for a stage (respects user priority) |
| `agents --run <r>` | List jobs, pane status, and activity for a run |
| `send --run <r> --job <j> --file <f>` | Send a message file to a running worker pane |
| `capture --run <r> --job <j> [--lines N]` | Capture recent worker pane lines when needed |
| `terminate --run <r> --job <j> --confirm <j>` | Terminate a worker pane cleanly |
| `relaunch --run <r> --job <j>` | Relaunch a worker pane with current assignment |
