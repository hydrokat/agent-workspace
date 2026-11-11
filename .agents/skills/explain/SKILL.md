---
name: explain
description: Use when the user asks how a module, feature, flow, or part of the codebase works, asks "explain X", "how does X work", "walk me through X", or invokes /explain. Finds the relevant code deterministically with locate.py, reads it, and explains it technically with file:line references and a diagram by default, or in plain language when the user asks for a non-technical, plain, or simple explanation (--non-technical). Read-only.
---

# Explain

## Overview

Use this skill to explain how part of a codebase works. Discovery is deterministic (`scripts/locate.py`). Reading the code and writing the explanation is the judgment work. It never edits files unless the user asks to save the explanation.

## Step 0: Parse The Request

| Invocation | Mode |
|---|---|
| `/explain <module or feature>` | Technical |
| `/explain <module or feature> --non-technical` | Plain language |
| `/explain how does <thing> work` | Technical |
| Request mentions "non-technical", "plain", "simple", or "for a non-developer" | Plain language |

The subject is a module name, feature, file, or question. Pick one to eight search terms from it (identifiers first, then nouns). With no subject, ask what to explain.

## Step 1: Locate The Code

Run:

```bash
python3 .agents/skills/explain/scripts/locate.py find <term> [<term> …] [--limit N] [--offset N]
```

It searches tracked files in the current repo and in each repo under `codebase-symlinks/`, labelled from `knowledgebase/context-history/codebase-map.md`. It skips binary, vendored, generated, oversized, and secret files (such as `.env`), and symlinks that leave their repo.

Result fields:
- `files`: ranked matches with matched lines. `total` and `next_offset` allow paging.
- `tests`: test files that match. `entry_points`: likely starting points. `references`: files that import the top matches.
- `areas`: where the matches cluster. `suggestions`: close terms when nothing matched.

Read `knowledgebase/context-history/codebase-map.md` first when `codebase-symlinks/` exists, and `AGENTS.md` or `README.md` for orientation.

Handle the result:
- **No match** (`total` is 0): offer the `suggestions`, or ask for another term. Do not invent an explanation.
- **Ambiguous** (the top `files` spread across unrelated `areas`): list the top two or three candidates with one line each and ask which one to explain before continuing.
- **Several repos**: say which repo each part lives in, using the `repos` labels.
- **More results** (`next_offset` is set) only matter when the first page does not cover the subject. Page with `--offset`.

If `locate.py` is missing or fails, say so and find the code manually (`git ls-files`, then search for the terms).

## Step 2: Read The Code

Read the files `locate.py` points to: the top `files`, the `entry_points`, the `references`, and the `tests`. Follow calls and imports until the flow is understood. Never explain code that was not read. Never print secrets or credentials found while reading.

## Step 3: Write The Explanation

Fill the template for the mode and drop sections that do not apply:
- Technical: `assets/technical.md`. Every claim about behavior carries a `path:line` reference. The diagram shows components and data flow.
- Plain language: `assets/non-technical.md`. No code, no file paths, no jargon or unexplained acronyms. Short paragraphs. Use one everyday analogy.

Say plainly what is uncertain or could not be found. Offer the next step: a deeper dive on one part, the other mode, `/feasibility` for a change, or `/spec-plan`.

## Step 4: Save (Only On Request)

Do not write files. If the user asks to save the explanation, ask where, then write it there.

## Mandates

- **Trigger**: Easy to invoke with `/explain`.
- **Read-Only**: Never modify the codebase. Write a file only when the user asks to save.
- **Script-First**: Find code with `locate.py`, not by free searching, unless the script is unavailable.
- **Grounded**: Technical claims need `file:line` references from code that was read.
- **Honest Gaps**: State what was not found, not covered by tests, or uncertain.
- **No Secrets**: Never quote `.env` values, keys, tokens, or credentials.
- **Ask When Ambiguous**: Ask which match to explain instead of guessing.
