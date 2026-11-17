---
name: factory-planner
description: "Headless Factory Mode plan-stage playbook. Turns an approved plan snapshot into specs via specs.py and writes artifacts/slices.json and result.json without modifying the repository."
---

# Factory Planner

## Read-Only Mandate

The `plan` stage has only `repo_read` and `issue_context` tools. Never modify, create, or delete files inside the target repository or worktree. Write outputs only under `$TANDEM_FACTORY_RUN_DIR/artifacts` and the job's `result.json`.

## Procedure

1. **Read the plan snapshot.** Read `plan-snapshot.md` from the stage context and list every item under `## Acceptance criteria`.
2. **Inspect the target repository (read-only).** Explore the codebase structure, existing patterns, and affected paths the same way `/spec-planner` inspects a repository, without asking the user or editing files.
3. **Write specs with `specs.py`.** Reuse `.agents/skills/spec-planner/scripts/specs.py` (never copy it) pointed at the run's artifact directory:
   - Ensure the artifact specs directory exists: `mkdir -p "$TANDEM_FACTORY_RUN_DIR/artifacts/specs"`.
   - Derive `--repo` or `--multi`, `--title`, `--branch`, and `--target` from the plan snapshot and run record; never prompt interactively.
   - Create the phase:
     ```bash
     python3 .agents/skills/spec-planner/scripts/specs.py --root "$TANDEM_FACTORY_RUN_DIR/artifacts" new-phase --repo <repo> --title "<title>" --branch <branch> --target <target>
     ```
   - Add one task per work slice:
     ```bash
     python3 .agents/skills/spec-planner/scripts/specs.py --root "$TANDEM_FACTORY_RUN_DIR/artifacts" new-task <phase> --title "<title>" --type "<New Feature|Enhancement|Bug Fix>" --paths <path...> [--depends <t-###...>]
     ```
   - Validate the generated phase:
     ```bash
     python3 .agents/skills/spec-planner/scripts/specs.py --root "$TANDEM_FACTORY_RUN_DIR/artifacts" lint <phase>
     ```
4. **Write `artifacts/slices.json`.** Write `$TANDEM_FACTORY_RUN_DIR/artifacts/slices.json` with one entry per task:
   ```json
   {
     "phase": "<phase-dir-name>",
     "slices": [
       {
         "id": "t-001",
         "title": "<task title>",
         "type": "New Feature",
         "paths": ["src/example.rs"],
         "dependencies": [],
         "acceptance_criteria": ["<exact criterion from plan-snapshot.md>"],
         "ui": false
       }
     ]
   }
   ```
   Ensure:
   - Every acceptance criterion in `plan-snapshot.md` appears in `acceptance_criteria` of at least one slice.
   - Every path in `paths` is relative to and inside the target repository (no `..` or absolute paths).
   - `ui` is `true` if the slice includes UI/frontend changes, otherwise `false`.
5. **Write `result.json`.** Write the job's `result.json` with `status: "succeeded"`, `files_changed: []`, a concise `summary`, and `artifacts` listing every generated spec file (`specs/<phase>/impl.md`, `specs/<phase>/t-001.md`, ...) plus `slices.json`.
