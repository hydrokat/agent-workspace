# Best Practices

Use this directory for reusable patterns, lessons learned, and recommended approaches.

- [Admin Bootstrap](./admin-bootstrap.md): How to gain initial Admin access.
- [Admin Team & Task Management](./admin-team-management.md): Workflows for managing organizational structure.
- [Admin Invite & Onboarding](./admin-invite-onboarding.md): Inviting users by email and auto-enrolling them.
- [Admin Reset](./admin-reset.md): System-wide point reset and audit trail.
- [Domain Layer](./domain-layer.md): Pure business logic helpers (e.g. membership status).
- [Error Normalization](./error-normalization.md): `actionError` pattern — log raw, surface safe.
- [Hashid Integer Invariant](./hashid-integer-invariant.md): Only BIGSERIAL PKs are encoded; UUIDs are never hashed.
- [Operating Principles](./operating_principles.md): Engineering standards for this codebase.
- [React State & TypeScript](./react-state-and-typescript.md): Client-side patterns.
- [Supabase PostgREST Embeds](./supabase-postgrest-embeds.md): Join patterns in select queries.
- [Submission Vouching Pattern](./submission-vouching-pattern.md): Team-lead vouching with self-vouch block (phase-016).
- [Category Point Capping](./category-point-capping.md): Per-category point cap enforcement and accumulation design (phase-016).
