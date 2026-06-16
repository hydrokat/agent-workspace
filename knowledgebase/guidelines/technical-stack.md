# Technical Stack: HTN Points System

## Frontend
- **Framework**: Next.js 16.2.4 (App Router)
- **Styling**: Tailwind CSS 4
- **Icons**: react-icons
- **Components**: shadcn/ui (Radix-based)
- **Internationalization**: Type-safe dynamic i18n using cookie-based locales (`NEXT_LOCALE`), lazy-loaded server-side dictionaries (`en` / `en-simple`), and a Client language select switcher.

## Backend & Data
- **Database**: Supabase (PostgreSQL)
- **Client Library**: `@supabase/supabase-js`, `@supabase/ssr`
- **Environment**: Docker Compose

### Schema Notes
- **`team_members` Table**: Has a `status` column (added 2026-04-26 migration). Valid values: `'Pending'`, `'Approved'`. Filter by `status = 'Approved'` when checking active membership.
- **`teams` Table**: The foreign key for the team lead is `fk_team_lead` (references `profiles.id`).

## Authentication
- **Provider**: Supabase Auth via `@supabase/ssr`
- **Session**: Managed server-side via SSR cookies — no client-side auth library (Next-Auth is NOT used)
- **Password Reset**: Enforced via `middleware.ts`; `must_reset_password` flag on `profiles` table

## Development Environment
- **Containerization**: Docker & Docker Compose
- **Node Version**: Managed via Docker (matches `package.json` requirements)

## Coding Standards
- **TypeScript**: Strict mode enabled
- **Data Integrity**: **ALL** database interactions, including Supabase queries and mutations, MUST be performed server-side (Server Components or Server Actions). No direct database interactions are permitted in Client Components.
- **Linting**: ESLint (Next.js config)
- **State Management**: None — Server Components pattern eliminates need for client state library (Zustand is NOT used)
- **Data Fetching**: Server Components / Server Actions (React Query is NOT used)
