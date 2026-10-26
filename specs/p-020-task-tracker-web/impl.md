# Implementation Plan: Update Node.js to 26 LTS and Upgrade Dependencies

## Branch Strategy
- **Working Branch**: chore/p-020-update-node-and-dependencies
- **Merge Target**: trunk
- **Observed Branch Pattern**: <type>/<short-description> (e.g. chore/p-###-<slug>, feature/phase-###-<slug>, fix/<slug>)
- **Branch Pattern Source**: git log and remote branches in task-tracker-web (PRs #22-#25)

## Repository Scope
- **Phase Directory**: `specs/p-020-task-tracker-web`
- **Scope Type**: Single Repo
- **Primary Repository**: task-tracker-web
- **Involved Repositories**:
  - `task-tracker-web`: Primary application repository (`codebase-symlinks/task-tracker-web`), owning Docker Compose runtime definitions, `Dockerfile`, GitHub Actions CI/CD workflows, `.nvmrc`, and the Next.js application (`app-src/package.json`, `app-src/package-lock.json`, source, and tests).
  - `agent-workspace`: Planning and documentation repository (`specs/p-020-task-tracker-web/`, `knowledgebase/guidelines/`, `AGENTS.md`), delivered alongside `task-tracker-web` in `t-006`.

## Phase Objectives
1. **Standardize on Node.js 26 Active LTS (`26.x`)** across all runtime surfaces: pin `node:26-alpine` / `node:26` in `docker-compose.yml` (replacing floating `node:alpine` and `node:latest`), add `.nvmrc` (`26`), update GitHub Actions workflows (`staging.yml` from `"24"` and `production.yml` from EOL `'20'` to `"26"`), declare `"engines": { "node": ">=26.0.0" }` in `app-src/package.json`, and align `@types/node` to `^26`.
2. **Upgrade all application and development dependencies** in `app-src/package.json` and `app-src/package-lock.json` to their latest compatible releases on npm, including major upgrades where upstream peer dependencies are compatible (`@sentry/nextjs` v10 → v11, `apexcharts` v4 → v8 + `react-apexcharts` v1 → v2, `@testing-library/jest-dom` v6 → v7, `dotenv` v17 → v18).
3. **Respect and document upstream peer-dependency holds** so lockfile resolution stays clean and deterministic:
   - `@dicebear/core` & `@dicebear/collection` held at `^9.4.2` (`@dicebear/collection` latest is `9.4.2` and requires `@dicebear/core@^9.0.0`; no v10 collection package is published).
   - `eslint` updated to latest `9.x` (`^9.39.5`), holding `eslint@10` until `eslint-config-next@16.4.3`'s transitive `eslint-plugin-react@7.37.0` and `eslint-plugin-import@2.32.0` declare `^10` peer support.
   - `typescript` updated to latest `6.x` (`^6.0.3`), holding `typescript@7` (native Go compiler rewrite) until Next.js's `tsconfig` language-service plugin and `typescript-eslint` support TS 7 AST/plugin APIs.
4. **Ensure a self-contained production build and boot through the `docker-compose.yml` `prod` container (`t-005`)**: isolate Alpine `linux-musl` `node_modules` from host OS binaries via a shared named volume, add `libc6-compat` in an Alpine `Dockerfile`, auto-bootstrap `npm ci --include=dev` when container `node_modules` is missing or stale, support configurable `${PROD_PORT:-80}:3000`, and verify both `docker compose run --rm prod npm run build` and `docker compose up -d prod` end-to-end.
5. **Guarantee zero functional, visual, or security regressions** by running TDD contract and component tests, full Jest suite via Node 26 Docker, `tsc --noEmit`, `npm run lint`, `npm run build` (via `prod`), and `npm audit`.

## Architecture

### Staged Upgrade Sequence
Because `app-src/package.json`, `app-src/package-lock.json`, and `docker-compose.yml` are shared runtime/lockfile artifacts, work is sequenced into five focused implementation tasks before final delivery:

1. **Runtime Layer (`t-001`)**: Pin `node:26-alpine` / `node:26` in `docker-compose.yml`, add `.nvmrc`, and update `.github/workflows/{staging,production}.yml` first so all subsequent `docker compose run --rm npm` / `npx` commands execute on Node.js 26 LTS.
2. **Core Framework, Data & UI Layer (`t-002`)**: Add `"engines": { "node": ">=26.0.0" }`, align `@types/node` (`^26`), and upgrade `next` (`16.2.6` → `16.4.3`), `eslint-config-next` (`16.2.6` → `16.4.3`), `react` / `react-dom` (`19.2.6` → `19.3.1`), `@types/react*` (`^19.3.0`), `@supabase/supabase-js` (`^2.118.0`), `@supabase/ssr` (`^0.12.7`), `zod` (`^4.6.7`), `@radix-ui/*`, `lucide-react` (`^1.61.0`), `react-icons` (`^5.7.0`), `tailwindcss` / `@tailwindcss/postcss` (`^4.3.3`), and `tailwind-merge` (`^3.7.0`).
3. **Major Runtime Integrations (`t-003`)**: Migrate `@sentry/nextjs` (`^10.53.1` → `^11.9.0`) across `next.config.ts`, `instrumentation*.ts`, and `sentry.*.config.ts`, and migrate `apexcharts` (`^4.7.0` → `^8.5.0`) + `react-apexcharts` (`^1.9.0` → `^2.1.1`) across `components/classic/charts/*` and `app/globals.css`.
4. **Dev/Test Tooling, Security Audit & Docs (`t-004`)**: Upgrade `jest` / `jest-environment-jsdom` (`^30.5.2`), `@testing-library/jest-dom` (`^7.0.1`), `@testing-library/react` (`^16.3.3`), `@testing-library/dom` (`^10.4.2`), `@playwright/test` / `@playwright/browser-chromium` (`^1.64.1`), `@axe-core/playwright` (`^4.14.0`), `dotenv` (`^18.0.9`), `ws` (`^8.22.0`), `@types/ws` (`^8.18.2`), `eslint` (`^9.39.5`), and `typescript` (`^6.0.3`); execute `npm audit` + full lint/typecheck/test gates; sync `technical-stack.md` and `CHANGELOG.md`.
5. **Docker Compose `prod` Container Build & Runtime Verification (`t-005`)**: Add an Alpine `Dockerfile` (`FROM node:26-alpine` + `libc6-compat`) and `docker/prod-entrypoint.sh`, isolate container `/react-app/node_modules` in a shared named volume (`app_node_modules`), parameterize `${PROD_PORT:-80}:3000`, and verify end-to-end production build (`docker compose run --rm prod npm run build`) and HTTP serving (`docker compose up -d prod`) against the upgraded stack.

### Target Version Matrix

| Category | Package / Runtime | Current | Target | Notes |
|---|---|---|---|---|
| Runtime | Node.js (`.nvmrc`, Docker, CI, `engines`) | Floating / `20` / `24` | `26` (`>=26.0.0`, `node:26-alpine`) | Active LTS track + `Dockerfile` (`libc6-compat`) |
| Framework | `next` / `eslint-config-next` | `16.2.6` | `16.4.3` | Exact version parity between `next` and `eslint-config-next` |
| UI Runtime | `react` / `react-dom` / `@types/react*` | `19.2.6` / `^19` | `19.3.1` / `^19.3.0` | React 19 minor update |
| Backend SDK | `@supabase/supabase-js` / `@supabase/ssr` | `^2.106.1` / `^0.10.3` | `^2.118.0` / `^0.12.7` | Verify SSR cookie helpers in `lib/supabase/*` and `proxy.ts` |
| Telemetry | `@sentry/nextjs` | `^10.53.1` | `^11.9.0` | Major v11 migration; verify `withSentryConfig` and auth error capture |
| Charts | `apexcharts` / `react-apexcharts` | `^4.7.0` / `^1.9.0` | `^8.5.0` / `^2.1.1` | Major v8/v2 migration; verify `ApexOptions` and `.apexcharts-*` CSS |
| Validation & CSS | `zod` / `tailwindcss` / `@tailwindcss/postcss` / `tailwind-merge` | `^4.4.3` / `^4` / `^3.6.0` | `^4.6.7` / `^4.3.3` / `^3.7.0` | Semver-compatible updates |
| UI & Icons | `@radix-ui/*` / `lucide-react` / `react-icons` | `1.1.x-2.2.x` / `^1.16.0` / `^5.6.0` | Latest `1.x/2.x` / `^1.61.0` / `^5.7.0` | Verify Radix slot/dialog/select/tabs types |
| Test & E2E | `jest*` / `@testing-library/jest-dom` / `@playwright/*` / `@axe-core/playwright` | `^30.4.x` / `^6.9.1` / `^1.60.0` / `^4.11.3` | `^30.5.2` / `^7.0.1` / `^1.64.1` / `^4.14.0` | `jest-dom` v7 marks `vitest` optional |
| Node & Env Tooling | `@types/node` / `dotenv` / `ws` / `@types/ws` | `^25` / `^17.4.2` / `^8.21.0` / `^8.18.1` | `^26` / `^18.0.9` / `^8.22.0` / `^8.18.2` | Align `@types/node` to Node 26 |
| Held Peer Blockers | `@dicebear/*`, `eslint`, `typescript` | `^9.4.2`, `^9`, `^6` | `^9.4.2`, `^9.39.5`, `^6.0.3` | Held from v10/v10/v7 due to upstream peer/plugin incompatibilities |

## Existing Codebase Guide
- **Relevant Code Paths**:
  - `codebase-symlinks/task-tracker-web/docker-compose.yml`: Defines `react-app`, `prod`, `init`, `init-next`, `npm`, `npx`, and `frontend-web` Docker services used for local development, production builds (`prod`), and containerized Jest runs.
  - `codebase-symlinks/task-tracker-web/.github/workflows/staging.yml` & `production.yml`: CI verification (`npm ci`, `npm run lint`, `npm run test`) and SSH deploy pipelines (`docker compose build --parallel && docker compose run --rm npm install && docker compose down && docker compose up prod -d`).
  - `codebase-symlinks/task-tracker-web/app-src/package.json` & `package-lock.json`: Single npm manifest and lockfile for the Next.js application.
  - `codebase-symlinks/task-tracker-web/app-src/lib/supabase/server.ts`, `middleware.ts`, `admin.ts`, `client.ts`, and `app-src/proxy.ts`: Supabase SSR/browser clients and Next.js 16 proxy entry.
  - `codebase-symlinks/task-tracker-web/app-src/next.config.ts`, `instrumentation.ts`, `instrumentation-client.ts`, `sentry.server.config.ts`, `sentry.edge.config.ts`, and `lib/utils/action-error.ts`: Sentry Next.js instrumentation and server-action error normalization.
  - `codebase-symlinks/task-tracker-web/app-src/components/classic/charts/{area-chart,bar-chart,radial-progress}.tsx` and `app/globals.css`: Client-only ApexCharts wrappers (`dynamic(..., { ssr: false })`) and light/dark CSS overrides.
  - `codebase-symlinks/task-tracker-web/app-src/jest.config.ts`, `jest.setup.js`, `jest.setup.ts`, `test-utils/dicebear-stub.js`, and `__tests__/*`: Jest 30 + `next/jest` test setup and 29 existing test suites.
- **Observed Patterns To Follow**:
  - **Server-Side Data Only**: All Supabase queries/mutations remain strictly inside Server Components, Route Handlers, or Server Actions (`lib/actions/*`, `lib/supabase/*`).
  - **Containerized Test & Build Execution**: Run Jest (`docker compose run --rm npx jest`) and production builds (`docker compose run --rm prod npm run build`) via Docker Compose using isolated Alpine `musl` `node_modules` so host OS native binaries never collide.
  - **Exact Framework Pairing**: `next` and `eslint-config-next` are pinned to the exact same version string (`16.4.3`), and `react` / `react-dom` are pinned to the exact same version string (`19.3.1`).
  - **Centralized Error Tracing**: `lib/utils/action-error.ts` normalizes server errors and reports unexpected exceptions via `Sentry.captureException` without leaking raw DB internals.
- **Regression-Sensitive Areas**:
  - `staging.yml` deployment sequence (`docker compose build --parallel` → `docker compose run --rm npm install` → `docker compose down` → `docker compose up prod -d`) and default port `80:3000` on `prod`.
  - Alpine `musl` native bindings (`@next/swc-linux-*-musl`, `@tailwindcss/oxide-linux-*-musl`, `lightningcss-linux-*-musl`, `@sentry/cli`) during `next build` inside `prod`.
  - Supabase SSR cookie refresh in `proxy.ts` → `lib/supabase/middleware.ts` and `lib/supabase/server.ts` after `@supabase/ssr` `0.10.x` → `0.12.x`.
  - Sentry auth error capture (`__tests__/sentry-auth-tracing.test.ts`) and build-time `withSentryConfig` options in `next.config.ts` after `@sentry/nextjs` v10 → v11.
  - Classic dashboard chart rendering and light/dark theme tooltip/axis contrast after `apexcharts` v4 → v8 and `react-apexcharts` v1 → v2.
  - Jest DOM matchers (`toBeInTheDocument`, `toHaveAttribute`, etc.) across all `.test.tsx` suites after `@testing-library/jest-dom` v6 → v7.

## Code Structure Diagram
```text
agent-workspace/
├── AGENTS.md (symlinked by CLAUDE.md, GEMINI.md)
├── knowledgebase/guidelines/
│   ├── technical-stack.md                  [t-004: Node 26 + stack versions]
│   └── branch-naming.md                    [branch strategy reference]
├── specs/p-020-task-tracker-web/
│   ├── impl.md
│   └── t-001.md .. t-006.md
└── codebase-symlinks/task-tracker-web/
    ├── .nvmrc                              [t-001: new, pins 26]
    ├── Dockerfile                          [t-005: new, node:26-alpine + libc6-compat]
    ├── docker/
    │   └── prod-entrypoint.sh              [t-005: new, Alpine node_modules sync + build/start]
    ├── docker-compose.yml                  [t-001: Node 26 tags; t-005: prod build, volume, port]
    ├── README.MD                           [t-005: document prod container build & run]
    ├── .github/workflows/
    │   ├── staging.yml                     [t-001: node-version "26"]
    │   └── production.yml                  [t-001: node-version '26', setup-node@v6]
    └── app-src/
        ├── .nvmrc                          [t-001: new, pins 26]
        ├── package.json                    [t-002..t-004: engines + upgrades]
        ├── package-lock.json               [t-002..t-004: regenerated on Node 26]
        ├── next.config.ts                  [t-003: @sentry/nextjs v11 config]
        ├── instrumentation*.ts             [t-003: Sentry v11 hooks]
        ├── sentry.{server,edge}.config.ts  [t-003: Sentry v11 init]
        ├── proxy.ts                        [t-002: Next 16.4 + @supabase/ssr]
        ├── lib/supabase/                   [t-002: @supabase/ssr 0.12 + supabase-js 2.118]
        ├── components/classic/charts/      [t-003: apexcharts v8 + react-apexcharts v2]
        ├── app/globals.css                 [t-003: .apexcharts-* selector parity]
        ├── jest.config.ts & jest.setup.*   [t-004: Jest 30.5 + jest-dom v7]
        ├── eslint.config.mjs               [t-004: eslint 9.39 + eslint-config-next 16.4.3]
        ├── playwright.config.ts            [t-004: Playwright 1.64 + dotenv 18]
        ├── CHANGELOG.md                    [t-004: release notes]
        └── __tests__/                      [t-001..t-005: new + 29 existing test suites]
```

## UI Mockups
This phase introduces **no visual redesign**, but `t-002` (Radix UI, Tailwind CSS 4.3, Lucide) and `t-003` (`apexcharts` v4 → v8 + `react-apexcharts` v1 → v2) touch rendered Classic dashboard chart components (`area-chart.tsx`, `bar-chart.tsx`, `radial-progress.tsx`). Visual parity across Classic/Tactical styles and Light/Dark modes is required:

```text
Desktop (>=1024px) — Classic Dashboard Charts Row
+-----------------------------------------------+  +-------------------------+
| Points Activity (AreaChart / BarChart)        |  | Tier Progress (Radial)  |
|  120 |          /\                            |  |        .-----.          |
|   80 |    /\   /  \    /\                     |  |       /  75%  \         |
|   40 |___/  \_/    \__/  \____                |  |       \  pts  /         |
|       Mon  Tue  Wed  Thu  Fri                 |  |        '-----'          |
|  [Hover Tooltip: legible surface + contrast]  |  |  Current -> Next Tier   |
+-----------------------------------------------+  +-------------------------+

Mobile (<768px) — Stacked Single-Column Cards
+-----------------------------------------------+
| Points Activity (100% width, responsive SVG)  |
+-----------------------------------------------+
| Tier Progress (Radial centered, 100% width)   |
+-----------------------------------------------+
```

- **States Covered**: SSR client-hydration placeholder (`ssr: false` dynamic import without `window` crash), populated chart series state, zero-point / empty series state, and hover tooltip active state in both Light and Dark themes.
- **Interaction Notes**: Chart hover tooltips, Radix dialogs/selects/tabs keyboard focus traps, and theme/style switchers must behave identically before and after upgrade.
- **Accessibility Notes**: Preserve `.apexcharts-text`, `.apexcharts-gridline`, and `.apexcharts-tooltip` contrast overrides in `app/globals.css` for Light and Dark modes, and maintain `@axe-core/playwright` zero-violation checks.

## Timeline
- **t-001**: Standardize Node.js 26 LTS across Docker Compose, CI workflows, and .nvmrc (Done)
- **t-002**: Upgrade Node engine metadata, Next.js 16.4, React 19.3, Supabase, and UI packages (Done)
- **t-003**: Migrate major runtime integrations (@sentry/nextjs v11 and apexcharts v8) (Done)
- **t-004**: Upgrade test, lint, and build devDependencies, audit security, and sync docs (Done)
- **t-005**: Enable and verify production builds through the Docker Compose prod container (Done)
- **t-006**: Delivery: Commit, PR, Review, CI, Merge (Done)
## Tasks
- [x] t-001: [Enhancement] Standardize Node.js 26 LTS across Docker Compose, CI workflows, and .nvmrc
- [x] t-002: [Enhancement] Upgrade Node engine metadata, Next.js 16.4, React 19.3, Supabase, and UI packages
- [x] t-003: [Enhancement] Migrate major runtime integrations (@sentry/nextjs v11 and apexcharts v8)
- [x] t-004: [Enhancement] Upgrade test, lint, and build devDependencies, audit security, and sync docs
- [x] t-005: [Enhancement] Enable and verify production builds through the Docker Compose prod container
- [x] t-006: Delivery: Commit, PR, Review, CI, Merge

## Parallel Waves

Generated by `specs.py waves <phase> --write`; regenerate after changing tasks, dependencies, or paths. Tasks in one wave can run side by side.

| Wave | Tasks | Why the rest wait |
|---|---|---|
| 1 | t-001 | ready now |
| 2 | t-002 | t-002: depends:t-001 |
| 3 | t-003 | t-003: depends:t-002 |
| 4 | t-004 | t-004: depends:t-003 |
| 5 | t-005 | t-005: depends:t-004 |
| 6 | t-006 | t-006: delivery:last |
