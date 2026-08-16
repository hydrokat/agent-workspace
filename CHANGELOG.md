# Changelog

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
