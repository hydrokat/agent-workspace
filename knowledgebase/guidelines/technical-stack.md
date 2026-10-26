# Technical Stack: HTN Points System

## Frontend
- **Framework**: Next.js 16.4.3 (App Router), React 19.3.1
- **Styling**: Tailwind CSS 4.3.3 (`@tailwindcss/postcss`, `tailwind-merge` 3.7.0)
- **Icons**: Lucide React 1.61.0, React Icons 5.7.0
- **Components**: shadcn/ui (Radix-based primitives: Alert Dialog, Dialog, Label, Progress, Select, Slot, Tabs)
- **Charts**: ApexCharts 8.5.0 + `react-apexcharts` 2.1.1 (Classic dashboard client-only wrappers)
- **Internationalization**: Type-safe dynamic i18n using cookie-based locales (`NEXT_LOCALE`), lazy-loaded server-side dictionaries (`en` / `en-simple`), and a Client language select switcher.

## Backend & Data
- **Database**: Supabase (PostgreSQL)
- **Client Library**: `@supabase/supabase-js` 2.118.0, `@supabase/ssr` 0.12.7
- **Validation**: Zod 4.6.7
- **Environment**: Docker Compose (`node:26-alpine`)

### Schema Notes
- **`team_members` Table**: Has a `status` column (added 2026-04-26 migration). Valid values: `'Pending'`, `'Approved'`. Filter by `status = 'Approved'` when checking active membership.
- **`teams` Table**: The foreign key for the team lead is `fk_team_lead` (references `profiles.id`).

## Authentication & Observability
- **Provider**: Supabase Auth via `@supabase/ssr`
- **Session**: Managed server-side via SSR cookies — no client-side auth library (Next-Auth is NOT used)
- **Password Reset**: Enforced via `middleware.ts`; `must_reset_password` flag on `profiles` table
- **Telemetry**: `@sentry/nextjs` 11.9.0 with instrumentation hooks and normalized server-action error capture

## Development & Test Environment
- **Containerization**: Docker & Docker Compose (`node:26-alpine`)
- **Node Version**: Node.js 26 Active LTS (`engines.node: >=26.0.0`, `.nvmrc: 26`, CI/CD Node 26)
- **Testing**: Jest 30.5.2, `@testing-library/jest-dom` 7.0.1, `@testing-library/react` 16.3.3, Playwright 1.64.1, `@axe-core/playwright` 4.14.0

## Coding Standards & Upstream Holds
- **TypeScript**: Strict mode enabled (`typescript` held at `^6.0.3` for Next.js compiler plugin compatibility)
- **Linting**: ESLint (`eslint` held at `^9.39.5` for `eslint-config-next` peer compatibility)
- **Avatar Generation**: `@dicebear/core` and `@dicebear/collection` held at `^9.4.2`
- **Data Integrity**: **ALL** database interactions, including Supabase queries and mutations, MUST be performed server-side (Server Components or Server Actions). No direct database interactions are permitted in Client Components.
- **State Management**: None — Server Components pattern eliminates need for client state library (Zustand is NOT used)
- **Data Fetching**: Server Components / Server Actions (React Query is NOT used)
