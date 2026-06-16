# Task 008: Knowledgebase update

## Description
Once the phase is implemented, update the knowledgebase so it reflects the new flows and
patterns. This task satisfies the workspace's Documentation-Sync principle and the explicit
spec instruction to update the knowledgebase after the spec is delivered.

## Existing Implementation & Guidelines
- `knowledgebase/business-flows/` — `core-contribution-loop.md`, `core-spec.md` (document the
  claim → submit → approve → points loop). These must be extended for vouching + caps.
- `knowledgebase/best-practices/` — e.g. `error-normalization.md`, `domain-layer.md`,
  `admin-team-management.md`. Add/extend entries for the new authorization + capping patterns.
- Use the `/knowledge` skill to keep formatting and structure consistent.

## Approach
1. **Business flows:** update `core-contribution-loop.md` (and `core-spec.md` if needed) to
   describe team-lead **vouching** (incl. the self-vouch restriction) alongside admin approval,
   and the **category point-cap** gate at claim time.
2. **Best practices:** add a note covering:
   - Team-lead authorization pattern (action + `SECURITY DEFINER` internal guard + RLS scoping),
     including the self-action prohibition.
   - Per-category point-capping design (on-the-fly accumulation, reset windows, manual reset).
3. **Categories:** document the categories data model and the category-required-on-task rule.
4. Cross-link from relevant `README.md` indexes.

## Objectives
- [ ] Vouching flow (with self-vouch restriction) documented in business-flows
- [ ] Category + point-cap model and enforcement documented (best-practices + business-flows)
- [ ] New authorization/capping best-practice note(s) added and cross-linked
- [ ] Docs match the shipped implementation (no aspirational/inaccurate claims)

## Unit Testing
- [ ] N/A (documentation). Validation = review for accuracy against merged code (link PRs/commits).

## Security Audit
- [ ] Docs do not expose secrets, internal ids, or bypass instructions.
- [ ] Authorization rules described match the enforced RLS/guards (no misleading guidance).

## Status
- [ ] Pending
