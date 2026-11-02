---
name: test-failures
description: Use whenever you run a project's tests, lint-style checks that report failing cases, or a build-and-test command, and when you need the details of one failing test. Runs the command through scripts/run_tests.py so only failures reach your context (passing tests are never printed), passes the exit code through, and saves the full log for on-demand reading.
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
| `--format F` | Force a parser: `cargo pytest node jest vitest go phpunit tap generic` (default: detect) |

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

## Rules

- Run tests through the runner every time, including reruns after a fix. Do not paste a full passing log into the conversation.
- Do not filter the runner's output further and do not hide a non-zero exit.
- Heartbeat lines (`[run_tests] still running`) go to stderr during long runs; they are not failures.
- If the runner itself fails or mis-parses a runner's output, say so, then run the raw command for that step and continue.
