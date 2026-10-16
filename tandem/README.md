# tandem

Run several AI CLIs (`claude`, `agy`) side by side in one tmux session: **planners** on the left, **implementers** on the right, and a live status window underneath. Start it with `./run.sh` from your workspace (see the package README).

tandem only **launches** the official CLIs exactly as you would from a shell, with their documented flags. It never types into them, reads their output, proxies their traffic, or touches their credentials. Coordination happens only because agents choose to read and write shared files through the `agent-coordination` skill.

## Keys

Single source: `src/tandem/tmux/keys.py` (the hint bar, help popup, and this table are generated from it). tmux's prefix is disabled, so tandem consumes no other keys.

| Key | Action |
|---|---|
| `F1` | open this help |
| `F2` | add an agent |
| `F3` | previous agent in this column |
| `F4` | next agent in this column |
| `F5` | focus the status window |
| `F6` | open the Queries tab |
| `F7` | focus planners |
| `F8` | focus implementers |
| `F9` | close the focused agent (asks first) |
| `F10` | exit menu: save and exit, detach, update tandem, kill |
| `F11` | restart an exited agent |
| `F12` | resize mode: arrows resize the focused pane, = resets, Enter exits |

On a laptop keyboard, macOS may treat F1-F12 as media keys: hold `fn`, or enable "Use F1, F2, etc. keys as standard function keys". Mouse works too: click a pane to focus it, drag a border to resize, scroll to scroll. Keyboard resize: `F12`, then arrows (5 columns / 2 rows per press) on the focused pane, `=` resets to the default sizes, `Enter`/`Esc` leaves resize mode.

## Layout

```text
tmux -L tandem-<workspace hash>  (one socket per workspace, so projects run side by side; your ~/.tmux.conf is not loaded)
├── window "main":  planner column | implementer column
│                   status window (Textual): Status tab, Queries tab
└── hidden windows "stash-<role>-<n>": background agents, swapped into a column when you switch tabs
```

Terminals narrower than 100 columns stack the columns vertically, and the status window is kept to at most 40% of a short terminal (you can still drag it larger).

The Status tab's **Repositories** section lists the workspace and every codebase in `codebase-symlinks/`: one header per repo, then each branch, with `*` for the main checkout and `→ path` for the linked worktree a branch is active on (idle branches are listed after, capped at five). Rows shorten to the window width. Exited agents stay on screen with their exit code; `F11` restarts them.

## Save, resume and update

**Exit menu (`F10`):**

- **Save state and exit** writes `.agent-workspace/tandem-session.json` (owner-only JSON: each agent's CLI, role, model, tab number and working folder, the tab showing in each column, and pane proportions), then ends the session and its agents.
- **Detach** leaves everything running.
- **Update tandem and workspace** (below).
- **Kill session without saving** ends it and leaves any earlier saved file as it was.

The next `./run.sh` (or `tandem`) that finds a saved file asks **[R]esume / start [b]lank / [q]uit** (Enter resumes). Non-interactive: `./run.sh -- --resume` or `--fresh`; `--fresh` keeps the file until you save again. Resuming relaunches the same agents with the same models and folders. Conversations are **not** restored: each CLI keeps its own history (for example `claude --resume`). An agent whose CLI is no longer installed is skipped, and a deleted folder falls back to the workspace.

**Update without quitting (`F10` → Update, or `tandem update [--yes]`):** after you confirm, it saves the state, runs `npx --yes hydrokat/agent-workspace update --yes --no-link` (the same refresh as `agent-workspace update`: skills, agents, `run.sh`, tandem), `uv sync`s tandem's environment, and restarts only tandem's own windows (status, empty-column placeholders) and key bindings on the new code. Agents are not touched or restarted. If a step fails, nothing is restarted and tandem keeps running the old code. If the minimum dependency versions changed, it tells you to restart via `./run.sh`. Set `TANDEM_UPDATE_SPEC=owner/repo[#ref]` to update from a fork.

## Quota line

The second row of tmux's bottom bar shows subscription quota for the CLIs that have agents running, e.g. `Quota  claude: 5h 42% (resets 14:30) · 7d 12% (resets Mon 09:00)  │  agy: quota not available`. It refreshes every 10 seconds from local files.

- **Claude Code:** tandem starts each Claude session with `--settings` pointing the documented *statusline* at `tandem quota-feed`. Claude Code itself passes `rate_limits` (the 5-hour and weekly windows) to that command; tandem stores only the percentages and reset times under `<coordination>/quota/`. Your own statusline is not lost: the feed runs the one from your local, project or user settings on the same input and prints its output. Claude only supplies this for Pro/Max subscribers and after a session's first reply; otherwise the line says `quota not available`. An old reading is labelled `[as of HH:MM]`.
- **Antigravity (`agy`):** always `quota not available`. Its only quota view is the interactive `/usage` panel, and Google has not said that third parties may read its statusline data.
- **What tandem does not do:** read or type into a pane, read an OAuth token, or call a usage endpoint.

## Coordination

Agents and tandem share `.agent-workspace/coordination/` (override with `TANDEM_COORD_DIR`): `sessions/`, `claims/`, `queries/`. The schema and state machines are in `templates/skills/agent-coordination/references/protocol.md`. Questions an agent asks appear in the **Queries** tab; your answer is written to the query file and the agent picks it up at its next checkpoint. Nothing is typed into a pane.

## Development

```bash
cd tandem
uv sync
uv run pytest        # needs tmux for the integration tests (skipped when absent)
uv run mypy          # strict
uv run tandem --help
```

Minimum tool versions live in `min-versions.env` (read by `../run.sh` as data, never sourced, and by `tandem.versions`). Python modules:

| Module | Role |
|---|---|
| `adapters/` | Build exact argv for `claude` / `agy`; list models |
| `wizard/` | Launch and add-agent screens |
| `tmux/` | Allow-listed tmux client, layout, tabs, keymap |
| `coordination/` | Typed reader/answer-writer for the protocol files |
| `sources/`, `snapshot.py`, `status/` | Status data sources and the Textual UI |

`tests/test_tos_guard.py` fails the build if code could send keys to or capture output from an AI pane.
