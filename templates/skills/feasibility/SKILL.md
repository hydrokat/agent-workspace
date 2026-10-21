---
name: feasibility
description: Use when the user asks whether a feature or module can be built, whether something is possible or realistic in this codebase, how it could be built, what it would cost or risk, or invokes /feasibility. Grounds the answer in the real stack and code with locate.py, checks constraints, existing patterns, third-party terms, and active work, then reports a verdict, effort, options with a recommendation, risks, and unknowns. Read-only.
---

# Feasibility

## Overview

Use this skill to decide whether a feature or module can be built in this codebase and how, without changing code. The stack and the nearby code come from `locate.py`. The verdict, options, and recommendation are the judgment work.

## Step 0: Parse The Request

`/feasibility <feature or module> [--save]`. The subject is a capability to add, change, or remove ("add SSO login", "replace the queue with Redis"). With no subject, ask what to assess. `--save` is the only way a file is written.

## Step 1: Ground In The Codebase

```bash
python3 .agents/skills/explain/scripts/locate.py stack
python3 .agents/skills/explain/scripts/locate.py find <term> [<term> …]
```

`stack` reports, per repo (the current repo and each repo under `codebase-symlinks/`): languages, frameworks, manifests, package managers, test runners with commands, and CI. `find` returns the affected areas, similar features to copy, entry points, references, and tests.

Handle these states:
- **Stack undetected** (`detected` is false everywhere): say so, ask for the intended stack, and mark the verdict `Needs spike` unless the user supplies it.
- **No related code** (`find` returns `total` 0): treat the work as greenfield. Say so and base the approaches on the stack and the repo's conventions.
- **Several repos**: name the repo each change belongs to.

Read the files `find` points to, `AGENTS.md`, `README.md`, `knowledgebase/guidelines/`, and `knowledgebase/context-history/codebase-map.md`. If `locate.py` is missing or fails, say so and gather the same facts by hand (`git ls-files`, manifests, search).

## Step 2: Check Constraints

1. **Patterns and rules**: existing layering, naming, validation, error handling, and test style; the guidelines in `knowledgebase/`.
2. **Active and planned work**: run `python3 .agents/skills/spec-planner/scripts/specs.py list` and read the `Pending`, `In Progress`, and `Blocked` tasks' `affected_paths`. Run `python3 .agents/skills/agent-coordination/scripts/coord.py overview` when it is installed. If either script is missing, read `specs/in-progress.md` and the specs by hand. Report overlaps and whether to sequence after them.
3. **Third-party tools and services**: when the feature depends on one, check its official docs and terms (see `knowledgebase/best-practices/tos-safe-orchestration.md`). A ban or terms-of-service risk is a hard blocker: the verdict is `Not feasible` for that approach, and the report says which clause. p-001 dropped unified `agy` chat routing for this reason.
4. **Licensing, platform, and cost limits** that the stack or the dependency imposes.

## Step 3: Decide

Verdict (pick one, give a one-line reason):
- `Feasible`: fits existing patterns, no blocking unknown.
- `Feasible with caveats`: buildable, but with named constraints, risks, or a changed scope.
- `Needs spike`: one or more unknowns decide the answer; list each with how to resolve it.
- `Not feasible`: blocked by a hard constraint (terms, platform, licensing, missing capability). Offer the nearest feasible alternative.

Effort (with the main drivers):
- `S`: under a day, one module, no new dependency.
- `M`: a few days, several files in one area, or one new dependency.
- `L`: about a week or more, several modules, a migration, or a new subsystem.
- `XL`: multi-week, cross-repo, or dependent on a spike or an external party.

Give two or three approaches with trade-offs and exactly one **Recommended**.

## Step 4: Report

Fill `assets/report.md` and print it in chat. Cite `file:line` for affected areas, mark each as new or modified, and name every unknown with a way to resolve it. Offer `/spec-plan` with the recommended approach as the next step.

## Step 5: Save (Only With `--save`)

Print the report in chat first. With `--save`, also write it to `specs/feasibility/YYYY-MM-DD-<slug>.md` (`<slug>` is lowercase letters, digits, and `-`). That folder is not a `p-###-*` phase, so `specs.py` ignores it. Never overwrite an existing report: add `-2`, `-3`, and so on.

## Mandates

- **Trigger**: Easy to invoke with `/feasibility`.
- **Read-Only**: Never change code or specs. Write only the saved report, and only with `--save`.
- **Script-First**: Get the stack and affected code from `locate.py`; get active work from `specs.py` and `coord.py`.
- **Grounded**: Every claim about the codebase comes from code that was read.
- **Hard Blockers**: Treat ToS, ban, or licensing risk as a blocker, never as a footnote.
- **One Recommendation**: Always recommend one approach, and say what would change the recommendation.
- **Honest Unknowns**: List what could not be verified instead of guessing.
