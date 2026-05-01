# UI Design Guideline: Readability & Contrast First

## Core Principle

Always prioritize readability over aesthetics. A beautiful UI that cannot be read is a failed UI.
**Reference mockups in `agent-workspace/knowledgebase/guidelines/design/` are the canonical source of truth for all UI implementation.**

---

## 1. Canonical Tokens (Source of Truth)

To maintain consistent depth, elevation, and contrast, strictly use these canonical tokens. **Never use raw hex colors (e.g., `bg-[#050505]`, `bg-[#0a0a0a]`) directly in JSX.** Always use the semantic CSS variables.

### Surfaces & Backgrounds
- **Page Background:** `--background: #131313` (Not `#050505`)
- **Header / Chrome:** `--header: #050505`
- **Surface Container Low:** `--surface-container-low: #1c1b1b`
- **Surface Container High:** `--surface-container-high: #2a2a2a`

### Borders & Outlines
- **Border (Decorative):** `--border: #222222` (Use only for decorative dividers, never for critical UI outlines on dark backgrounds)
- **Outline (UI Components):** `--outline: #4d4d4d` (or `border-foreground/30`)

### Brand & Interactive
- **Primary:** `--primary: #c70000`
- **Destructive:** `--destructive: #c70000` (Consider distinguishing if needed)
- **Primary Glow:** Use `.glow-primary` or `.hover-glow-primary` classes to apply the intended red shadow `box-shadow: 0 0 15px rgba(199, 0, 0, 0.4)`.

---

## 2. Prevent White-on-White (and Similar Low Contrast Issues)

- Never allow text or UI elements to blend into the background.
- Explicitly check for:
  - White text on white/light backgrounds
  - Light gray on white
  - Dark text on dark backgrounds
- Do not rely on assumptions—verify visually and programmatically.

### Rule

If contrast is questionable, assume it is wrong until proven otherwise.

---

## 3. Enforce Minimum Contrast Standards

Follow accessibility standards (WCAG):

- Normal text: **minimum 4.5:1 contrast ratio**
- Large text (≥ 18pt or 14pt bold): **minimum 3:1**
- UI components (buttons, inputs, borders): **clearly distinguishable (minimum 3:1)**

---

## 4. Validate States Explicitly

Check all UI states:
- Default
- Hover
- Focus
- Active
- Disabled
- Error / Success

Ensure **every state maintains proper contrast and readability**.
*Hover must be visually distinct from idle.*

---

## 5. Prohibit Raw Hex in JSX

- **Never** use raw hex colors like `bg-[#050505]`, `text-[#c70000]`, or `border-[#222222]` inside JSX `className`.
- Always use semantic Tailwind tokens like `bg-background`, `text-primary`, or `border-border`.
- If a token is missing, add it to `globals.css` and use the semantic name.

---

## 6. Manual Verification Checklist

Before shipping:

- [ ] No white-on-white or low-contrast text
- [ ] All text is readable at a glance
- [ ] Buttons and inputs are clearly visible with ≥3:1 contrast outlines
- [ ] Hover states are clearly perceivable (e.g., using `.hover-glow-primary`)
- [ ] Works in both light and dark mode
- [ ] Tested on real devices (not just emulator)
- [ ] No raw hex codes used in JSX

---

## 7. Caveman Rule (Simple but Effective)

If you have to squint, zoom, or guess → it's wrong.

Fix it.
