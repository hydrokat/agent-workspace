# Sprint s-003: tandem Rust cutover and release

## Metadata
- **Sprint**: s-003-tandem-rust-cutover
- **Status**: planned
- **Start Date**: 2026-11-16
- **End Date**: 2026-11-20
- **Target Window**: 1 Week (7 Days)
- **AI-Assisted Capacity Limit**: 5.0 Days
- **Committed Capacity**: 7.5 Days
- **Total Phases**: 2
- **Total Tasks**: 6
- **Overloaded**: Yes

## Business Objectives & Context
Sprint 3 of 3 in the tandem Rust migration, plus agent-experience enhancements.

**Goals:**
- **p-012 (Must Have):** make Rust the only tandem. This covers the cargo-built release assets with the same contract, CI, the run.sh cargo source fallback (no uv or Python), the installer shipping the crate and removing legacy Python files, porting the README screenshot generator, migration QA across 5 platforms and the live upgrade path, plus docs and the release.
- **p-013 (Should Have):** failed-tests-only test output for agents (the `test-failures` skill), a `demo-recording` skill with burned-in subtitles and human-paced actions for terminal and browser demos, "tandem (agent-workspace)" branding in docs and help text only, and the **active sprint shown in tandem's Status window** (headline, progress, days left, overload). This also fixes the double-counted committed effort in `sprints.py`.

**Capacity warning (accepted by the user on 2026-11-02):** the sprint is **overloaded** at about 7.5 days against a 5.0-day capacity (p-012 4.5 + p-013 3.0), with no buffer left for s-002 spillover. p-013 is Should Have, so it slips to a follow-up sprint first if time runs out. Every p-013 task starts only after p-012 merges.

## Deliverables
- [Business Requirements Document (BRD)](./business-requirements-document.html)
- [Functional Requirements Document (FRD)](./functional-requirements-document.html)
- [Functional Specification Document (FSD)](./functional-specification-document.html)

## Prioritized Scope (MoSCoW & RICE)

| Phase | Title | MoSCoW | Reach | Impact | Confidence | Effort | RICE Score | Spec Link |
|---|---|---|---|---|---|---|---|---|
| p-012-agent-workspace-package | p-012-agent-workspace-package | Must Have | 100 | 3 | 0.8 | 4.5d | 53.33 | [p-012-agent-workspace-package](../../specs/p-012-agent-workspace-package/impl.md) |
| p-013-agent-workspace-package | p-013-agent-workspace-package | Should Have | 100 | 1 | 0.8 | 3d | 26.67 | [p-013-agent-workspace-package](../../specs/p-013-agent-workspace-package/impl.md) |

## Mandatory & Auto-Included Items
- None

## Deferred Backlog (Won't Have)
- None

## Lock & Audit Log
- 2026-11-02 07:21:02: Sprint created with status planned
- 2026-11-02 09:35:24: Added phase p-013-agent-workspace-package (Should Have, RICE: 32.00)
- 2026-11-02: Scope updated: p-013 gained t-005 (active sprint in the Status window); efforts corrected (p-012 4.5d, p-013 3.0d)
