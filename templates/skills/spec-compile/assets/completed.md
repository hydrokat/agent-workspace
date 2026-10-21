# Compiled Specs — Completed

> Generated: {{date}}
> Scope: {{scope}}

<!-- if:phases -->
## Summary

{{count}} of {{total}} phases fully complete.

---

<!-- phase -->
## {{phase_dir}}: {{phase_title}}

**Tasks:** {{task_count}} tasks, all complete

### Tasks

| Task | Title | Done | Tested | Security Audited |
|------|-------|------|--------|-----------------|
{{task_rows}}

### Objectives Achieved

{{objectives}}

---

<!-- /phase -->
<!-- /if:phases -->
<!-- if:empty -->
No phases are fully complete within the selected scope.
<!-- /if:empty -->
<!-- if:no-scope -->
No matching specs were found within the selected scope.
<!-- /if:no-scope -->
