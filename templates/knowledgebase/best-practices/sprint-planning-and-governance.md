# Sprint Planning and Governance

Best practices for planning, prioritizing, locking, and executing 1-week AI-assisted human development sprints.

## 1-Week Capacity Sizing

AI-assisted teams work in rapid 1-week cycles. Sprints must be realistically sized to ensure honest delivery commitments:
- **Baseline Budget**: Default 5.0 effort-days per sprint.
- **Phase Limit**: At most 3 distinct `specs/p-###-*` phases per sprint.
- **Task Limit**: At most 12 total implementation tasks across all phases.
- **Oversized Work**: When a single planned spec exceeds 5.0 effort-days or 12 implementation tasks, split it into multiple cohesive `p-###-*` phases and package them with `/pm-new-sprint`.
- **Automated Sizing**: `sprints.py discover` scans existing backlog specs, classifies items as mandatory (carry-overs and bug fixes), recommended, or optional, and warns when committed work exceeds capacity.

## Dual Prioritization: RICE & MoSCoW

Every phase in a sprint is evaluated using both quantitative and qualitative scoring:

### RICE Scoring
- **Reach (R)**: Number of users or operations impacted per cycle (`> 0`).
- **Impact (I)**: Multiplier chosen strictly from `{3.0 (massive), 2.0 (high), 1.0 (medium), 0.5 (low), 0.25 (minimal)}`.
- **Confidence (C)**: Multiplier chosen strictly from `{1.0 (high), 0.8 (medium), 0.5 (low)}`.
- **Effort (E)**: Estimated effort in days (`> 0`).
- **Score Calculation**: `round((Reach * Impact * Confidence) / Effort, 2)`.

### MoSCoW Classification
- **Must Have**: Critical deliverables required for sprint success. Must not exceed 70% of total sprint capacity (`<= 3.5` days in a 5.0-day sprint).
- **Should Have**: Important enhancements delivered if time permits.
- **Could Have**: Desirable features that can be dropped without failing the sprint.
- **Won't Have**: Explicitly deferred to future sprints.

## Sprint Governance: Lock & Hotfix Bypass

- **Scope Freeze**: Once planned and agreed upon, lock the sprint with `/pm-sprint-lock`. This freezes the committed phases and prevents ad-hoc additions.
- **Unlock Audit**: Unlocking requires `/pm-sprint-unlock --confirm --reason "<reason>"` and appends an entry to the audit log in `sprint.md`.
- **Hotfix Bypass**: A locked sprint strictly rejects `sprints.py add-phase` unless `--hotfix` and a non-empty `--reason` are supplied, and the phase contains bug fix or hotfix work.
- **Formal Close**: End-of-sprint completion uses `/pm-sprint-close`. Carried-over tasks return to the backlog or are rolled forward into the next sprint.

## Stakeholder Documents & HTML Safety

Sprints generate three self-contained stakeholder documents:
1. **Business Requirements Document (BRD)**: Executive summary, problem statement, business goals, and ROI.
2. **Functional Requirements Document (FRD)**: Functional capabilities, acceptance criteria, and user interactions.
3. **Functional Specification Document (FSD)**: Technical architecture, data models, APIs, and security controls.

### Security Invariants
- **XSS Prevention**: Every dynamic field rendered into HTML must be escaped using `html.escape(value, quote=True)`.
- **Zero External CDNs**: All styles and scripts must be inline and self-contained; no remote fonts, icons, or stylesheets.
- **Zero Script Injection**: No `<script>` tags or inline `javascript:` URIs are permitted in generated deliverables.

## Collision-Free Multi-Agent Execution

When multiple agent sessions or panes execute sprint tasks concurrently (`/pm-sprint-implement`):
- **Work Selection**: Query `sprints.py next-work` to pull the highest-priority pending task.
- **Atomic Coordination**: Acquire an atomic claim via `coord.py claim` before touching any source file. If a race condition occurs (exit code 1), immediately retry `next-work`.
- **Worktree Isolation**: When another session is active, work in a dedicated task worktree (`<working-branch>--t-###`).
- **Single-Writer Plans**: Only the orchestrator session updates `impl.md` (via `specs.py set-status`) and `sprint.md`. Subagents implement code, run tests, and report results without editing plan documents.
