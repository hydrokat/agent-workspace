# ToS-Safe Orchestration of AI CLIs

Rules for tools that run several official AI CLIs (`claude`, `agy`, ...) together. Some providers restrict or do not support wrapping their CLI inside third-party applications on consumer plans; stay on the safe side by never acting as a wrapper.

## Rules

1. **Spawn exactly as a user would.** Launch each CLI as an argv list with its documented flags. No proxying, no OAuth or token handling, no API calls on the CLI's behalf.
2. **Never type into or read from an AI pane.** Do not use `send-keys`, `capture-pane`, `pipe-pane`, or tmux buffers on those panes. Allowed on AI panes: create, move (`swap-pane`), focus, resize, kill, and set border options.
3. **No prompt routing.** No feature may forward user prompts into a CLI or aggregate several CLIs behind one chat box.
4. **Coordinate through files the agents choose to read.** Skills tell agents to read and write shared files at checkpoints; the orchestrator renders those files and writes answers to them.
5. **Enforce with tests, not discipline.** Keep a default-deny command allow-list in one client class, make the only `send-keys`/`capture-pane` helpers refuse anything not marked as the tool's own pane (fail closed), and add a source-grep test so the forbidden commands cannot appear elsewhere (`tandem/tests/test_tos_guard.py`).
6. **argv, never shell strings.** Validate model ids (`^[A-Za-z0-9][A-Za-z0-9._:-]*$`, no leading `-`) and pass them as separate argv elements. Pass only the environment the agent needs (`TANDEM_SESSION_ID`, `TANDEM_ROLE`, `TANDEM_CLI`, `TANDEM_COORD_DIR`).

7. **Resume through IDs, not contents.** To restore AI state, store only the conversation ID and reopen it with the CLI's documented resume flag. Get the ID from something the CLI hands you: an ID you assigned (`claude --session-id`), its documented statusline payload, or a log it writes to a path you passed (`agy --log-file`). Never read a transcript's contents; checking that a transcript file exists is enough to decide whether to resume.

## Consequences for UX

- No keystroke-injected answers: questions are answered in a separate window and picked up by the agent at its next checkpoint.
- Restart for an exited pane needs its own key (a plain letter would have to be forwarded to live panes).
- Panes carry their metadata as tmux user options (`@tandem_*`) so key-bound commands are stateless.

## Verify before trusting a provider rule

The no-wrapping rule for `agy` in p-001 came from the Antigravity FAQ as recorded in the plan. The quote was not re-verified during implementation, and a public forum thread shows the question is still being asked: https://discuss.ai.google.dev/t/is-invoking-the-official-antigravity-cli-agy-print-from-a-third-party-developer-tool-an-acceptable-use/175462. Re-read the provider's current terms before adding any feature that sits between the user and a CLI.
