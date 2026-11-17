---
name: ui-ux-pro-max
description: Use when designing UI/UX systems, selecting visual styles, color palettes, typography pairings, layout grids, UX heuristics, or accessibility checklists, or when invoked with /ui-ux-pro-max. Adapted from https://github.com/nextlevelbuilder/ui-ux-pro-max-skill for vendor-agnostic design intelligence.
---

# UI/UX Pro Max

Design intelligence framework for web and mobile products, adapted from [ui-ux-pro-max-skill](https://github.com/nextlevelbuilder/ui-ux-pro-max-skill). Produces cohesive, accessible, implementation-ready design systems and UI specifications.

## When To Use

- Defining or auditing a product's visual language, design tokens, or component system.
- Designing landing pages, SaaS dashboards, onboarding flows, pricing tables, or mobile interfaces.
- Reviewing existing UI for visual hierarchy, responsive behavior, motion restraint, or WCAG AA/AAA accessibility.
- Invoked directly via `/ui-ux-pro-max`.

## Workflow

### Step 1: Analyze Product & Context

Clarify four inputs before choosing aesthetics:
1. **Product Type**: SaaS dashboard, marketing landing page, e-commerce storefront, developer tool, editorial site, or mobile app.
2. **Audience & Brand Tone**: Technical/utilitarian, executive/trustworthy, editorial/refined, playful/consumer, or luxury/minimal.
3. **Target Stack & Constraints**: Tailwind CSS, shadcn/ui, React/Next.js, plain HTML/CSS, React Native, or Flutter; light-only, dark-only, or dual-theme.
4. **Primary Conversion / Task Goal**: What the user must understand or accomplish within the first 5 seconds.

### Step 2: Select Visual Style & Layout System

Pick one coherent visual direction rather than mixing conflicting tropes:

| Style Direction | Core Characteristics | Best For | Avoid When |
|---|---|---|---|
| **Swiss / Minimalist** | Crisp 12-column grid, generous whitespace, stark typographic hierarchy, 0-6px radius, hairline borders | Developer tools, enterprise SaaS, documentation, fintech | Playful consumer entertainment |
| **Modern Soft SaaS** | Subtle surface elevation, 8-12px radius, clean sans-serifs, soft neutral borders, restrained accent fills | B2B SaaS, productivity apps, analytics dashboards | High-fashion editorial |
| **Editorial / Serif-Led** | Expressive serif display headings paired with neutral sans body, asymmetrical grids, warm paper tones | Brand storytelling, agencies, content platforms | Dense real-time trading/ops UI |
| **Dark Technical / Glass** | Deep slate/obsidian surfaces, 1px translucent borders, high-contrast foregrounds, crisp data visualization | AI products, security consoles, observability dashboards | Print-heavy or outdoor mobile usage |
| **High-Contrast Bold** | Saturated primary accents, oversized display scale, tight grids, decisive CTAs | Product launches, campaign landing pages, waitlists | Long-session back-office workflows |

### Step 3: Define Color Tokens & Contrast Rules

Establish semantic tokens rather than raw hex values scattered through components:
- **Surfaces**: `background` (canvas), `surface` (cards/panels), `surface-elevated` (popovers/modals).
- **Text**: `foreground` (primary body/headings), `muted-foreground` (secondary labels), never below `4.5:1` contrast (`3:1` for large text >= 18pt / 14pt bold).
- **Borders & Hairlines**: Visible in both light (`#E2E8F0` / `slate-200`) and dark (`rgba(255,255,255,0.12)`) modes; avoid invisible `white/10` borders on light backgrounds.
- **Interactive Accents**: `primary` + `primary-foreground`, plus semantic `success`, `warning`, `destructive` paired with icons or labels (never color alone).

### Step 4: Choose Typography Pairing & Scale

Select one purposeful font pairing and stick to a strict modular scale:
- **Clean SaaS / UI**: `Plus Jakarta Sans` or `Inter` (headings + body) with `JetBrains Mono` for code/metrics.
- **Technical / Developer**: `Space Grotesk` or `Geist` headings + `Inter` body + `JetBrains Mono` data.
- **Editorial / High-Trust**: `Newsreader` or `Instrument Serif` headings + `Inter` or `Work Sans` body.
- **Geometric Modern**: `Outfit` headings + `Work Sans` body.

Enforce readable measures (`55–75ch` for prose), tabular numerals (`tabular-nums`) for tables/pricing/KPIs, and crisp heading-to-body contrast.

### Step 5: Apply UX Heuristics & Anti-Patterns

- **Icons & Visuals**: Use a single consistent SVG icon set (e.g., Lucide or Heroicons) at fixed sizes (`16px` / `20px` / `24px`); never use raw emojis as UI icons.
- **Interaction & Motion**: Add `cursor-pointer` on interactive cards/controls; keep micro-transitions to `150–250ms` using `transform` and `opacity`; avoid hover transforms that cause layout shift; respect `prefers-reduced-motion: reduce`.
- **Layout & Navigation**: Pad main content below fixed navbars; standardize container max-widths (`max-w-6xl` or `max-w-7xl`); test viewports at `375px`, `768px`, `1024px`, and `1440px` with zero horizontal overflow.
- **Async & Empty States**: Always specify loading skeletons, empty states with next actions, and inline error recovery.

### Step 6: Produce The Design System Brief

Fill `assets/design-system-brief.md` with concrete tokens, typography rules, layout specs, component guidelines, and the pre-delivery checklist. Hand off implementation specs cleanly to `frontend-dev`.

## Pre-Delivery Checklist

- [ ] Contrast verified >= 4.5:1 for body text and >= 3:1 for large text and UI boundaries in all supported themes.
- [ ] Consistent SVG icon library; zero emoji icons in product chrome.
- [ ] Keyboard focus rings visible (`:focus-visible`), all form inputs labeled, semantic HTML landmarks used.
- [ ] Touch targets >= 44x44px on mobile; zero horizontal scroll at 375px.
- [ ] `prefers-reduced-motion` respected; no infinite decorative animations.
- [ ] Loading, empty, and error states defined for every data-driven view.

## Mandates

- **Trigger**: Invoke with `/ui-ux-pro-max`.
- **Tokens Over Ad-Hoc Values**: Always output named design tokens and rules, not arbitrary one-off colors.
- **Accessibility Is Non-Negotiable**: Never trade WCAG contrast or keyboard focus visibility for aesthetics.
- **Implementation Handoff**: Provide exact CSS/Tailwind/component guidance so `frontend-dev` can build without guessing.
