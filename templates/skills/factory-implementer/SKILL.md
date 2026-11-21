---
name: factory-implementer
description: "Interactive Factory Mode develop stage playbook. Reads prompt.md and slices.json, implements changes inside slice paths without weakening create-tests files, follows spec-implement test-first and comment rules, never commits, writes result.json, then stops."
---

# Factory Implementer

## Mandate

- Read `$TANDEM_JOB_DIR/prompt.md` first when starting in your interactive worker pane. Once `$TANDEM_JOB_DIR/result.json` is written, finish your turn and stop cleanly so the runner can advance.
- Follow the test-first and code-comment discipline rules in `.agents/skills/spec-implement/SKILL.md` (Steps 5 and 7) by reference.
- Work strictly inside the paths declared in `$TANDEM_FACTORY_RUN_DIR/artifacts/slices.json`. Never modify protected paths (`.agent-workspace/`, `workflows/`, `.github/workflows/`, `.git/hooks/`, or check-config files).
- **Preserve `create-tests` files.** Never delete test files created in `create-tests`, and do not modify them unless strictly required (if modified, provide a clear `"rationale"` in `result.json`).
- **Do not commit** (`git add` / `git commit`) and never push. Tandem verifies the stage contract and commits in-scope changes automatically.

## Procedure

1. **Read the slices and plan context.** Read `$TANDEM_FACTORY_RUN_DIR/artifacts/slices.json` and `plan-snapshot.md` from the stage context, plus any failure summary if retrying.
2. **Implement within slice paths.** Write clean, minimal implementation code inside the declared slice `paths`.
   - Run relevant test commands through `python3 .agents/skills/test-failures/scripts/run_tests.py -- <test command>` to verify behaviour. Optionally start one `python3 .agents/skills/test-failures/scripts/watch.py start` watcher while iterating (`wait`, then `results`), run the final verification through `run_tests.py`, and `watch.py stop --all` before writing `result.json`.
3. **Write `result.json`.** Write `$TANDEM_JOB_DIR/result.json` with:
   - `"status": "succeeded"` (or `"failed"` with `"rationale"` if blocked)
   - `"summary"` describing the implementation
   - `"files_changed"` listing the modified/created files
   - `"rationale"` (required if any `create-tests` file was modified, or on failure)
   - `"hypothesis"` (required whenever `TANDEM_ATTEMPT > 1` on a retry)
