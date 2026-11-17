---
name: pricing
description: Use when designing pricing tiers, selecting value metrics, modeling willingness-to-pay, structuring anchoring and expansion paths, or reviewing a pricing page, or when invoked with /pricing. Adapted from https://skillsmp.com/creators/coreyhaines31/marketingskills/skills-pricing.
---

# Pricing Strategy

Value-based pricing and packaging framework, adapted from [coreyhaines31/marketingskills (pricing)](https://skillsmp.com/creators/coreyhaines31/marketingskills/skills-pricing). Helps teams align price, value metrics, packaging tiers, and pricing page UX with buyer willingness-to-pay.

## When To Use

- Launching pricing for a new SaaS, AI tool, developer platform, or service offering.
- Refactoring flat pricing into tiered value-metric packaging.
- Auditing conversion drop-offs on a pricing page or diagnosing under-monetization.
- Invoked directly via `/pricing`.

## Workflow

### Step 1: Identify Value Metric & Willingness-To-Pay (WTP)

Choose a value metric that scales naturally as the customer gets more value:
- **Seat-based**: Best when collaboration and per-user adoption drive value.
- **Usage / Consumption**: Best for APIs, compute, credits, or variable volume (pair with predictable caps/alerts so buyers do not fear runaway bills).
- **Hybrid (Platform + Usage/Seats)**: Balances predictable base revenue with upside expansion.
- **Outcome / Tiered Capacity**: Best when value correlates with projects, workspaces, or revenue processed.

Benchmark the customer's **next-best alternative** (competitor price, manual labor cost, or cost of the problem unsolved) to anchor WTP on economic value rather than cost-plus margins.

### Step 2: Design Good-Better-Best Packaging

Structure **3–4 clear tiers** (plus Enterprise/Custom when needed), each mapped to a distinct buyer segment:
- **Free / Starter (Acquisition & Activation)**: Proves the core aha-moment without cannibalizing paid value; sets natural graduation triggers.
- **Pro / Growth (Core Revenue Anchor)**: Built for the primary ICP; highlight as "Most Popular" or "Recommended".
- **Team / Business (Collaboration & Governance)**: Adds permissions, SSO/audit logs, shared workflows, SLA, and higher usage limits.
- **Enterprise (Custom / Annual)**: Security review, custom DPA, dedicated support, procurement-friendly invoicing.

Ensure every tier has **1–2 crisp upgrade triggers** (a feature, limit, or compliance need the buyer hits precisely when their value grows).

### Step 3: Apply Anchoring, Billing & Discount Guardrails

- **Annual vs. Monthly**: Default toggle to Annual (e.g., 2 months free / ~17–20% discount) to improve cash flow and retention.
- **Visual Anchoring**: Position the recommended tier cleanly; keep feature comparison rows scannable (max 6–8 key differentiators above the fold, expandable full matrix below).
- **Discount Guardrails**: Never cut list price ad hoc; trade discounts for annual prepay, multi-year commitments, or public case studies.
- **Grandfathering Policy**: Decide upfront how existing customers are treated during price changes to protect trust.

### Step 4: Structure The Pricing Page

Design the page to remove buyer anxiety at the moment of decision:
1. Clear headline tying price to outcome + billing toggle (`Monthly` / `Annual (Save X%)`).
2. 3–4 tier cards with: segment label ("Best for..."), price + unit, primary CTA, and 4–6 differentiated bullets.
3. Social proof strip near the cards (logos, customer metric, or trust badge).
4. Expandable comparison table for technical/procurement buyers.
5. Objection-busting FAQ (refunds/cancellation, overage behavior, seat changes, security/invoicing).

### Step 5: Document The Strategy

Fill `assets/pricing-strategy.md` with the recommended value metric, tier matrix, expansion mechanics, pricing page wireframe copy, and migration/testing plan.

## Mandates

- **Trigger**: Invoke with `/pricing`.
- **Value Over Cost-Plus**: Anchor prices against buyer value and alternatives, never solely against server/COGS costs.
- **Predictability Matters**: When using usage pricing, always define caps, alerts, or bundled allowances so buyers are not afraid to adopt.
- **Clean Differentiation**: Each tier must serve a clearly named buyer segment with unmistakable upgrade triggers.
