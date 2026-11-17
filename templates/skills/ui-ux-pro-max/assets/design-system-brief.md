# Design System Brief: [Product / Feature Name]

**Target Stack**: [Tailwind CSS / shadcn/ui / Next.js / React Native / Flutter]
**Theme Modes**: [Light | Dark | Light + Dark]
**Style Direction**: [Swiss Minimalist | Modern Soft SaaS | Editorial Serif | Dark Technical | High-Contrast Bold]

## 1. Design Direction & Rationale
[Explain the visual direction, emotional tone, and how it supports the product's primary user task.]

## 2. Color Palette & Semantic Tokens

| Token | Light Mode | Dark Mode | Usage & Contrast Target |
|---|---|---|---|
| `--background` | `#FFFFFF` | `#0B0F19` | Page canvas |
| `--surface` | `#F8FAFC` | `#111827` | Cards, panels, sidebars |
| `--foreground` | `#0F172A` | `#F8FAFC` | Primary headings and body (>= 4.5:1) |
| `--muted-foreground` | `#475569` | `#94A3B8` | Secondary labels and captions (>= 4.5:1) |
| `--border` | `#E2E8F0` | `#1F2937` | Card dividers and input outlines |
| `--primary` | `[hex]` | `[hex]` | Primary CTAs and active states |
| `--primary-foreground` | `[hex]` | `[hex]` | Text/icons on `--primary` (>= 4.5:1) |
| `--destructive` | `[hex]` | `[hex]` | Destructive actions and validation errors |

## 3. Typography & Scale

- **Display / Headings**: `[Font Family]`, weights `[600, 700]`
- **Body / UI**: `[Font Family]`, weights `[400, 500, 600]`
- **Monospace / Metrics**: `[Font Family]` (`tabular-nums` for tables and KPIs)

| Role | Size / Line Height | Tracking | Usage |
|---|---|---|---|
| Display / H1 | `36px–48px / 1.1` | `-0.02em` | Hero & page titles |
| H2 | `24px–30px / 1.2` | `-0.015em` | Section headers |
| H3 | `18px–20px / 1.3` | `-0.01em` | Card & group titles |
| Body | `15px–16px / 1.55` | `0` | Primary prose (`max-width: 65ch`) |
| Caption / Label | `12px–13px / 1.4` | `+0.01em` | Metadata, table headers, badges |

## 4. Layout, Spacing & Elevation
- **Container Max Width**: `[1200px / 1280px]` with `[16px / 24px / 32px]` responsive gutters
- **Spacing Scale**: `4px` base grid (`4, 8, 12, 16, 24, 32, 48, 64`)
- **Border Radius**: `[sm: 6px, md: 8px, lg: 12px]`
- **Breakpoints**: `375px` (mobile), `768px` (tablet), `1024px` (laptop), `1440px` (desktop)

## 5. Key Component & State Specs
- **Navigation**: [Top bar / sidebar behavior, active indicator, mobile drawer]
- **Primary Actions & Forms**: [Focus rings, validation errors, disabled/loading states]
- **Data & Feedback**: [Skeleton loaders, empty-state copy + CTA, error recovery]

## 6. UX Anti-Patterns To Avoid
- [List 3-5 project-specific pitfalls, e.g., layout shift on hover, emoji icons, low-contrast muted text.]

## 7. Pre-Delivery Accessibility Checklist
- [ ] WCAG AA contrast verified (>= 4.5:1 normal text, >= 3:1 large text & UI controls)
- [ ] Visible `:focus-visible` ring on every interactive control
- [ ] Semantic landmarks (`header`, `nav`, `main`, `footer`) and labeled form inputs
- [ ] Consistent SVG icon set with descriptive `aria-label` or `aria-hidden`
- [ ] `prefers-reduced-motion` honored and zero horizontal scroll at `375px`
