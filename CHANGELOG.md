# Changelog

## [2026-10-23]

### Docs

- README: screenshots of tandem (overview, Status and Queries tabs, help) rendered from a fictional demo project; `docs/screenshots/` holds the generator.

### tandem

- Fixed: the status window's border title was tmux's default pane title (the machine's hostname). It now reads `STATUS`.
- Fixed: agent rows in the status window repeated the CLI (`agy·agy·gemini…`).
- Singular counts ("1 agent", "1 worktree", "1 open query").

## [2026-10-22]

### tandem

- Fixed: the help popup (and every other tandem window) began with `ERROR:root:code for hash blake2b was not found` and a traceback on Pythons built against a limited OpenSSL. tandem no longer imports `hashlib` (a test enforces it). The per-workspace tmux socket name is now derived differently, so a session started with the previous build is not reused; end it with `tmux -L <socket> kill-server` or just detach.

## [2026-10-21]

### tandem

- Fixed: starting tandem in a second project attached to the first project's session. Every workspace now gets its own tmux server (`tandem-<hash of the workspace path>`), so projects run side by side. The status and placeholder windows are pinned to their workspace.
- A session started by an older version still lives on the shared `tandem` socket and is not reused; tandem prints a note with the command to end it (`tmux -L tandem kill-server`).

## [2026-10-20]

### CLI Package

- `init` asks whether to create a blank local git repo when the folder isn't one (`--git` / `--no-git` skip the question). It never nests a repo inside another.
- `update` offers to commit only the files the update changed (`--commit` / `--no-commit`); other uncommitted work and `.bak` files are left alone. `--yes` alone never commits.
- New `.agent-workspace/.gitignore` keeps `tandem-session.json`, `coordination/` and backups out of git.
- spec-planner / spec-implement: when the agent workspace is itself a git repo, the delivery task also commits (and, with a remote, PRs and merges) the workspace's own changes.

## [2026-10-19]

### tandem

- Quota line under the window (second tmux status row). Claude: fed by Claude Code's own documented statusline data (`rate_limits` 5-hour and 7-day), via `--settings`, chaining to your existing statusline. Antigravity and anything else: `quota not available`. No pane reading, no token use, no usage endpoints.

## [2026-10-18]

### tandem

- **Save and resume:** `F10` → *Save state and exit* writes `.agent-workspace/tandem-session.json` (owner-only JSON: agents' CLI, role, model, tab number, folder, visible tabs, pane proportions) and ends the session. The next launch asks to resume or start blank; `--resume` / `--fresh` skip the question. Conversations are not restored (each CLI has its own resume). Missing CLIs are skipped and deleted folders fall back to the workspace.
- **Update without quitting:** `F10` → *Update tandem and workspace* (or `tandem update [--yes]`) saves state, runs `npx --yes hydrokat/agent-workspace update --yes --no-link`, syncs tandem's Python environment, and restarts only tandem's own windows and key bindings. Agents keep running. A failed step restarts nothing. `TANDEM_UPDATE_SPEC=owner/repo[#ref]` selects another source.
- The `F10` menu now offers save and exit, detach, update, and kill (no save).

## [2026-10-17]

### tandem

- Status window: new **Repositories** section (expanded by default) lists the workspace and every codebase in `codebase-symlinks/`, one header per repo with each branch below it: `* branch` for the main checkout, `branch → ../worktree` for a branch active in a linked worktree, then idle branches (capped at five). Rows shorten to the window width and re-render on resize. Replaces the collapsed Codebases/Worktrees/Branches sections, which only described the workspace repo.
- Fixed: branch names containing `/` were truncated in worktree listings; `codebase-map.md` inside `codebase-symlinks/` was shown as a missing codebase.
- Keyboard resize: `F12` enters resize mode (arrows resize the focused pane, `=` resets, `Enter`/`Esc` leaves); mouse border-drag still works.
- Responsive: the status window is clamped to 40% of a short terminal on resize; new `tandem reset-layout`.

## [2026-10-16]

### CLI Package

- Added `run.sh`: checks `git`, `tmux`, `uv`, `claude`, and `agy`, asks before every install or update, enforces the minimum versions in `tandem/min-versions.env`, then launches tandem. Supports `--check`, `--yes`, `--no`.
- Added **tandem** (`tandem/`, Python + uv): launches `claude` / `agy` side by side in tmux as planners (left) and implementers (right), with a Textual status window (Status and Queries tabs), per-column agent tabs, a launch wizard, and an F-key keymap. It only spawns the official CLIs; it never sends keys to or reads from their panes.
- `package.json` `files` now ships `run.sh` and the tandem sources. `init` and `update` now install `run.sh` (workspace root, executable) and the tandem project (`.agent-workspace/tandem/`), so `./run.sh` works in any workspace. A modified `run.sh` is backed up to `run.sh.bak`. They still do not require Python; only `run.sh` does.
- `run.sh` prefers `.agent-workspace/tandem/` and falls back to a sibling `tandem/` (package checkout).

### Base Skills

- Added `agent-coordination` (with stdlib `scripts/coord.py`): session registry, atomic task claims, heartbeats, and non-blocking agent questions stored under `.agent-workspace/coordination/`.
- Enhanced `spec-planner`: new "Check Active & Planned Work" step; task template gains `## Affected Paths` and `## Depends On`.
- Enhanced `spec-implement`: claims a task before working, uses a worktree when another implementer is active, heartbeats and checks answers at milestones, blocks only the affected objective, and releases the claim afterwards. Both skills fall back to `in-progress.md` when coordination is unavailable.

### Base Files

- `AGENTS.md` §8 gains the multi-agent coordination principle and the session-start `whoami` rule.
- `specs/in-progress.md` entries gain optional Session and Claim fields.
- Added best-practice notes: ToS-safe orchestration, non-blocking agent questions, and testing bash with a stubbed PATH.

## [2026-10-10]

### Base Skills

- Enhanced `code-review-remediation` into a full PR loop: pull open PRs/MRs (all relevant repositories unless one is named), review, comment, remediate, and repeat until all clear. It now bundles the code review rubric (`references/review-criteria.md`), runs CI locally first and posts evidence when remote CI fails but local passes, produces a merge report, asks before merging, and replies only `Merged` after a successful merge.

## [2026-10-09]

### Base Skills

- Enhanced `specs-planner` to create new phases as `p-###-[name-of-repo]`, use `p-###-multi` for multi-repo phases, create task files as `t-###.md`, and record involved repositories in `impl.md`.
- Updated `spec-implement` and `spec-compile` to read the new `p-###-*` phase directories and `t-###.md` task files while preserving read-only support for existing legacy specs.

### Base Files

- Updated workspace, specs, progress tracking, compiled-output, and package documentation to describe the new repo-scoped phase and task naming conventions.

## [2026-09-22]

### Base Skills

- Enhanced `spec-implement` to use the `caveman` skill whenever possible during spec execution and to document exceptions when terse mode is unavailable or inappropriate.
- Added strict `spec-implement` code-comment discipline: source comments are for durable documentation only, with implementation reasoning kept in task files, plans, commits, or knowledgebase entries.

### Base Files

- Updated package documentation to describe spec implementation's caveman-mode execution and code-comment cleanup expectations.

## [2026-09-02]

### Base Skills

- Enhanced `specs-planner` to require low-fidelity UI mockups for frontend/UI phases and tasks, including responsive variants, important states, interaction notes, and accessibility notes.

### Base Files

- Updated specs documentation to remind agents to include UI mockups before frontend/UI implementation starts.

## [2026-08-16]

### Base Skills

- Enhanced `specs-planner` to prompt for a working branch and merge target before planning, record the selected branch naming pattern for future phases, and always include a final delivery task covering commit, PR creation, code review/remediation loops, pipeline fix loops, and merge.

## [2026-07-16]

### Base Files

- Enhanced `specs/in-progress.md` tracking to be multi-agent aware: agents must explicitly record their role, ID, and working branch/worktree to prevent collisions when running concurrently.
- Updated `AGENTS.md` Progress Tracking rules and `specs/in-progress.md` template to enforce explicitly declaring the agent context.

## [2026-07-03]

### Base Files

- Added a Branch Protection operating principle to `AGENTS.md`: contributors must never commit or make changes directly on `main`, `master`, or `trunk` unless the user explicitly instructs it, and must use a worktree or a new branch before implementing.
- Added a Concurrent Agent Awareness operating principle to `AGENTS.md`: contributors must not assume exclusive access to the codebase — check for other agents' in-progress work via `git status`, task files, and branches/worktrees before starting, and coordinate instead of overwriting.
- Added a Progress Tracking rule to `AGENTS.md`'s Documentation Sync section and a new `specs/in-progress.md` starter file, scaffolded by `init`: agents must list every active task or spec (labeled `adhoc` if it has no spec) with its ongoing status and next step, and remove entries as soon as the task is complete.

### Base Skills

- Updated `spec-implement` to create or switch to a feature branch before implementing (never directly on `main`, `master`, or `trunk`), reusing an existing non-protected branch when already checked out on one.
- Added `workspace-cleanup` skill: scans the workspace for files and directories that may not belong (OS/editor cruft, stale `.bak` backups, orphaned generated provider output, empty directories, stale `.tmp/` scratch files, and other unrecognized files), lists every candidate with a reason, and asks the user which ones to delete before removing anything. Trigger with `/workspace-cleanup`.

## [2026-07-02]

### Base Skills

- Added `code-review` skill: production-grade PR/diff review covering correctness, business flow, architecture, security, performance, database safety, API contracts, tests, deployment risk, and blast radius, with a structured findings-first output and an approval/changes-requested verdict.
- Added `code-review-remediation` skill: takes a pull request from reviewer feedback to a validated remediation commit — gathers unresolved review threads, validates reviewer intent before changing code, implements scoped fixes, reconciles with the base branch, and reports outcomes.
- Added `pull-request` skill: turns completed implementation work into a PR — branch protection checks, atomic commits, validation, a structured PR description template, and a final review checklist.

### Base Files

- Added `.tmp/` as a scaffolded workspace directory for agent working files. It is created by `init`, safe to delete at any time, and documented in `AGENTS.md`'s Core Directory Map.

## [2026-06-04]

### CLI Package

- Introduced `agent-workspace` CLI (`npx hydrokat/agent-workspace`).
- Added `init` command: scaffolds directory structure, installs agents/skills/guidelines from bundled templates, generates provider outputs, optionally links sibling codebases, runs `doctor`, and writes `.agent-workspace/manifest.json`.
- Added `update` command: refreshes managed library files (with `.bak` backups for local edits), refreshes the `AGENTS.md` managed block, fully regenerates provider outputs, never touches project content (`specs/`, `knowledgebase/business-flows/`, user entries).
- Added `doctor` command: validates required dirs, per-provider agent files, frontmatter (`name`, `description`, `model`), model correctness against `models.json`, compat doc symlinks, and codebase symlink resolution. Exits non-zero on failure.
- Added provider adapters for Claude, Gemini, and Codex with per-adapter agent transforms and model injection.
- Added `src/data/models.json` encoding the agent × provider model map sourced from the `AGENTS.md` recommendations table.
- Added `templates/` as the bundled canonical payload (12 agents, 5 skills, knowledgebase starters).
- Ported `link_codebases.py` to JS (`src/core/link-codebases.js`); the package now requires only Node ≥ 18.
- Reduced `workspace-init` and `workspace-sync` skill files to thin wrappers that invoke the CLI.
- Added 22 smoke tests (`test/`) covering init, update, doctor, CLI flags, backup contract, and no-clobber guarantees.

## [2026-05-05]

### Base Files

- Added an Agent Model Recommendations section to `AGENTS.md` for cross-provider role selection guidance.
- Refreshed the Codex/OpenAI recommendations in `AGENTS.md` to use current, role-appropriate model IDs: `gpt-5.5`, `gpt-5.3-codex`, `gpt-5.4`, and `gpt-5.4-mini` instead of stale `o3`, `GPT-4.1`, and `codex-mini` references.

## [2026-04-24]

### Base Files

- Reframed the workspace around vendor-agnostic base sources of truth using `AGENTS.md`, `.agents/agents/`, and `.agents/skills/`.
- Updated `AGENTS.md` with research-first rules, skill-first execution, compatibility sync policy, and base-file precedence.
- Added `WORKFLOW.md` as the shared execution policy for development and orchestration behavior.
- Added `README.md` with setup, initialization, usage, sync, and contribution guidance for humans and agents.
- Neutralized workspace documentation so it describes a reusable project workspace rather than a Gemini-specific agent environment.
- Updated knowledgebase and specs documentation to align with the vendor-agnostic workspace model.

### Base Skills

- Added `workspace-init` to bootstrap a new workspace with starter directories and files.
- Added `workspace-sync` to regenerate Gemini, Claude, and Codex compatibility outputs from base files.
- Updated `workspace-sync` to sync both agents and skills from the base workspace.
- Hardened `workspace-sync` so it preserves base files and avoids overwriting existing non-generated compatibility documents.
- Renamed `knowledgebase-manager` to `knowledge`.
- Updated `knowledge` so it can be triggered with `/knowledge` and so it reviews and updates `guidelines`, `best-practices`, `business-flows`, and `context-history` as needed.
- Updated `knowledge` to require collaboration between `business-analyst` and `technical-writer` for meaningful knowledgebase maintenance.
- Added `find-skills` skill to find skills using the skills MP website.

### Base Agents

- Added a vendor-agnostic agent catalog under `.agents/agents/`.
- Added role definitions for `codebase_investigator`, `generalist`, `business-analyst`, `code-reviewer`, `frontend-dev`, `qa-engineer`, `security-auditor`, `senior-backend-dev`, `tech-lead-orchestrator`, `technical-writer`, and `terminal-agent`.
- Updated `tech-lead-orchestrator` to require `WORKFLOW.md`, explicit task-to-agent mapping, and mandatory pre-report and post-report outputs.
- Added `api-pentester`

## Changelog Rules

- Record only changes to base files, base skills, and base agents.
- Do not log regenerated `.gemini/` or `.claude/` outputs unless the underlying source behavior changed.
- Prefer concise, high-signal entries that describe what changed and why it matters.
