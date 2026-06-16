# Domain Layer Convention (`lib/domain/`)

## Context

Established in Phase 014 (Task 007) to separate pure business rules from Server Action
orchestration and Supabase infrastructure code.

## Target Architecture

```
presentation (app/, components/)
  — inferred types only; no business rules; no DB imports
  ↓
application (lib/actions/*)
  — authz handoff; Zod validation at boundary; DB fetch + domain call + DB persist
  ↓
domain (lib/domain/*)
  — pure TypeScript: ranking, capacity, tier thresholds, membership invariants
  — ZERO imports from: supabase, next, zod, or any Node.js I/O
  ↓
infrastructure (lib/supabase/*, external APIs)
  — DB access backed by generated `Database` types
```

### Dependency Direction

Dependencies point **inward only**. Application imports domain; domain imports nothing from
application, infrastructure, or framework layers. This makes domain modules trivially testable
without any mocks.

## Modules Created

| File | Responsibility |
|------|---------------|
| `lib/domain/ranking.ts` | `assignRanks<T>()` — dense-rank computation; `compareByPointsThenName<T>(nameOf)` — comparator factory; `paginateEntries<T>(entries, page, limit)` — 1-based page slicer |
| `lib/domain/tier.ts` | `getTierInfo(points)` + `TIER_THRESHOLDS` constant — discount tier logic |
| `lib/domain/participation.ts` | `resolveRoleLimit()`, `hasCapacity()`, `expandedLimit()` — task slot rules |
| `lib/domain/membership.ts` | `requiredProfileStatusOnMembershipChange()`, `isMemberActive()` |
| `lib/domain/reset.ts` | `REQUIRED_RESET_APPROVALS = 2` — reset governance threshold (cross-referenced in SQL RPCs) |

## Auth Routing Split

Middleware auth routing was extracted to `lib/auth/routing.ts`:

```ts
export function resolveAuthRoute(input: AuthRoutingInput): AuthRoutingResult
// Returns { type: 'pass' } | { type: 'redirect'; to: string }
```

`lib/supabase/middleware.ts` calls `resolveAuthRoute()` and acts on the result. The pure function
covers all redirect cases and is covered by unit tests in `lib/auth/__tests__/routing.test.ts`.

## Auth Guards Split

Auth helpers (`getAuthenticatedUser`, `isAdmin`, `isTeamLead`) were extracted from `teams.ts` to
`lib/auth/guards.ts`. They are re-exported from `teams.ts` for backward compatibility so no
existing imports break.

## Rules for Adding Domain Modules

1. No `import` from `supabase`, `next`, `zod`, or any external API package.
2. Functions take plain data parameters and return plain data — no side effects.
3. Export named types (interfaces) alongside functions so callers can type-check inputs.
4. Place unit tests in `lib/domain/__tests__/` — they should run fast with zero mocks.
5. If logic requires DB state to compute, it belongs in `lib/actions/` (application layer), not
   domain. Only the pure rule itself goes in domain.

## Testing Domain Modules

Domain tests need no Jest mocks because modules have zero I/O dependencies:

```ts
import { assignRanks } from "../ranking";

it("assigns dense ranks with ties", () => {
  const result = assignRanks([{ points: 100 }, { points: 100 }, { points: 50 }]);
  expect(result.map(e => e.rank)).toEqual([1, 1, 3]);
});
```
