# Compiled Specs — Incomplete

> Generated: {{date}}
> Scope: {{scope}}

<!-- if:phases -->
## Summary

{{count}} of {{total}} phases have incomplete tasks.

---

<!-- phase -->
## {{phase_dir}}: {{phase_title}}

**Progress:** {{complete_tasks}}/{{task_count}} tasks complete

### Task Status

| Task | Title | Done | Tested | Security Audited |
|------|-------|------|--------|-----------------|
{{task_rows}}

### Incomplete Tasks

{{incomplete_tasks}}

---

<!-- /phase -->
<!-- /if:phases -->
<!-- if:empty -->
All phases within the selected scope are fully complete. See completed.md.
<!-- /if:empty -->
<!-- if:no-scope -->
No matching specs were found within the selected scope.
<!-- /if:no-scope -->
