# Task Completion Criteria

## Detection Patterns

Tasks are markdown files (`task-YYY.md`) under `specs/phase-XXX/`. Each criterion is detected by searching the file for the patterns below.

---

### 1. Done

A task is **done** if **any** of these are true:

- All checkboxes under `## Objectives` are checked (`- [x]`)
- A `## Status` section contains `- [x]` on any line
- A line matching `Status: Done`, `Status: Complete`, or `Status: ✅` (case-insensitive) exists

If `## Objectives` is present but has unchecked items (`- [ ]`), the task is NOT done regardless of other signals.

---

### 2. Tested

A task is **tested** if **any** of these are true:

- A section heading matches `## Testing`, `## Test Results`, or `## Tests` (case-insensitive), and it contains at least one `- [x]` item
- A line matching `Tested: Yes`, `Tested: ✅`, or `Testing: Passed` (case-insensitive) exists
- A `## Status` section contains a line like `- [x] Tested`

---

### 3. Security Audited

A task is **security audited** if **any** of these are true:

- A section heading matches `## Security`, `## Security Audit`, or `## Security Review` (case-insensitive), and it contains at least one `- [x]` item
- A line matching `Security Audit: Passed`, `Security: ✅`, or `Security Reviewed: Yes` (case-insensitive) exists
- A `## Status` section contains a line like `- [x] Security Audited` or `- [x] Security Reviewed`

---

## Missing Criteria Reporting

When a task is incomplete, note which criteria are missing. Use this format in `incomplete.md`:

```
- [ ] Done
- [x] Tested
- [ ] Security Audited
```

## Handling Ambiguity

- If a section exists but is empty, treat the criterion as **not met**.
- If a task file cannot be read, mark all criteria as **unknown** and flag the file path in the output.
- If a phase has no task files (only `impl.md`), treat the phase as **incomplete**.
