# In Progress

Tracks what agents are currently working on so the user can see active work at a glance.

Rules:
- Every active task or spec must have an entry here, including the ongoing task and its next step.
- If the task belongs to a spec, indicate the spec name (e.g. `phase-002`).
- If the task does not belong to a spec, label it `adhoc`.
- Remove an entry as soon as its task is completed. This file reflects only active work, not history.

## Example

### phase-002 — Task 003: Add refresh token rotation
- **Ongoing:** Implementing rotation logic in `auth/service.js`.
- **Next step:** Add regression tests for expired refresh tokens.

### adhoc — Fix flaky CI timeout
- **Ongoing:** Investigating intermittent timeout in the integration test suite.
- **Next step:** Reproduce locally with increased logging.
