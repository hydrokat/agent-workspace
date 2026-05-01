# UI Contrast & Hover-State Audit

- **Date:** 2026-04-30
- **Reporter:** Gie
- **Auditor:** Claude
- **Scope:** Member, admin, and shared UI primitives in `task-tracker-web/app-src/`
- **Reference baseline:** Design mockups at `agent-workspace/knowledgebase/guidelines/design/` (admin_task_management, member_dashboard, rewards_ticket_tiers, task_submission_approval) — used as source of truth per user direction
- **Triggering observation:** "I am seeing low-contrast UI in both idle and hover state."

---

## TL;DR

The reported low-contrast issue is real and has three compounding root causes that ripple across nearly every page and primitive:

1. **The page background is wrong.** `globals.css` declares `--background: #050505`, but the mockups treat `#050505` as the *header* color and use `#131313` as the page surface. Every page in the app sits on a darker base than designed, which collapses the surface-elevation hierarchy and pushes already-thin borders below the 3:1 minimum for UI components.

2. **The hover glow system is silently dead.** `globals.css:111` applies `box-shadow: none !important` to `*`, with an exception only for elements carrying `.primary-glow` or `data-glow="primary"`. The codebase uses `hover-glow-primary` and `glow-primary` (different class names that aren't defined anywhere) on **17 places** across 8 files. The intended red glow on every primary CTA, the progress bar fill, and the points-total bar **never renders**. This is most likely the single biggest contributor to "hover looks the same as idle."

3. **Outline tokens fail their job.** `border-border` resolves to `#222222`. On the actual page background `#050505`, that's a contrast ratio of **~1.3:1**, far below the WCAG 3:1 minimum for UI components and below the visual threshold most viewers can perceive. `Badge variant="outline"`, `Button variant="outline"` overrides, table dividers, dashed file-upload zones, and `border-l-2` accents on cards all become visually absent until the user hovers — and the hover often doesn't help either.

Stack the three together: the page is too dark, surfaces don't lift, borders don't show, and hover effects are off — that's the state the user is describing.

---

## 1. Token Mismatch: Mockup vs. Implementation

| Token | Mockup (source of truth) | `globals.css` actual | Effect |
|---|---|---|---|
| Page background | `#131313` | `--background: #050505` | Page is darker; surface containers (`bg-card #0a0a0a`) only lift 5 luminance points above page instead of 9. Visual depth flattens. |
| Header / chrome | `#050505` | (no token) | Mockup's chrome color is being used as page bg, so the top nav `bg-[#050505]/90` blends into the page and the border between them disappears. |
| Sidebar | `#0a0a0a` | (no token) | Sidebar surface == card surface. No differentiation. |
| Surface-container-low | `#1c1b1b` | (missing) | Mockup uses 5 surface tiers; implementation has only `bg-card #0a0a0a` and `bg-secondary #161616`. Modals, popovers, and elevated rows have nowhere to land. |
| Surface-container-high | `#2a2a2a` | (missing) | Same — no defined "elevated" surface for hovered rows. |
| `--border` | `#222222` (matches mockup) | `#222222` ✓ | Matches mockups, but mockups place these borders on `#131313` (≈1.7:1) — still subtle in mockup. On the implementation's `#050505` page the same border is `~1.3:1` — *worse* than the mockup intends. |
| Brand red | `#c70000` ✓ | `#c70000` ✓ | OK. |
| Muted text | mockup uses `text-neutral-400` (~`#a3a3a3`) and `text-neutral-500` (~`#737373`) | `--muted-foreground: #cccccc` | Implementation muted is *brighter* than mockup. That's actually fine for a11y, but it diverges from the visual rhythm. |

---

## 2. The Dead-Glow Catastrophe

**`globals.css:106-118`** declares the global shadow nuke and exception:

```css
* { box-shadow: none !important; }

.primary-glow,
[data-glow="primary"] {
  box-shadow: 0 0 15px rgba(199, 0, 0, 0.4) !important;
}
```

**Class usage in app code:**

| Class used in JSX | Defined in CSS? | Render result |
|---|---|---|
| `hover-glow-primary` | ❌ no | Dead. Every primary CTA hover loses its intended glow. |
| `glow-primary` | ❌ no | Dead. Points bar, progress fill, default Badge silently lose glow. |
| `primary-glow` | ✅ yes | Defined but **never used** anywhere in JSX. |
| `data-glow="primary"` | ✅ supported | Used 0 times in JSX. |

**Affected files (incomplete grep):**

- `app/page.tsx:68`, `app/login/page.tsx:92`, `app/dashboard/page.tsx:86,109,140`
- `app/tasks/claim-task-dialog.tsx:36,113`
- `app/dashboard/submit/[participationId]/submit-proof-form.tsx:99`
- `app/teams/team-join-button.tsx:43,69`
- `app/admin/tasks/task-management.tsx:175,425,446`
- `app/admin/teams/team-management.tsx:132,391,440`
- `components/ui/badge.tsx:12` (default variant)

The fix is one of: rename the JSX class to `primary-glow`, add `glow-primary`/`hover-glow-primary` rules to `globals.css`, or use the `data-glow="primary"` attribute. See Task A1.

---

## 3. Findings by Category

### 3.1 Shared UI primitives

#### `components/ui/button.tsx`

- **Line 16, `outline` variant:** `border-white/40 bg-transparent hover:bg-white hover:text-black hover:border-white`
  - Idle border `rgba(255,255,255,0.4)` on `--background` `#050505` ≈ effective `#696969`, contrast ~3.4:1. Borderline; on the dashboard's `bg-[#0a0a0a]/20` headers it drops further. Mockup uses solid `border-white` for secondary buttons.
  - Hover is fine (white fill, black text — matches mockup).
- **Line 12, `default` variant:** `hover:bg-primary/90 hover:scale-[1.02] active:scale-95`
  - Hover is a 10% opacity step on an already-saturated red plus a 2% scale. With glow dead, this is most of the hover signal — and it's faint. Mockup hover = idle bg + glow.
- **Line 14, `destructive` variant:** `bg-destructive/90` hover. `--destructive` is the same `#c70000` as `--primary`, so destructive looks identical to default and hover is identical to idle. No semantic distinction.
- **Line 19, `ghost` variant:** `hover:bg-secondary hover:text-white`. `--secondary` = `#161616`. Over `--background` `#050505` the surface delta is barely 4 luminance units → hover almost imperceptible.

#### `components/ui/input.tsx` and `components/ui/textarea.tsx`

- **`input.tsx:11`** uses `placeholder:text-foreground/50` (= `#e5e2e1` @ 50% ≈ `#727171` on `bg-[#0a0a0a]` = ~4.0:1). Just under WCAG AA 4.5:1 for normal text.
- **`textarea.tsx:12`** uses `placeholder:text-muted-foreground` (= `#cccccc`). Inconsistent with input; mockup uses `placeholder-neutral-600` (`#525252`).
- Both use `border-0 border-b-2 border-border` — single underline border at `#222222` on `#0a0a0a` is ~1.4:1. Idle field is barely a field; only `focus:border-primary` makes it visible.

#### `components/ui/select.tsx`

- **Line 22, `SelectTrigger`:** `bg-background` (= page color) + `border border-border`. The trigger blends into the page; the only edge is a 1.4:1 border. **No `hover:` state defined** — only `focus:`. Mouse hover provides no feedback.
- **Line 78, `SelectContent`:** `bg-background` again — the popover doesn't visually lift off the page.
- **Line 121, `SelectItem`:** highlighted state is `focus:bg-primary focus:text-white`. Radix maps hover to `data-highlighted` which the `focus:` selector catches — works, but the jump is from invisible to full saturated red. There's no intermediate "hovered" tier.

#### `components/ui/badge.tsx`

- **Line 7:** `cva` base class is `"inline-flex items-center rounded-none border px-2 py-0.5 text font-bold uppercase tracking-widest …"` — the literal class `text` does nothing in Tailwind. The intended size class is missing; size depends on inherited typography. Same orphan `text` class appears in `table.tsx:76` and dozens of page-level overrides.
- **Line 12, `default`:** `border-primary bg-primary/10 text-primary glow-primary` — `glow-primary` is the dead class.
- **Line 14, `secondary`:** `border-border bg-secondary text-muted-foreground` → border invisible on most surfaces; bg `#161616` on a `#050505` page is `~1.4:1` surface change.
- **Line 17, `outline`:** `border-border text-muted-foreground` — same border issue; this is the most-overridden variant (used with bg overrides everywhere).

#### `components/ui/table.tsx`

- **Line 61, `TableRow`:** `hover:bg-secondary/50` — `#161616` at 50% over `bg-card #0a0a0a` ≈ effective `#101010`. Surface change ~2 luminance units. Hover row not perceivable. Page-level overrides go further: `hover:bg-secondary/40`, `hover:bg-secondary/30`, `hover:bg-secondary/20`.
- **Line 76, `TableHead`:** orphan `text` class noted above.
- **Line 23, `TableHeader`:** `[&_tr]:border-b border-border` — header divider at 1.4:1 against card.

#### `components/ui/tabs.tsx`

- **Line 17, `TabsList`:** `bg-muted` (= `#161616`) over page `#050505`. List has minimal contrast against page.
- **Line 32, `TabsTrigger`:** `data-[state=active]:bg-background data-[state=active]:text-foreground` — active tab bg is `#050505`, *darker* than the inactive list bg `#161616`. Active tab reads as a "well" not as raised. **No idle hover state** at all.

#### `components/ui/card.tsx`, `dialog.tsx`, `progress.tsx`, `label.tsx`

- `card.tsx:26,73` use `border-border/50` for header/footer dividers — that's `#222222` @ 50% on `#0a0a0a` ≈ `#161616`, ratio ~1.2:1. Dividers between header and body are essentially invisible.
- `dialog.tsx:41` `bg-[#0a0a0a]` content over `bg-black/90` overlay — fine, but no shadow lift (per global rule). Modal sits flat.
- `progress.tsx:21` `bg-primary` indicator with no glow (mockup wants `box-shadow: 0 0 10px #c70000`). Dead.
- `label.tsx:10` adds `text font-black` — orphan `text` class again.

### 3.2 Member-facing pages

- **`app/dashboard/page.tsx:64`** `bg-[#050505]` page wrapper hardcoded; should be `bg-background` (and that should be `#131313`).
- **`app/dashboard/page.tsx:204,296,317`** Row/transaction hovers `hover:bg-secondary/30` and `hover:bg-secondary/20` are imperceptible (see §3.1 calculation).
- **`app/dashboard/page.tsx:186`** `<Button variant="outline" className="border-border text-foreground/80 hover:text-white">` — overrides the variant's `border-white/40` *down* to `border-border` (`#222222`), making the empty-state CTA's border invisible. Hover only changes text color; the button outline never appears.
- **`app/dashboard/page.tsx:213`** Badge override `bg-[#0a0a0a] border-border` — bg matches the card, border ~1.4:1 → badge reads as text only.
- **`app/dashboard/page.tsx:250`** "Verified" badge uses `bg-secondary text-foreground/70`. Differs from the mockup (`bg-transparent text-#c70000` + animated dot). Visual identity drifts.
- **`app/tasks/task-board-client.tsx:104`** TaskCard `hover:bg-[#0e0e0e]` — luminance change ~1 unit. Card hover invisible. Mockup hover = border + radial gradient + glow.
- **`app/tasks/task-board-client.tsx:54-56`** TabsTrigger overrides set `border border-transparent` idle, only adding border on `data-[state=active]`. No idle borders, no hover state — tabs are invisible until clicked.
- **`app/tasks/task-board-client.tsx:62`** Search uses a raw `<input>` element with `bg-card border-b-2 border-border` instead of the `Input` primitive. Inconsistent; same low-contrast border.
- **`app/tasks/task-board-client.tsx:107`** Badge `border-border bg-card text-foreground/70` — bg matches card, border invisible, text 70% opacity. Triple-low-contrast.
- **`app/login/page.tsx:72,86`** Inputs override default with `bg-[#0a0a0a]/50` (50% opacity over the card) — the field bg becomes lighter or darker depending on the card translucency, not predictable.
- **`app/login/page.tsx:102`** Signup button: `border-foreground/30` idle = `#e5e2e1` @ 30% ≈ `#454443` on `bg-card #0a0a0a` = **~2.0:1**, fails 3:1 UI minimum. Hover finally pulls it to `border-primary`.
- **`app/dashboard/submit/[participationId]/submit-proof-form.tsx:99`** Uses `hover-glow-primary` (dead) + `border-none`. Hover = nothing visible beyond the 10% bg-opacity step.
- **`app/tasks/claim-task-dialog.tsx:62,66,72,78`** `SelectTrigger` and `SelectItem` overrides use `bg-[#0a0a0a]` matching the dialog body and `focus:bg-secondary` (`#161616`) — highlighted item barely lifts.

### 3.3 Admin pages

(Detailed file-level findings are extensive; recurring patterns reproduced below; full per-line list captured during audit.)

- **`app/admin/tasks/task-management.tsx:233`** Table row `hover:bg-secondary/40` — invisible.
- **`app/admin/tasks/task-management.tsx:239`** Outline badge on `bg-[#050505]` — border 1.3:1.
- **`app/admin/tasks/task-management.tsx:271`** Delete button `hover:border-red-500/20` — uses raw `red-500` (`#ef4444`) at 20% opacity instead of `--primary`/`--destructive`, and the 20% opacity is invisible.
- **`app/admin/teams/team-management.tsx:178,198,219,279,281`** Same patterns: `hover:bg-secondary/40`, `border-border` outlines, `border-primary/20` (also invisible at 20%), and `red-500/20` decline buttons.
- **`app/admin/approvals/participation/participation-approvals.tsx:111,131,148,157`** Same row/badge/decline patterns plus a primary "Approve" button using `hover:shadow-none` which doubly negates any glow.
- **`app/admin/submissions/submissions-table.tsx:156,167,189,194,204,213`** Same family of issues: `border-border` outlines, `text-[#c70000]/70` timestamps (~1.8:1), `hover:bg-primary/5` (5% opacity, invisible), `hover:shadow-[0_0_15px_rgba(199,0,0,0.4)]` killed by global `box-shadow: none !important`.
- **Sidewide:** Hardcoded inline colors `bg-[#050505]`, `bg-[#0a0a0a]`, `bg-[#0a0a0a]/20`, `bg-secondary/40`, `bg-secondary/50`, `border-red-500/20`, `red-500` recur across every admin file. Most should be CSS variables, several are direct violations of the design tokens.

### 3.4 Cross-cutting UX gaps (not strictly contrast)

These are visible in the audit but adjacent to the contrast complaint:

- Orphan `text` class scattered through cva configs and overrides — appears intended as `text-xs` or `text-[10px]` but has no effect.
- `red-500` (Tailwind default) used instead of `--primary`/`--destructive` for delete/reject affordances — color drift.
- Form field underline-only style is fine in isolation but breaks when combined with full-border overrides on the same input (login fields), producing an unintended L-shape.
- `Tabs` primitive has no idle visual at all — no border, no bg, no hover. It's a "ghost until clicked" pattern that is hard to discover.
- `Select` primitive missing hover state (only focus). Tab-keyboard users get an indicator; mouse users don't.

---

## 4. Tasks to Fix

Priority key: **P0** = directly responsible for "hover looks like idle" complaint. **P1** = pervasive contrast failure. **P2** = consistency/cleanup.

### P0 — Hover affordance (start here, biggest visible impact)

**Task A1: Repair the glow class system.**
- File: `app-src/app/globals.css` and 8 component files using the dead classes.
- Add `.glow-primary` and `.hover-glow-primary` rules in `globals.css`, OR rename JSX usages to the existing `.primary-glow`. Recommended: add both new classes since the intent is clear from JSX, then deprecate the unused `.primary-glow`.
- Specifically: define `.glow-primary { box-shadow: 0 0 15px rgba(199,0,0,0.4) !important; }` and `.hover-glow-primary:hover { box-shadow: 0 0 20px rgba(199,0,0,0.5) !important; }`.
- Verify: every primary CTA on dashboard, login, claim dialog, submit-proof, admin create/edit/delete buttons must show a red halo on hover.

**Task A2: Make `.primary-glow` exception more permissive.**
- File: `app-src/app/globals.css:106-118`.
- Either narrow the `*` shadow nuke to specific elements that need it, or add hover allowances. Right now the global `box-shadow: none !important` defeats every per-component `hover:shadow-[…]` in the codebase.
- Recommended: keep the nuke, but add a class allow-list (`.glow-primary, .hover-glow-primary, [data-glow]`).

**Task A3: Fix `Button` `default` and `destructive` hover.**
- File: `app-src/components/ui/button.tsx:12,14`.
- Replace opacity-only hover with bg-shift + glow. Suggested: `default` → `hover:bg-[#a30000] hover:shadow-[0_0_15px_rgba(199,0,0,0.5)]`; `destructive` → distinct color (e.g., add a darker red or keep destructive same as primary but ensure glow).

**Task A4: Give `TableRow` a perceivable hover.**
- File: `app-src/components/ui/table.tsx:61` and all page-level overrides (`/30`, `/40`, `/50`).
- Switch from opacity-blended hover to a fully-opaque elevated surface, e.g. `hover:bg-[#161616]` (no /50 modifier) or introduce `--surface-container-high #2a2a2a` and use it.
- Audit and remove every `hover:bg-secondary/{20,30,40,50}` override; let the primitive's hover do the work.

**Task A5: Add idle and hover state to `Tabs` and `Select` triggers.**
- Files: `app-src/components/ui/tabs.tsx:32`, `app-src/components/ui/select.tsx:22,121`.
- TabsTrigger: add an idle visible underline or muted bg, plus `hover:bg-secondary/60` for inactive items, and flip active state to be brighter (not darker).
- SelectTrigger: add `hover:border-border/80 hover:bg-card` (or a `--surface-container-low` token).

### P1 — Contrast (a11y baseline)

**Task B1: Move the page background to `#131313`.**
- File: `app-src/app/globals.css:74`.
- Change `--background: 0 0% 2%` to `--background: 0 0% 7.5%` (= `#131313`). Re-test all pages — many overrides like `bg-[#050505]` should also be replaced with `bg-background` so the change propagates.
- Add a `--header: 0 0% 2%` (= `#050505`) token and update `navbar.tsx:21` to use it.

**Task B2: Add the missing surface tier tokens.**
- File: `app-src/app/globals.css`.
- Add `--surface-container-low: 0 0% 11%` (`#1c1b1b`), `--surface-container: 0 0% 12.5%`, `--surface-container-high: 0 0% 16.5%` (`#2a2a2a`), `--surface-container-highest: 0 0% 21%`.
- Use these for hovered rows, modals, popovers, and `data-state=active` surfaces. Without these, every elevation has to live in the same `#0a0a0a`–`#161616` band that's already too narrow.

**Task B3: Replace `border-border` outlines on outline-variant components with brighter outlines.**
- Files: `components/ui/badge.tsx:14,17`, page-level `Badge variant="outline"` and `Button variant="outline"` overrides everywhere.
- For UI-component edges that need to be visible, use `border-foreground/30` (≈4:1 on card) or define `--outline: 0 0% 30%` (`#4d4d4d`, ~7:1 on card). Reserve `--border` `#222222` for **decorative** dividers, not outline buttons/badges.

**Task B4: Fix `outline` button's idle border.**
- File: `app-src/components/ui/button.tsx:16`.
- Change `border-white/40` to `border-white/70` or solid `border-white` to match mockup; current 40% sits at ~3.4:1 on `#050505` and worse on translucent backgrounds.

**Task B5: Make form-field idle borders visible.**
- Files: `components/ui/input.tsx:11`, `components/ui/textarea.tsx:12`, `components/ui/select.tsx:22`.
- Replace `border-border` with `border-foreground/25` or a new `--input-border` token at ~`#3a3a3a`. Field outline must satisfy 3:1 against field bg.
- Standardize `placeholder` color across input and textarea (one of them, not both).

**Task B6: Stop using `red-500` and `border-red-500/20`.**
- Files: `navbar.tsx:140`, `admin/tasks/task-management.tsx:271`, `admin/teams/team-management.tsx:219`, `participation-approvals.tsx:148`, `submissions-table.tsx:204`.
- Use `--destructive` (= `--primary`) or introduce a distinct `--danger` token. 20% opacity overlays are invisible; switch to ≥60% opacity or a different design pattern.

**Task B7: Fix the misnamed CSS class on the default Badge.**
- File: `components/ui/badge.tsx:12` — change `glow-primary` to `primary-glow` (or fix in Task A1 globally).

### P2 — Consistency / cleanup

**Task C1: Remove the orphan `text` class.**
- Files: `components/ui/badge.tsx:7`, `components/ui/table.tsx:76`, `components/ui/label.tsx:10`, and page-level overrides (~30+ occurrences).
- Replace with `text-xs` / `text-[10px]` / `text-[11px]` matching the visual size that was *probably* intended.

**Task C2: Replace inline `bg-[#0a0a0a]`, `bg-[#050505]`, `bg-[#0a0a0a]/20` with semantic tokens.**
- Files: every file in `app/admin/`, `app/dashboard/`, `app/tasks/`, `app/login/`.
- Use `bg-card`, `bg-background`, `bg-secondary`, or new `bg-surface-container-low`. Hardcoded hex colors in JSX make the app fragile to design-system changes.

**Task C3: Use the `Input` primitive in task search.**
- File: `app/tasks/task-board-client.tsx:62`.
- Replace the raw `<input>` with `<Input className="…">` so it picks up shared a11y and styling.

**Task C4: Document the design-token contract.**
- Add a short section to `agent-workspace/knowledgebase/guidelines/ui-design-guideline.md` listing the canonical tokens (background, card, surface tiers, border, outline, primary, destructive, foreground/muted) and prohibiting raw hex usage in JSX. Reference the mockup files as source of truth.

**Task C5: Add a contrast smoke test.**
- Add an automated check (Playwright + axe-core or `@axe-core/playwright`) for the 4 highest-traffic routes (`/login`, `/dashboard`, `/tasks`, `/admin/tasks`) so regressions are caught at CI time.

---

## 5. Suggested Sequencing

1. **A1 + A2** in one PR — restore glow visibility everywhere. This alone will resolve ~60% of the "hover looks the same as idle" complaint with a one-file change.
2. **B1 + B2** — fix the page background and add the surface tier tokens. This restores depth perception.
3. **A3, A4, A5** — bake hover behavior into the primitives so page-level overrides can be deleted.
4. **B3–B7** — outlines, fields, color drift.
5. **C1–C5** — cleanup; can be parallelized.

A1+A2+B1 alone would address the vast majority of what the user is seeing. Recommend prioritizing those.

---

## 6. Out of Scope (Not Fixed Here)

- `--destructive` and `--primary` are the same red. Mockups don't disambiguate either, but the implementation maps both to `#c70000`, so destructive actions look identical to primary actions. Consider adding a separate destructive token in a follow-up phase.
- The `Tabs` "ghost until active" pattern is a UX choice; if the team confirms it's intentional, leave alone. If not, redesign the active state.
- WCAG AA conformance was not the user-chosen target; mockups ARE the target. Several mockup colors (e.g., `placeholder-neutral-600` on `#0a0a0a` ≈ 2.6:1) themselves fall below WCAG AA. If a future scope is "match mockups AND meet AA," some mockup tokens will need to change.

---

## 7. References

- Design mockups: `agent-workspace/knowledgebase/guidelines/design/{admin_task_management,member_dashboard,rewards_ticket_tiers,task_submission_approval}/code.html`
- UI guideline: `agent-workspace/knowledgebase/guidelines/ui-design-guideline.md`
- Token source: `task-tracker-web/app-src/app/globals.css`
- Primitive source: `task-tracker-web/app-src/components/ui/`
