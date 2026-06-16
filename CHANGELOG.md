# Changelog

All notable changes to the base workspace are tracked here.

This changelog is for source-of-truth assets only:

- base files such as `AGENTS.md`, `WORKFLOW.md`, `README.md`, `knowledgebase/`, and `specs/`
- base skills under `.agents/skills/`
- base agents under `.agents/agents/`

Generated compatibility artifacts under `.gemini/` and `.claude/` should not be tracked here unless the source-of-truth behavior changes.

## [2026-06-08]

### Phase 014 - Clean Code & Architecture Remediation (Tasks 006–008)

- **Auth Guards Module**: Extracted `getAuthenticatedUser`, `isAdmin`, and `isTeamLead` from `lib/actions/teams.ts` into `lib/auth/guards.ts`. Re-exports from `teams.ts` preserve backward compatibility for all existing consumers.
- **Domain Layer**: Created `lib/domain/` with four pure TypeScript modules (no Supabase/Zod/Next.js imports): `ranking.ts` (dense-rank `assignRanks()`), `tier.ts` (`getTierInfo()` + `TIER_THRESHOLDS`), `participation.ts` (capacity resolution), and `membership.ts` (approval-activates-profile invariant).
- **Leaderboard Refactor**: `lib/actions/leaderboard.ts` now uses `assignRanks()` from the domain layer, removing duplicate inline ranking loops.
- **Middleware Auth Split**: Auth routing logic extracted from `lib/supabase/middleware.ts` into a pure `resolveAuthRoute()` function in `lib/auth/routing.ts`. Middleware now calls the pure function and acts on its result — all five redirect cases preserved (unauthenticated, onboarding, reset, pending, authed-on-login).
- **Unit Tests**: Added 36 new tests across `lib/domain/__tests__/ranking.test.ts`, `lib/domain/__tests__/tier.test.ts`, and `lib/auth/__tests__/routing.test.ts`. Full suite: 303 tests passing.
- **Security Audit**: Completed OWASP 2025 baseline audit; all critical/high findings from Tasks 001–008 are fixed or accepted with rationale. Documented in `task-008.md`.
- **Knowledgebase**: Added `best-practices/error-normalization.md` (actionError pattern) and `best-practices/domain-layer.md` (layering convention, module list, testing rules).

## [2026-06-07]

### Phase 013 - System State Reset

- **Database Migrations**: Added migration `20260607141000_create_system_reset_tables_and_rpcs.sql` introducing `system_reset_requests` and `system_reset_approvals` tables, row-level security (RLS) constraints, and `SECURITY DEFINER` functions to coordinate multi-admin consensus reset workflows.
- **Server Actions**: Created `lib/actions/admin-reset.ts` to authorize action triggers, request approvals, and initiate atomic system-state wipes. Revalidates relevant cache pathways upon completion.
- **Danger Zone Page**: Added the `/admin/danger-zone` Route and the corresponding client component `<DangerZoneClient>` with structured lists of wiped and preserved resources, a confirmation modal requiring typed confirmation (`RESET`), and consensus meters.
- **Best Practices Documentation**: Published `knowledgebase/best-practices/admin-reset.md` covering the multi-admin approval requirements, system boundaries, and database row-trigger override mechanics.
- **Testing & Audit**: Added full test coverage in `admin-reset.test.ts` and `danger-zone-client.test.tsx` verifying route authorization, consensus gating, and state validation.

### Phase 012 - Leaderboards

- **Core UI & Nav**: Added dedicated team and individual leaderboards with custom highlights for rank positions 1 to 10, customizable pagination size selectors, and dynamic off-page rank indicators (rendering the user's current rank as a floating 11th item).
- **Profile Configuration**: Added a name-editing form (`app/account/update-profile-form.tsx`) to let users change their display names.
- **Data Integration**: Created server action queries to fetch and aggregate leaderboard ranks based on transactional point history.
- **Testing**: Added unit test coverage for `leaderboard-client.test.tsx`, `update-profile-form.test.tsx`, and `profiles.test.ts`.

## [2026-06-05]

### Phase 011 - Admin Invitation & Onboarding

- **Database Migration**: Added a new database migration (`20260605125000_add_must_complete_onboarding_to_profiles.sql`) introducing the `must_complete_onboarding` boolean column on the `profiles` table to safely gate invited admins.
- **Server Actions**: Created `lib/actions/admins.ts` containing the `inviteAdmin`, `listAdmins`, and `completeOnboarding` server-only actions. These handle administrative invites via Supabase's native API, list admins, and process onboarding name and password registration.
- **Admin Management Page**: Built `/admin/admins/page.tsx` displaying the current administrator directory and setup status, featuring a modal-based administrator invitation trigger.
- **Invite Admin Dialog**: Built `<InviteAdminDialog>` in `components/admin/invite-admin-dialog.tsx` for sending email invitations, with local fallback link visibility for dev environments.
- **Onboarding Page**: Built the dedicated `/welcome` onboarding surface (`app/welcome/page.tsx` and `<WelcomeForm>` component) to guide new admins through setting their name and password on first login.
- **Middleware Gating**: Extended `lib/supabase/middleware.ts` to redirect authenticated users with `must_complete_onboarding = true` to `/welcome` across the entire application.
- **Unit Testing & Security Audit**: Added comprehensive unit tests in `lib/actions/__tests__/admins.test.ts` (11 new tests) achieving 100% green coverage. Documented a feature security audit under Task 006.

## [2026-06-01]

### Phase 010 - UI Enhancements & Internationalization

- **i18n Infrastructure**: Implemented a fully type-safe cookie-based internationalization model (`NEXT_LOCALE`) supporting `"en"` (tactical) and `"en-simple"` (plain language) vocabularies. Derived and lazy-loaded server-side dictionaries.
- **Language Switcher**: Built a client-side `<LanguageSwitcher>` component using shadcn UI elements, integrated into the navigation sidebar.
- **Dynamic Localization**: Externalized and localized landing page (`app/page.tsx`), login page (`app/login/page.tsx`), and main member dashboard (`app/dashboard/page.tsx`) with zero remaining hardcoded strings.
- **Responsive Layout Remediation**: Added `overflow-x-auto` wrappers to tabular dashboard data to ensure layout integrity and prevent horizontal scroll. Introduced name/email text-truncation inside sidebar block to avoid overflow at mobile (375px) breakpoints.
- **Unit Testing**: Developed a comprehensive Jest test suite checking dictionary parity/completeness, `getLocale` cookie/header resolution, and `LanguageSwitcher` components. Adapted pre-existing tests to support the i18n prop signatures, with all 21 test suites (219 tests) passing successfully.
- **Knowledgebase Update**: Added `knowledgebase/guidelines/i18n.md` and updated `technical-stack.md` and `ui-design-guideline.md` to document the responsive layouts and i18n standards.

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
