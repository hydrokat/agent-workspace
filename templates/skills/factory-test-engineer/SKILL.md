---
name: factory-test-engineer
description: "Headless Factory Mode create-tests stage playbook. Reads slices.json, writes failing tests inside slice paths following spec-implement test-first and comment rules, never commits, and writes result.json."
---

# Factory Test Engineer

## Mandate

- Follow the test-first and code-comment discipline rules in `.agents/skills/spec-implement/SKILL.md` (Steps 5 and 7) by reference.
- Work strictly inside the paths declared in `$TANDEM_FACTORY_RUN_DIR/artifacts/slices.json`. Never modify protected paths (`.agent-workspace/`, `workflows/`, `.github/workflows/`, `.git/hooks/`, or check-config files).
- **Do not commit** (`git add` / `git commit`) and never push. Tandem verifies the stage contract and commits in-scope changes automatically.

## Procedure

1. **Read the slices and plan context.** Read `$TANDEM_FACTORY_RUN_DIR/artifacts/slices.json` and `plan-snapshot.md` from the stage context.
2. **Write failing tests within slice paths.** Add or update test files inside the slice `paths` that verify the acceptance criteria.
   - Run test commands through `python3 .agents/skills/test-failures/scripts/run_tests.py -- <test command>` and confirm the new tests **fail** against the unimplemented code.
   - If the change is a pure refactor with no behaviour change, set `"no_behaviour_change": true` and provide a clear `"rationale"` in `result.json`.
3. **Write `result.json`.** Write `$TANDEM_JOB_DIR/result.json` with:
   - `"status": "succeeded"` (or `"failed"` with `"rationale"` if blocked)
   - `"summary"` describing the tests written
   - `"files_changed"` listing the modified/created test files
   - `"no_behaviour_change"` (`false` unless pure refactor with `"rationale"`)
   - `"hypothesis"` (required whenever `TANDEM_ATTEMPT > 1` on a retry)
