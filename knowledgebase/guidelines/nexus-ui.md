# Nexus UI System

The Nexus is a high-performance design system built for technical sophistication and rapid information processing. It prioritizes razor-sharp geometry and high-contrast visuals.

## Core Visual Principles

### 1. Razor-Sharp Geometry
- **Border Radius**: Use `rounded-none` for almost all components. 
- **Corners**: Strictly 0px radius. This conveys technical precision and an unyielding character.
- **Borders**: Standard border width is 1px. Use `#222222` for default and `#c70000` for active/focal states.

### 2. High-Contrast Color Palette
The palette is anchored by deep blacks to maximize the impact of the primary red accent.

-   **Base Background**: `#050505` (Deepest black)
-   **Surface/Container**: `#0a0a0a`
-   **Elevated Card**: `#161616`
-   **Primary Accent**: `#c70000` (Deep blood-red)
-   **Text Primary**: `#e5e2e1`
-   **Text Muted**: `#ae8882`

### 3. Typography
-   **Primary (Technical/Headings)**: `Space Grotesk`. Use for headers, numbers, and technical labels.
-   **Secondary (Body)**: Standard sans-serif with high legibility.
-   **Label Style**: Uppercase with 0.1em letter spacing for a "command center" feel.

### 4. Tactile Digital Accents
-   **Glow Effects**: Use a 15% opacity version of the primary red (`#c7000026`) for subtle backlighting on active components.
-   **Status Indicators**: 
    -   **Active/Alert**: Primary red (`#c70000`).
    -   **Safe/Neutral**: Muted grey. Avoid traditional green to maintain the system's aesthetic cohesion.

## Component Implementation Rules

-   **Buttons**: Sharp corners (`rounded-none`), primary red background for main actions, bordered for secondary.
-   **Inputs & Textareas**: 
    -   **Geometry**: Sharp corners (`rounded-none`).
    -   **Background**: Strict `#0a0a0a`.
    -   **Borders**: Bottom-only border (`border-b-1`) using `#222222`.
    -   **Focus State**: Red bottom border (`#c70000`) with a subtle red under-glow.
-   **Typography**:
    -   **Headings & Technical**: `Space Grotesk`.
    -   **Monospace**: Use for data, IDs, and code-like elements.
-   **Cards**: Sharp corners (`rounded-none`), `#161616` background, subtle glow on hover for interactive items.

## Summary: "Cold, Dark, and Fast"
If a component feels "soft" or "playful," it is not Nexus. It should feel like a precision instrument.



## Accessibility & Contrast Audit (2026-04-27)
- **Contrast Standards**: Minimum contrast ratio of 4.5:1 for body text. 
- **Muted Text**: Avoid colors darker than `#888888` on black backgrounds for readable auxiliary data. Standard muted color is `#ae8882`. 
- **Borders**: Minimum border color `#222222` to ensure visibility between different surface tiers. 
- **Interactive States**: Buttons must ensure high contrast between text and background. Primary buttons use white text on `#c70000`. 
- **Form Inputs**: Bottom borders must be clearly visible against the surface background to define the input area.