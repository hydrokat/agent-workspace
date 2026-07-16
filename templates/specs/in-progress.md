# In Progress

Tracks what agents are currently working on so the user can see active work at a glance and to prevent agent collisions.

Rules:
- Every active task or spec must have an entry here, including the ongoing task and its next step.
- If the task belongs to a spec, indicate the spec name (e.g. `phase-002`).
- If the task does not belong to a spec, label it `adhoc`.
- **Multi-Agent Tracking:** Every entry MUST explicitly declare the agent role, conversation ID, and the working branch/worktree.
- Remove an entry as soon as its task is completed. This file reflects only active work, not history.

## Example

### phase-002 — Task 003: Add refresh token rotation
- **Agent:** `@senior-backend-dev` (ID: `abc-1234`)
- **Location:** Branch `feat/refresh-rotation`
- **Ongoing:** Implementing rotation logic in `auth/service.js`.
- **Next step:** Add regression tests for expired refresh tokens.

### adhoc — Fix flaky CI timeout
- **Agent:** `@qa-engineer` (ID: `def-5678`)
- **Location:** Worktree `./.tmp/ci-debug`
- **Ongoing:** Investigating intermittent timeout in the integration test suite.
- **Next step:** Reproduce locally with increased logging.
