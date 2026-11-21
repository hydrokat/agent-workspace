---
name: test-failures
description: Use whenever you run a project's tests, lint-style checks that report failing cases, or a build-and-test command, and when you need the details of one failing test. Runs the command through scripts/run_tests.py so only failures reach your context (passing tests are never printed), passes the exit code through, and saves the full log for on-demand reading. For fast feedback while developing, scripts/watch.py keeps one background test watcher (bacon, node, bun, vitest, jest, phpunit-watcher, ptw) and prints only the latest run's failures.
---

# Test Failures

## Overview

Test output is mostly noise: hundreds of passing names around the one failure that matters. `scripts/run_tests.py` runs the command for you, parses the runner's output, and prints only what failed. The command's own exit code is passed through, so scripts and gates behave as before. The full output is always saved, so nothing is lost.

## Step 1: Run Tests Through The Runner

```bash
python3 .agents/skills/test-failures/scripts/run_tests.py -- <test command>
```

Examples: `-- cargo test`, `-- npm test`, `-- uv run pytest -q`, `-- go test ./...`, `-- vendor/bin/phpunit`. Options:

| Option | Meaning |
|---|---|
| `--cwd DIR` | Run the command in `DIR` (default: current folder) |
| `--timeout SEC` | Kill the run after `SEC` seconds and exit 124 (default 1800) |
| `--max-lines N` | Cap the printed output (default 120) |
| `--format F` | Force a parser: `cargo pytest node jest vitest bun go phpunit tap generic` (default: detect) |

With no command, it asks `explain/scripts/locate.py stack` for the project's test command and says which one it picked. A command with no obvious runner still works: the generic parser reports lines with `FAIL`, `ERROR`, `panicked`, or `AssertionError`.

## Step 2: Read The Result

| First line | Meaning | What to do |
|---|---|---|
| `PASS: 412 passed, 3 skipped in 12.4s (exit 0)` | All good | Continue. Nothing else is printed. |
| `FAIL: 3 failed, 409 passed (exit 1)` | Tests failed | Read each failing test's block, fix, rerun. |
| `ERROR: ...` plus the last lines | The command failed but no failing test was recognised: compile error, crash, missing runner, timeout (exit 124) | Read the tail. It is the real error. |

Never assume a quiet run passed: trust the first line and the exit code.

## Step 3: Dig Deeper Only When Needed

- A block is cut off (`… N more lines`): `python3 .agents/skills/test-failures/scripts/run_tests.py show "<test name>"` prints that test's full block from the latest log.
- You need everything: open the log path printed on the last line (`.agent-workspace/test-runs/<time>-<command>.log`, owner-only, ANSI codes removed, newest 20 kept, gitignored).
- `show --log <path>` reads an older log. Paths must be inside the workspace.

## Watch Mode (Development Feedback)

During development, keep one watcher running instead of re-launching the suite after every edit (guideline: `knowledgebase/guidelines/watch-mode-testing.md`). `run_tests.py` stays the authority at gates (done, commit, CI).

```bash
python3 .agents/skills/test-failures/scripts/watch.py start            # once per suite; picks the runner from the project files
python3 .agents/skills/test-failures/scripts/watch.py wait             # after an edit: blocks until the re-run finishes
python3 .agents/skills/test-failures/scripts/watch.py results          # failures of the latest run only (exit 0 pass, 1 fail)
python3 .agents/skills/test-failures/scripts/watch.py stop --all       # when the task ends
```

| Command | Meaning |
|---|---|
| `start [--cwd DIR] [--runner R] [-- <cmd>]` | Start one background watcher per target. A second `start` for the same target is refused (exit 3). `--runner`: `bacon bun node vitest jest phpunit-watcher ptw`; or pass your own watcher command after `--` |
| `wait [--timeout SEC] [--quiet SEC] [--grace SEC]` | Block until the re-run for the latest edit has finished. Returns at once if nothing changed, and after `--grace` seconds if an edit triggered no re-run. Exit 124 on timeout |
| `results [--max-lines N]` | Same `PASS` / `FAIL` / `ERROR` format as `run_tests.py`, for the latest run only |
| `status [--all]`, `stop [--all]` | Inspect or end watchers. Output and state live in `.agent-workspace/test-watch/<target>/` (owner-only, gitignored) |

Detection: `Cargo.toml` → `bacon --headless -j test`, `bun test` → `bun test --watch`, `node --test` → `node --test --watch`, vitest, jest, `composer.json` → `vendor/bin/phpunit-watcher`, pytest projects → `ptw`. If the watcher binary is missing, `start` prints the install command and runs the suite once through `run_tests.py` instead (its exit code is passed through).

Rules:
- Read results with `wait` then `results`. Never attach to the watcher's terminal and never re-invoke the runner by hand after an edit.
- `results` reports the latest run only. If it says `ERROR` or `WAITING`, read the tail it prints or run `wait` again.
- Run `run_tests.py` for the full suite before declaring a task done, before committing, and in CI. A watcher may skip tests it was not told about.
- Stop watchers when finished (`watch.py stop --all`); do not leave them running between tasks.
- Bacon's `--headless` stdout is parsed with the cargo parser. Its `.bacon-locations` export is not used by `watch.py`.

## Rules

- Run tests through the runner every time, including reruns after a fix. Do not paste a full passing log into the conversation.
- Do not filter the runner's output further and do not hide a non-zero exit.
- Heartbeat lines (`[run_tests] still running`) go to stderr during long runs; they are not failures.
- If the runner itself fails or mis-parses a runner's output, say so, then run the raw command for that step and continue.
