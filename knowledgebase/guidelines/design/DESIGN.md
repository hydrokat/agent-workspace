---
name: Nexus
colors:
  surface: "#131313"
  surface-dim: "#131313"
  surface-bright: "#3a3939"
  surface-container-lowest: "#0e0e0e"
  surface-container-low: "#1c1b1b"
  surface-container: "#201f1f"
  surface-container-high: "#2a2a2a"
  surface-container-highest: "#353534"
  on-surface: "#e5e2e1"
  on-surface-variant: "#e7bdb6"
  inverse-surface: "#e5e2e1"
  inverse-on-surface: "#313030"
  outline: "#ae8882"
  outline-variant: "#5d3f3a"
  surface-tint: "#ffb4a8"
  primary: "#ffb4a8"
  on-primary: "#690000"
  primary-container: "#c70000"
  on-primary-container: "#ffd4ce"
  inverse-primary: "#c00000"
  secondary: "#c8c6c5"
  on-secondary: "#313030"
  secondary-container: "#474746"
  on-secondary-container: "#b7b5b4"
  tertiary: "#c6c6c7"
  on-tertiary: "#2f3131"
  tertiary-container: "#606262"
  on-tertiary-container: "#dddede"
  error: "#ffb4ab"
  on-error: "#690005"
  error-container: "#93000a"
  on-error-container: "#ffdad6"
  primary-fixed: "#ffdad4"
  primary-fixed-dim: "#ffb4a8"
  on-primary-fixed: "#410000"
  on-primary-fixed-variant: "#930000"
  secondary-fixed: "#e5e2e1"
  secondary-fixed-dim: "#c8c6c5"
  on-secondary-fixed: "#1c1b1b"
  on-secondary-fixed-variant: "#474746"
  tertiary-fixed: "#e2e2e2"
  tertiary-fixed-dim: "#c6c6c7"
  on-tertiary-fixed: "#1a1c1c"
  on-tertiary-fixed-variant: "#454747"
  background: "#131313"
  on-background: "#e5e2e1"
  surface-variant: "#353534"
typography:
  headline-xl:
    fontFamily: Space Grotesk
    fontSize: 48px
    fontWeight: "700"
    lineHeight: "1.1"
    letterSpacing: -0.04em
  headline-lg:
    fontFamily: Space Grotesk
    fontSize: 32px
    fontWeight: "600"
    lineHeight: "1.2"
    letterSpacing: -0.02em
  headline-md:
    fontFamily: Space Grotesk
    fontSize: 24px
    fontWeight: "600"
    lineHeight: "1.3"
    letterSpacing: -0.01em
  body-lg:
    fontFamily: Space Grotesk
    fontSize: 18px
    fontWeight: "400"
    lineHeight: "1.6"
    letterSpacing: 0em
  body-md:
    fontFamily: Space Grotesk
    fontSize: 16px
    fontWeight: "400"
    lineHeight: "1.6"
    letterSpacing: 0em
  label-md:
    fontFamily: Space Grotesk
    fontSize: 12px
    fontWeight: "700"
    lineHeight: "1"
    letterSpacing: 0.1em
spacing:
  unit: 4px
  xs: 4px
  sm: 8px
  md: 16px
  lg: 24px
  xl: 48px
  gutter: 16px
---

## Brand & Style

The design system is built on the "Nexus" philosophy—a fusion of rigorous structural precision and aggressive, high-energy accents. It is designed for high-performance environments where technical sophistication and rapid information processing are paramount. The personality is authoritative, futuristic, and unyielding.

The style leverages **High-Contrast Minimalism** layered over a **Tactile Digital** framework. It uses razor-sharp geometry to convey professionalism, while integrating subtle "glowing" light leaks and dynamic movement to prevent the UI from feeling static or sterile. The goal is an interface that feels like a precision instrument: cold, dark, and incredibly fast.

## Colors

The palette is anchored by a void-like neutral base to maximize the impact of the primary red.

- **Primary (#c70000):** A deep, blood-red used sparingly for critical actions, status indicators, and focal points. It is often accompanied by a soft glow effect to increase perceived luminance without causing eye strain.
- **Surface Tiers:**
  - **Base:** #050505 (Deepest black for the background).
  - **Surface:** #0a0a0a (Primary container background).
  - **Elevated:** #161616 (Secondary containers and cards).
- **Accents:**
  - **Glow:** A semi-transparent 15% opacity version of the primary red used for subtle backlighting.
  - **Borders:** #222222 for standard separation; #c70000 for active states.
- **Typography:** Pure white (#ffffff) for high-contrast headers, and muted grey (#888888) for auxiliary data.

## Typography

This design system exclusively utilizes **Space Grotesk** to maintain a cohesive, technical aesthetic.

- **Headlines:** Set with tight tracking and heavy weights. They should feel architectural and commanding.
- **Body:** Open tracking and standard weights ensure readability against the dark background.
- **Labels:** Always uppercase with generous letter-spacing to mimic data-readouts and telemetry.
- **Special Case:** Numerical data should use tabular figures where possible to align with the grid-based layout.

## Layout & Spacing

The layout follows a **Rigid Fluid Grid**. While the layout adapts to screen size, elements must always snap to a 4px baseline grid.

- **Grid:** A 12-column system with narrow 16px gutters to maintain a "dense" professional feel.
- **Rhythm:** Vertical rhythm is strictly enforced in multiples of 8px.
- **Borders as Spacing:** Use 1px solid borders (#222222) instead of whitespace to define zones, emphasizing the "Nexus" nature of the design. Elements should feel slotted into a machine.

## Elevation & Depth

Depth is conveyed through **Tonal Stacking** and **Light Emission** rather than traditional shadows.

- **Layering:** Backgrounds are the darkest point. Each interactive layer above it becomes slightly lighter (e.g., #050505 -> #0a0a0a -> #161616).
- **Glow Effects:** Instead of drop shadows, use `box-shadow: 0 0 15px rgba(199, 0, 0, 0.2)` on active or primary elements. This creates a "neon-on-black" depth that suggests energy.
- **Borders:** High-elevation elements (modals) use a subtle 1px border of #333333 to separate themselves from the surfaces below.

## Shapes

The shape language is **Strictly Geometric**.

- **Corners:** 0px radius (Sharp) for all primary containers, buttons, and input fields. This reinforces the technical, non-generic edge.
- **Exceptions:** Icons may contain curves for legibility, but they should be housed within square framing.
- **Lines:** All dividers must be 1px or 2px—no soft gradients or feathered edges.

## Components

- **Buttons:**
  - _Primary:_ Solid #c70000 background, white text, sharp corners. On hover, add a 10px red outer glow.
  - _Secondary:_ Transparent background, 1px #ffffff border.
- **Inputs:** Dark #0a0a0a fill with a bottom-only border of #333333. On focus, the bottom border turns #c70000 with a subtle red "under-glow."
- **Chips:** Small, square-edged boxes with #161616 backgrounds and #888888 text. Active chips use red text and a 1px red border.
- **Cards:** No shadows. Use #0a0a0a background and a #222222 border. Headline text inside cards should always be bold.
- **Status Indicators:** Use the primary red for "Alert" or "Active" states. For "Safe" states, use a muted grey rather than green to keep the focus on the red accent system.
- **Data Grids:** Alternating row highlights using #080808 and #0a0a0a. High-contrast white text for primary data.
