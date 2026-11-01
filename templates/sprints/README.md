# Sprints Directory

This directory stores 1-week sprint plans, prioritization models, and stakeholder deliverables managed by the `project-manager` agent and `/pm-*` skills.

## Directory Structure

```text
sprints/
├── README.md                                          # This guide
└── s-###-<slug>/                                      # Sprint directory (e.g., s-001-core-architecture/)
    ├── sprint.md                                      # Sprint metadata, RICE & MoSCoW scope table, audit log
    ├── business-requirements-document.html            # BRD: business objectives, KPIs, stakeholders
    ├── functional-requirements-document.html          # FRD: user flows, functional requirements (FR-001...)
    └── functional-specification-document.html         # FSD: technical architecture, contracts, edge cases
```

## Sprint Principles

1. **1-Week AI-Assisted Capacity Limit**: Every sprint is sized for a single 1-week execution cycle (default budget: `5.0` effort-days, `<= 3` spec phases, `<= 15` tasks). Single spec phases exceeding `5.0` effort-days or `> 12` tasks must be split across multiple phases.
2. **Dual Prioritization (MoSCoW & RICE)**: Scope is classified under MoSCoW (`Must Have`, `Should Have`, `Could Have`, `Won't Have`) and scored with RICE (`(Reach * Impact * Confidence) / Effort`). Must-Have items should not exceed 70% of total capacity to preserve a buffer for code review and CI.
3. **Sprint Lock Governance**: Once a sprint begins implementation, its scope is locked (`/pm-sprint-lock`). New spec phases cannot be added without an emergency hotfix bypass (`--hotfix --reason`) or explicit unlock double-check (`/pm-sprint-unlock --confirm --reason`).
4. **Self-Contained Stakeholder HTML Deliverables**: Each sprint produces three standalone, print-ready HTML documents (BRD, FRD, FSD) containing embedded styles, no external network dependencies, and no scripts.

## Command Reference

- `/pm-new-sprint`: Plan and scaffold a new 1-week sprint with RICE and MoSCoW priorities.
- `/pm-sprint-explain`: Walk through the active sprint's objectives, scope, and deliverables.
- `/pm-sprint-lock`: Lock sprint scope to prevent mid-flight scope creep.
- `/pm-sprint-unlock`: Unlock a sprint after reviewing scope change justification.
- `/pm-sprint-close`: Close the current sprint and evaluate remaining deliverables and carry-over.
- `/pm-sprint-implement`: Execute the sprint across non-colliding parallel waves.
- `/pm-sprint-status`: View live status, active claims, blockers, and realistic completion ETAs.
- `/pm-sprint-list`: List active, planned, and closed sprints.
- `/pm-commands`: Display a state-aware catalog of all `/pm-*` commands.
