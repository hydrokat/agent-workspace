# Compiled Spec Output Format

Both output files live in `specs/compiled/`. Create the directory if it does not exist (it will have been cleared in Step 4 of the workflow).

---

## `specs/compiled/completed.md`

Written for filter modes: `all`, `completed`, range, or single phase.

```markdown
# Compiled Specs — Completed

> Generated: YYYY-MM-DD
> Scope: [all phases | phases 001–010 | phase 005 | completed filter]

## Summary

X of Y phases fully complete.

---

## Phase XXX: [Phase Name]

**Tasks:** N tasks, all complete

### Tasks

| Task | Title | Done | Tested | Security Audited |
|------|-------|------|--------|-----------------|
| task-001 | [Title] | ✅ | ✅ | ✅ |
| task-002 | [Title] | ✅ | ✅ | ✅ |

### Objectives Achieved

[Brief summary derived from impl.md phase objectives]

---
```

**Empty state** (no complete phases in scope):

```markdown
# Compiled Specs — Completed

> Generated: YYYY-MM-DD
> Scope: [scope description]

No phases are fully complete within the selected scope.
```

---

## `specs/compiled/incomplete.md`

Written for filter modes: `all`, `incomplete`, range, or single phase.

```markdown
# Compiled Specs — Incomplete

> Generated: YYYY-MM-DD
> Scope: [all phases | phases 001–010 | phase 005 | incomplete filter]

## Summary

X of Y phases have incomplete tasks.

---

## Phase XXX: [Phase Name]

**Progress:** N/M tasks complete

### Task Status

| Task | Title | Done | Tested | Security Audited |
|------|-------|------|--------|-----------------|
| task-001 | [Title] | ✅ | ✅ | ✅ |
| task-002 | [Title] | ❌ | ✅ | ❌ |
| task-003 | [Title] | ❌ | ❌ | ❌ |

### Incomplete Tasks

#### task-002: [Title]

Missing:
- [ ] Done — objectives not fully checked
- [ ] Security Audited — no security audit section found

#### task-003: [Title]

Missing:
- [ ] Done — objectives not fully checked
- [ ] Tested — no testing section found
- [ ] Security Audited — no security audit section found

---
```

**Empty state** (all phases in scope are complete):

```markdown
# Compiled Specs — Incomplete

> Generated: YYYY-MM-DD
> Scope: [scope description]

All phases within the selected scope are fully complete. See completed.md.
```

---

## Scope Note

Always include the `> Scope:` line beneath the generated date. Use these values:

| Filter | Scope line |
|---|---|
| `all` or no arg | `Scope: all phases` |
| `completed` | `Scope: completed filter (all phases scanned)` |
| `incomplete` | `Scope: incomplete filter (all phases scanned)` |
| `001-010` range | `Scope: phases 001–010` |
| `005` single | `Scope: phase 005` |

---

## Ordering

- Sort phases numerically (phase-001 before phase-002).
- Sort tasks within a phase numerically.
- In `incomplete.md` task status tables, list incomplete tasks before complete tasks.
