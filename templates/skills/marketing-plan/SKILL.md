---
name: marketing-plan
description: Use when creating a go-to-market strategy, launch plan, ICP definition, messaging matrix, channel strategy, or 30/60/90-day marketing roadmap, or when invoked with /marketing-plan. Adapted from https://skillsmp.com/creators/coreyhaines31/marketingskills/skills-marketing-plan.
---

# Marketing Plan

Comprehensive go-to-market (GTM) and marketing planning framework, adapted from [coreyhaines31/marketingskills (marketing-plan)](https://skillsmp.com/creators/coreyhaines31/marketingskills/skills-marketing-plan). Translates product capabilities into target segments, sharp positioning, prioritized acquisition channels, and measurable 30/60/90-day execution plans.

## When To Use

- Launching a new product, feature tier, or market expansion.
- Aligning product, growth, and sales around a single ICP and messaging strategy.
- Building a quarterly or 30/60/90-day acquisition and retention roadmap.
- Invoked directly via `/marketing-plan`.

## Workflow

### Step 1: Audit Product, Stage & Constraints

Gather baseline inputs from the codebase, `knowledgebase/`, and user context:
1. **Product Truth**: What the product actually does today, core differentiators, and proof points (benchmarks, case studies, open-source traction).
2. **Stage & Motion**: Pre-launch / waitlist, early PMF (founder-led), PLG self-serve, or sales-assisted B2B.
3. **Constraints**: Team capacity, budget, compliance/brand limits, and timeline.

### Step 2: Define ICP & Buyer Personas

Narrow the primary target before broadening:
- **Primary ICP**: Company/user profile, trigger event that creates urgency, current painful workaround, and budget authority.
- **Buyer vs. User**: Distinguish the champion/end-user from the economic buyer and security/procurement blockers.
- **Anti-ICP**: Who is explicitly out of scope right now so messaging and channels stay sharp.

### Step 3: Craft Positioning & Messaging Matrix

Define positioning using clear competitive contrast:
- **Category & Anchor**: What existing category or mental model the buyer already understands.
- **Core Differentiator**: Why this wins vs. status quo (spreadsheets, manual work, legacy incumbents).
- **Messaging Matrix**: Map each persona to their top pain point, core value proposition, proof point, and primary CTA.

### Step 4: Prioritize Channels & Funnel Loops

Select **2–3 primary channels** based on where the ICP already spends attention and the product's ACV/motion — avoid spreading thin across every platform:
- **Organic / Content / SEO**: Comparison pages, problem-led tutorials, templates, and documentation-as-marketing.
- **Community & Social**: Founder/executive distribution (`/social`), developer communities, targeted partnerships.
- **Outbound / Partnerships / Events**: Account-based outreach, integration co-marketing, webinars.
- **Paid Acquisition**: High-intent search or retargeting only once landing page conversion and unit economics are validated.

Define stage-by-stage funnel metrics across **Awareness -> Acquisition -> Activation -> Retention -> Revenue -> Referral** with a single North Star metric and realistic conversion targets.

### Step 5: Build The 30/60/90-Day Roadmap

Sequence execution so early learnings inform later spend:
- **Days 1–30 (Foundation & Proof)**: Finalize positioning, ship high-converting landing/comparison pages, instrument analytics, validate messaging with 10–20 target buyers.
- **Days 31–60 (Channel Execution & Loops)**: Launch top 2 channels on a repeatable weekly cadence, ship lead magnets/offers (`/offers`), publish customer proof.
- **Days 61–90 (Optimization & Scale)**: Double down on highest-CAC-efficiency channel, prune underperforming experiments, formalize sales enablement (`/sales-enablement`) and expansion plays.

### Step 6: Output The Plan

Fill `assets/marketing-plan.md`, replacing every placeholder with specific, testable deliverables, owners, and KPIs.

## Mandates

- **Trigger**: Invoke with `/marketing-plan`.
- **Focus Over Breadth**: Never recommend 8 simultaneous channels for a small team; rank and commit to 2–3 primary bets.
- **Evidence-Grounded Claims**: Only cite product capabilities, pricing, or metrics verified in the repo or provided by the user.
- **Actionable Deliverables**: Every roadmap phase must list concrete assets, owners, and measurable success criteria.
