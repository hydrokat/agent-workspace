# Changelog

All notable changes to the base workspace are tracked here.

This changelog is for source-of-truth assets only:

- base files such as `AGENTS.md`, `WORKFLOW.md`, `README.md`, `knowledgebase/`, and `specs/`
- base skills under `.agents/skills/`
- base agents under `.agents/agents/`

Generated compatibility artifacts under `.gemini/` and `.claude/` should not be tracked here unless the source-of-truth behavior changes.

## [2026-05-01]

### Workspace Rebranding

- Rebranded the design system and application from **Kinetic Grid** to **Nexus** across all documentation and specification files.
- Renamed `knowledgebase/guidelines/kinetic-grid-ui.md` to `nexus-ui.md` and updated all internal references.
- Updated all design specifications and HTML previews in `knowledgebase/guidelines/design/` to use Nexus branding and semantic CSS classes.

### Phase 006 - Playwright E2E Test Suite

- Initialized Phase 006 implementation plan in `specs/phase-006/`.
- Created tasks (601-604) for Playwright setup with local Supabase lifecycle management, Auth/Teams testing, Task lifecycle testing, and Points/Rewards engine validation.

## [2026-04-30]

### Knowledgebase & Specs

- Added `knowledgebase/context-history/2026-04-30-ui-contrast-audit.md` with deep-dive findings on token mismatches, dead glow systems, and low-contrast primitives.
- Updated `knowledgebase/guidelines/ui-design-guideline.md` with canonical tokens (Background #131313, Header #050505, Surface Tiers) and prohibition of raw hex in JSX.
- Added `specs/phase-005/` implementation plan and atomic tasks (501-504) for UI/UX remediation.

### Implementation (Phase 004 - Rewards & Dashboards)

- **Point Aggregation**: Implemented `getUserTotalPoints` Server Action and created `user_points_summary` view in Supabase for performant merit tracking.
- **Rewards Backend**: Created `rewards.ts` actions for CRUD operations on rewards and processing member claims.
- **Member Dashboard**: Completed main member dashboard with points overview, progress milestones, task history, and merit ledger.
- **Rewards Marketplace**: Built `/rewards` page for members to browse available assets and submit acquisition claims.
- **Admin Reward Management**: Built `/admin/rewards` for curators to provision assets and authorize personnel claims.
- **E2E Validation**: Added `reward-flow.test.ts` integration test validating the eligibility and claim lifecycle.
- **Security Audit**: Verified RLS and Server Action authorization logic for point/reward integrity.

### Implementation (Phase 005)

- **Restored Hover Glow**: Defined `.glow-primary` and `.hover-glow-primary` in `globals.css`; fixed global `box-shadow` nuke; applied glow to `Button`, `Badge`, and `TableRow` primitives.
- **Corrected Design Tokens**: Updated `--background` to `#131313`; introduced 4-tier surface container system; replaced hardcoded `bg-[#050505]` and `bg-[#0a0a0a]` with semantic tokens across 15+ files.
- **Enhanced Contrast**: Boosted border visibility for `Input`, `Textarea`, `Select`, `Badge`, and `Button` (outline variant); standardized placeholder contrast.
- **Cleanup**: Eliminated orphaned `text` class across the codebase; replaced raw `<input>` with `<Input>` primitive in task board for consistency; removed `red-500` drift in favor of semantic `destructive` token with increased opacity.

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
