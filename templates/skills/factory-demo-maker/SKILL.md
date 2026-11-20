---
name: factory-demo-maker
description: "Interactive Factory Mode demo-stage playbook. Reads prompt.md, starts the app locally as planned, captures local PNG screenshots for each UI acceptance criterion, writes demo.json and result.json, then stops."
---

# Factory Demo Maker

## Local-Only Mandate

The `demo` stage runs in an interactive worker pane with only `browser` and `screenshot` tools. Read `$TANDEM_JOB_DIR/prompt.md` first, never modify, create, or delete files inside the target repository or worktree, and never commit or push. All screenshots stay on this machine in `$TANDEM_FACTORY_RUN_DIR/artifacts` and are never uploaded. Finish your turn and stop once `result.json` is written.

## Procedure

1. **Read the plan and UI acceptance criteria.** Inspect `$TANDEM_FACTORY_RUN_DIR/artifacts/slices.json` and the plan snapshot to identify every UI slice and acceptance criterion, plus how to start the local app.
2. **Capture local PNG screenshots.** Start the app locally on `127.0.0.1` or `localhost` and capture at least one PNG screenshot per UI acceptance criterion (at most 20 screenshots per run, each at most 5 MiB) into `$TANDEM_FACTORY_RUN_DIR/artifacts/demo/`.
3. **Write `$TANDEM_FACTORY_RUN_DIR/artifacts/demo.json`.** Emit a JSON object with:
   ```json
   {
     "screenshots": [
       {
         "path": "demo/shot-01.png",
         "caption": "Short description of what the screenshot shows.",
         "criterion": "UI acceptance criterion covered"
       }
     ]
   }
   ```
   Rules:
   - Every `path` must be a relative path under `$TANDEM_FACTORY_RUN_DIR/artifacts` pointing to a valid PNG file (<= 5 MiB).
   - Every screenshot must have a non-empty `caption`.
   - Total screenshots must be >= the number of UI acceptance criteria and <= 20.
4. **Write `result.json`.** Write the job's `result.json` with `status: "succeeded"`, `files_changed: []`, a concise `summary`, and `artifacts` listing `demo.json` and all captured PNG paths.
