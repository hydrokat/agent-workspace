---
name: factory-reviewer
description: "Interactive Factory Mode review-stage playbook. Reads prompt.md, reviews the run branch diff against the base commit read-only using the code-review checklist, writes findings.json and result.json, then stops."
---

# Factory Reviewer

## Read-Only Mandate

The `review` stage runs in an interactive worker pane with only `repo_read` and `diff_review` tools. Read `$TANDEM_JOB_DIR/prompt.md` first, never modify, create, or delete files inside the target repository or worktree, and never commit or push. Any edit to the worktree fails the stage. Write outputs only under `$TANDEM_FACTORY_RUN_DIR/artifacts` and `$TANDEM_JOB_DIR/result.json`, then finish your turn and stop cleanly.

## Procedure

1. **Inspect the fresh review context.** Read the plan snapshot, acceptance criteria, changed files list, diff against the base commit, and test/lint evidence provided in the stage context. Judge the code and evidence directly, not any implementer claim.
2. **Apply the `code-review` checklist.** Follow `.agents/skills/code-review/SKILL.md` to review correctness, edge cases, architecture, security, performance, error handling, and tests across the run branch diff against `base_commit`.
3. **Write `$TANDEM_FACTORY_RUN_DIR/artifacts/findings.json`.** Emit a JSON object with:
   ```json
   {
     "verdict": "approve",
     "findings": [
       {
         "id": "f-001",
         "severity": "blocker",
         "file": "src/example.rs",
         "line": 42,
         "title": "Short finding title",
         "detail": "Concrete explanation of the issue and why it matters.",
         "suggestion": "Specific actionable remediation."
       }
     ]
   }
   ```
   Rules:
   - `severity` must be one of `blocker`, `major`, `minor`, `nit`.
   - `verdict` must be `approve` or `changes_requested` (tandem will independently derive `changes_requested` if any `blocker` or `major` finding is present).
   - Every `file` must be a relative path inside the diff or the target repository.
4. **Write `result.json`.** Write the job's `result.json` with `status: "succeeded"`, `files_changed: []`, a concise `summary`, and `artifacts: ["findings.json"]`.
