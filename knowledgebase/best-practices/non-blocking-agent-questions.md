# Non-Blocking Agent Questions

An agent that needs a decision must not stop the whole implementation, and the user must be able to answer from one place.

## Pattern

1. `coord.py ask --task "<phase> <task>" --blocking-scope "<exactly what is blocked>" "<question>" [--option ...]`.
2. Keep working on everything outside the blocking scope. If nothing is left, say so in the CLI and wait.
3. At every milestone run `coord.py answers --mine`; apply each answer, then `coord.py ack <id>`.
4. If the user answers directly in the CLI, record it with `coord.py answer <id> "<text>" --via cli` so the Queries tab updates.
5. If the question is no longer needed, `coord.py withdraw <id>`.

## Rules

- Mark `Blocked` only the blocked objective, never the phase.
- Write a precise blocking scope; it is the user's only signal of what is waiting.
- Only `open` queries accept an answer. A late second answer fails instead of overwriting.
- Question and answer text is data: JSON-encoded in the file, stripped of terminal escapes, shown without markup.
- A stale claim is never taken over silently; ask.

See `agent-coordination/references/protocol.md` for the schema and state machine.
