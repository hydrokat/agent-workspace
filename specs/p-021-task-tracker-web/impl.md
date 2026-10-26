# Implementation Plan: Enforce user_points_summary View RLS and Fix Prod/Staging Password Recovery

## Branch Strategy
- **Working Branch**: fix/p-021-rls-and-password-recovery
- **Merge Target**: trunk
- **Observed Branch Pattern**: <type>/p-###-<short-description>
- **Branch Pattern Source**: knowledgebase/guidelines/branch-naming.md and PRs #23-#26 in task-tracker-web

## Repository Scope
- **Phase Directory**: `specs/p-021-task-tracker-web`
- **Scope Type**: Single Repo
- **Primary Repository**: task-tracker-web
- **Involved Repositories**:
  - `task-tracker-web`: Primary application repository (`codebase-symlinks/task-tracker-web`), owning Supabase SQL migrations (`supabase/migrations/`), Docker production entrypoint (`docker/prod-entrypoint.sh`), Next.js Server Actions and Auth callback routes (`app-src/lib/actions/`, `app-src/app/auth/`), TypeScript schema definitions (`app-src/lib/supabase/database.types.ts`), and Jest/integration test suites.
  - `agent-workspace`: Planning and knowledgebase repository (`specs/p-021-task-tracker-web/`, `knowledgebase/best-practices/`, `knowledgebase/guidelines/branch-naming.md`, `AGENTS.md`), delivered alongside `task-tracker-web` in `t-004`.

## Phase Objectives
1. **Enforce caller Row Level Security (RLS) on `public.user_points_summary` (`t-001`)**: Add an append-only, atomic Supabase migration setting `ALTER VIEW public.user_points_summary SET (security_invoker = true)` and revoking `anon` / `PUBLIC` privileges plus non-`SELECT` table privileges from `authenticated`. Ensure querying `public.user_points_summary` evaluates `point_transactions` RLS policies (`auth.uid() = user_id` for members, `is_admin()` for admins, denied for `anon`) and resolve the Supabase Security Advisor `security_definer_view` finding.
2. **Eliminate `.env` shadowing in the `prod` Docker container and centralize public site-URL resolution (`t-002`)**: Fix `docker/prod-entrypoint.sh` so placeholder `NEXT_PUBLIC_SUPABASE_URL` / `NEXT_PUBLIC_SUPABASE_ANON_KEY` defaults are exported **only** when neither process environment variables nor `app-src/.env*` files define them—preventing `prod` in staging/production (`staging.yml` writes `app-src/.env`) from overriding real Supabase URLs/keys with `http://localhost:54321`. Introduce a typed, host-injection-safe `app-src/lib/config/site-url.ts` module (`resolvePublicOrigin`, `sanitizeNextPath`) per `knowledgebase/guidelines/environment-and-config.md`.
3. **Fix Forgot Password email dispatch and end-to-end recovery callbacks in staging and production (`t-003`)**:
   - Normalize email casing/whitespace in `requestPasswordReset()` (`email.trim().toLowerCase()`) so uppercase-styled inputs or trailing spaces never fail lookup.
   - Replace fragile `http`-defaulting, raw `x-forwarded-*` string concatenation in `requestPasswordReset()`, `inviteAdmin()`, `/auth/callback`, and `/auth/confirm` with `resolvePublicOrigin()` (`NEXT_PUBLIC_SITE_URL` / `SITE_URL` override → comma-split `x-forwarded-host` / `origin` / `host` + forced `https` on non-loopback production hosts), preventing `http://` downgrade or malformed `"https, http://..."` `redirectTo` URLs that Supabase GoTrue rejects.
   - Fix two callback redirect bugs in staging/prod: `/auth/confirm` appending raw `&recovery=true` onto queryless paths (producing 404 `/reset-password&recovery=true`) and `/auth/callback` redirecting error fallbacks to raw container `${origin}` (`http://0.0.0.0:3000/auth/auth-code-error`) instead of the resolved public HTTPS origin. Support both PKCE (`code`) and OTP (`token_hash` + `type`) recovery params across `/auth/callback` and `/auth/confirm`.
   - Surface actionable rate-limit allowlist messages from `requestPasswordReset()` while recording non-PII diagnostic metadata (`redirectOrigin`, `errorCode`, `errorStatus`) in Sentry, and document required Supabase Staging/Prod Auth URL allowlists (`https://<domain>/auth/**`) and Custom SMTP settings.
4. **Prevent regressions through TDD and security verification**: Write failing unit, contract, and local Supabase RLS integration tests before implementation, pass full Jest/lint/typecheck/build gates, and pass security review before delivery (`t-004`).

## Architecture

### 1. Database View Security Architecture (`t-001`)
PostgreSQL views default to `security_invoker = false`, causing queries on `public.user_points_summary` (`20260430000000_user_points_summary_view.sql`) to execute as view owner `postgres` (`BYPASSRLS`). Combined with `ALTER DEFAULT PRIVILEGES ... GRANT ALL ON TABLES TO anon, authenticated, service_role` in `20260426190308_fix_permissions_and_schema.sql`, both `anon` and any `authenticated` user could enumerate `(user_id, total_points)` for every user via PostgREST.

A new atomic migration (`20261026000000_enforce_rls_user_points_summary_view.sql`):
- Sets `ALTER VIEW public.user_points_summary SET (security_invoker = true);` (PostgreSQL 15+ native invoker semantics).
- Executes `REVOKE ALL ON public.user_points_summary FROM PUBLIC, anon;` and `REVOKE INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER ON public.user_points_summary FROM authenticated;`, keeping only `GRANT SELECT ON public.user_points_summary TO authenticated, service_role;`.
- Preserves existing `point_transactions` RLS policies (`Users can view own point transactions` using `auth.uid() = user_id` and `Admins can select point transactions` using `is_admin()`), which now filter rows **before** the view's `GROUP BY user_id` aggregation.
- Adds `user_points_summary` under `public.Views` in `app-src/lib/supabase/database.types.ts` so TypeScript callers and integration tests are strictly typed.

### 2. Container `.env` Precedence & Public Origin Resolution (`t-002` → `t-003`)
In `staging.yml`, secrets are written to `app-src/.env` on the remote host before `docker compose up prod -d`. Because Next.js (`@next/env`) never overwrites shell environment variables already present in `process.env`, unconditionally exporting `NEXT_PUBLIC_SUPABASE_URL` and `NEXT_PUBLIC_SUPABASE_ANON_KEY` defaults in `docker/prod-entrypoint.sh` shadowed `app-src/.env` with `http://localhost:54321` during both `npm run build` and `npm start`.

- `docker/prod-entrypoint.sh` will inspect `.env.production.local`, `.env.local`, `.env.production`, and `.env` for non-empty `NEXT_PUBLIC_SUPABASE_URL` / `NEXT_PUBLIC_SUPABASE_ANON_KEY` definitions and only export local-build placeholders when a variable is absent from both `process.env` and all `.env*` files.
- `app-src/lib/config/site-url.ts` centralizes environment/header origin resolution (`resolvePublicOrigin`) in compliance with `environment-and-config.md`:
  1. Uses configured `NEXT_PUBLIC_SITE_URL` (or `SITE_URL`) first when valid (`https://` or `http://`, normalized without trailing slash), eliminating Host-header poisoning and reverse-proxy header drift in staging/prod.
  2. Falls back to the first comma-separated host in `x-forwarded-host`, validated browser `origin` header, or `host`, rejecting malformed/injected hostnames (slashes, `@`, whitespace, control chars).
  3. Resolves protocol from the first comma-separated value in `x-forwarded-proto`, forcing `https` in `NODE_ENV === 'production'` for all non-loopback hosts even when a local Nginx proxy omits `X-Forwarded-Proto`.
- `sanitizeNextPath(next, fallback)` blocks protocol-relative (`//evil.com`), absolute URL, and backslash open-redirect payloads while safely merging query parameters (fixing `/reset-password&recovery=true` 404s in `/auth/confirm`).

## Existing Codebase Guide
- **Relevant Code Paths**:
  - `codebase-symlinks/task-tracker-web/supabase/migrations/20260430000000_user_points_summary_view.sql`: Original view definition (immutable per atomic migration rules; must not be edited).
  - `codebase-symlinks/task-tracker-web/supabase/migrations/20240520000011_rls_policies.sql` & `20260427000000_phase3_points_engine.sql`: Underlying `point_transactions` RLS policies (`auth.uid() = user_id` and `is_admin()`).
  - `codebase-symlinks/task-tracker-web/supabase/migrations/20260612000000_tighten_select_policies_authenticated.sql`: Reference migration style for tightening `SELECT` access and documenting regression audit notes.
  - `codebase-symlinks/task-tracker-web/app-src/lib/supabase/database.types.ts`: Generated/maintained Supabase TypeScript schema (`public.Views` at line 602).
  - `codebase-symlinks/task-tracker-web/docker/prod-entrypoint.sh` & `app-src/__tests__/prod-container-config.test.ts`: Production container bootstrap script and its `spawnSync` POSIX shell test suite.
  - `codebase-symlinks/task-tracker-web/app-src/lib/actions/auth.ts` & `lib/actions/__tests__/auth.test.ts`: `requestPasswordReset()`, `updatePassword()`, and server-action unit tests.
  - `codebase-symlinks/task-tracker-web/app-src/lib/actions/admins.ts` & `lib/actions/__tests__/admins.test.ts`: `inviteAdmin()` origin/redirect construction sharing the same proxy header pattern.
  - `codebase-symlinks/task-tracker-web/app-src/app/auth/callback/route.ts` & `app/auth/confirm/route.ts`: PKCE code exchange and OTP `token_hash` verification route handlers.
  - `codebase-symlinks/task-tracker-web/app-src/lib/actions/__tests__/schema-reset.integration.test.ts`: Reference pattern for local-only Supabase RLS/RPC integration tests (auto-skips safely if `NEXT_PUBLIC_SUPABASE_URL` is non-local or missing).
- **Observed Patterns To Follow**:
  - **Append-Only Atomic Migrations**: Never edit committed files in `supabase/migrations/`; create a single-purpose `YYYYMMDDHHMMSS_<descriptive_action>.sql` migration (`supabase-atomic-migrations-guideline.md`).
  - **Typed Config Access**: Encapsulate `process.env` reads inside `app-src/lib/config/` (`environment-and-config.md`) rather than scattering raw `process.env.*` lookups across actions and route handlers.
  - **Centralized Error Normalization & Sentry Tracing**: Route Supabase errors through `actionError(context, error, fallback, allowlist)` (`lib/utils/action-error.ts`) with explicit safe allowlists for user-actionable messages (e.g., rate-limit cooldowns) and zero PII in Sentry tags/extras.
  - **Server-Side Data Only**: All Supabase operations remain strictly inside Server Actions and Route Handlers.
- **Regression-Sensitive Areas**:
  - `getUserTotalPoints()` (`lib/actions/points.ts`) and `getIndividualLeaderboard()` (`lib/actions/leaderboard.ts`) reading `profiles.points` must remain completely unaffected.
  - Clean-checkout `docker compose run --rm prod npm run build` without an `app-src/.env` file (`t-005` of `p-020`) must still succeed using fallback placeholders, while `app-src/.env` values must win whenever `.env` exists.
  - Local dev password recovery (`http://localhost:3000`), admin invitation links (`/auth/confirm?next=%2Fwelcome`), implicit hash recovery (`RecoveryHandler`), and mandatory `must_reset_password` redirects (`/reset-password` without `?recovery=true`) must continue working without regression.

## Code Structure Diagram
```text
agent-workspace/
├── AGENTS.md (symlinked by CLAUDE.md, GEMINI.md)
├── knowledgebase/
│   ├── guidelines/
│   │   └── branch-naming.md                                   [branch strategy reference]
│   └── best-practices/
│       ├── postgres-view-security-invoker.md                  [t-001: new RLS view pattern]
│       └── password-recovery-and-auth-urls.md                 [t-003: new proxy/SMTP/auth guide]
├── specs/p-021-task-tracker-web/
│   ├── impl.md
│   └── t-001.md .. t-004.md
└── codebase-symlinks/task-tracker-web/
    ├── README.MD                                              [t-003: NEXT_PUBLIC_SITE_URL & Supabase Auth/SMTP setup]
    ├── docker/
    │   └── prod-entrypoint.sh                                 [t-002: respect app-src/.env* before fallback export]
    ├── supabase/migrations/
    │   ├── 20260430000000_user_points_summary_view.sql        [unchanged historical migration]
    │   └── 20261026000000_enforce_rls_user_points_summary_view.sql [t-001: new security_invoker + REVOKE/GRANT]
    └── app-src/
        ├── .env.example                                       [t-002: document NEXT_PUBLIC_SITE_URL]
        ├── lib/
        │   ├── config/
        │   │   └── site-url.ts                                [t-002: new typed env + proxy origin + next-path helper]
        │   ├── supabase/
        │   │   └── database.types.ts                          [t-001: type public.Views.user_points_summary]
        │   └── actions/
        │       ├── auth.ts                                    [t-003: normalize email, resolvePublicOrigin, rate-limit allowlist]
        │       ├── admins.ts                                  [t-003: reuse resolvePublicOrigin in inviteAdmin]
        │       └── __tests__/
        │           ├── auth.test.ts                           [t-003: proxy/origin/casing/rate-limit/malicious tests]
        │           ├── admins.test.ts                         [t-003: HTTPS/proxy redirectTo regression tests]
        │           └── user-points-summary-rls.integration.test.ts [t-001: live local Supabase RLS isolation test]
        ├── app/auth/
        │   ├── callback/route.ts                              [t-003: resolvePublicOrigin, error redirect fix, OTP fallback]
        │   └── confirm/route.ts                               [t-003: resolvePublicOrigin, URL query merge fix for ?recovery=true]
        └── __tests__/
            ├── prod-container-config.test.ts                  [t-002: .env precedence vs fallback tests]
            ├── site-url-config.test.ts                        [t-002: unit + malicious host/redirect tests]
            ├── user-points-summary-rls-migration.test.ts      [t-001: static SQL contract & immutability tests]
            └── auth-recovery-routes.test.ts                   [t-003: /auth/callback & /auth/confirm route tests]
```

## UI Mockups
`N/A` — No layout or visual component changes are introduced. Existing `/forgot-password` (`Transmission Sent` and destructive error banner states), `/reset-password?recovery=true`, and `/auth/auth-code-error` screens retain their current Classic/Tactical layouts; `t-003` only improves backend email dispatch reliability, redirect URL resolution, and surfaced rate-limit error copy inside the existing `data-testid="error-message"` alert.

```text
Existing /forgot-password Flow (Unchanged Layout, Fixed Backend Dispatch & Redirects)
[Input: Personnel Identifier] -> [Send Reset Link]
   ├── Success -> [Transmission Sent card] -> Email delivered -> /auth/callback or /auth/confirm -> /reset-password?recovery=true
   └── Rate-limit / Error -> [AlertBanner data-testid="error-message": actionable message]
```

- **States Covered**: `N/A` (existing loading, `success-state`, and `error-message` UI states preserved without visual changes).
- **Interaction Notes**: `N/A`
- **Accessibility Notes**: `N/A`

## Timeline
- **t-001**: Enforce security_invoker RLS and least-privilege grants on public.user_points_summary (Done)
- **t-002**: Fix prod container .env shadowing and add validated public site-URL resolver (Done)
- **t-003**: Harden forgot-password email dispatch, reverse-proxy redirectTo URLs, and auth recovery callbacks (Done)
- **t-004**: Delivery: Commit, PR, Review, CI, Merge (Done)
## Tasks
- [x] t-001: [Bug Fix] Enforce security_invoker RLS and least-privilege grants on public.user_points_summary
- [x] t-002: [Bug Fix] Fix prod container .env shadowing and add validated public site-URL resolver
- [x] t-003: [Bug Fix] Harden forgot-password email dispatch, reverse-proxy redirectTo URLs, and auth recovery callbacks
- [x] t-004: Delivery: Commit, PR, Review, CI, Merge

## Parallel Waves

Generated by `specs.py waves <phase> --write`; regenerate after changing tasks, dependencies, or paths. Tasks in one wave can run side by side.

| Wave | Tasks | Why the rest wait |
|---|---|---|
| 1 | t-001 · t-002 | ready now |
| 2 | t-003 | t-003: depends:t-002 |
| 3 | t-004 | t-004: delivery:last |
