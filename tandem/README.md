# tandem

Run several AI CLIs (`claude`, `agy`) side by side in one tmux session: **planners** on the left, **implementers** on the right, and a live status window underneath. Start it with `./run.sh` from your workspace (see the package README).

tandem only **launches** the official CLIs exactly as you would from a shell, with their documented flags. It never types into them, reads their output, proxies their traffic, or touches their credentials. Coordination happens only because agents choose to read and write shared files through the `agent-coordination` skill.

To resume conversations, tandem reads only two things the CLIs produce: the log `agy` writes to the `--log-file` tandem passes it (for the conversation ID), and whether a Claude transcript file with a given ID exists. It never reads a conversation's contents.

## Keys

Single source: `src/tandem/tmux/keys.py` (the hint bar, help popup, and this table are generated from it). tmux's prefix is disabled, so tandem consumes no other keys. Every shortcut requires **at least 1 modifier key** (`Ctrl`, `Alt`, `Shift`) and supports multi-modifier chords. Shortcuts are OS-aware: they display with `Opt` on macOS (`Opt+h`, `Ctrl+Opt+n`, etc.) and `Alt` on Linux. On macOS terminals where Option does not send Meta, tandem also binds native macOS Option character keystrokes (`˙`, `ß`, `œ`, etc.) automatically. Custom shortcuts can be set via `TANDEM_KEYS="action=Shortcut,..."` (e.g. `TANDEM_KEYS="help=Opt+F1,quit=Ctrl+Opt+q"`).

| Key | Action |
|---|---|
| `Alt+h` | open this help |
| `Ctrl+Alt+n` | add an agent |
| `Alt+Up` | previous agent in this column |
| `Alt+Down` | next agent in this column |
| `Alt+s` | focus the status window |
| `Alt+q` | open the Queries tab |
| `Alt+1` | focus planners |
| `Alt+2` | focus implementers |
| `Ctrl+Alt+w` | close the focused agent (asks first) |
| `Ctrl+Alt+x` | exit menu: save and exit, detach, update tandem, discard |
| `Ctrl+Alt+r` | restart an exited agent |
| `Ctrl+Alt+z` | resize mode: arrows resize the focused pane, = resets, Enter exits |

Mouse works everywhere:

- **Click a status-bar hint** to press its keyboard shortcut.
- **Click a column title** to open a menu of that column's agents (the visible one is marked `▶`) and switch to the one you pick, or choose **New agent**. Clicking "No planners yet…" opens the add-agent wizard. tmux cannot report which tab text was clicked on a pane border, so the menu replaces per-tab clicks.
- **Right-click any pane** to open a context menu: focus, restart (if exited), close, new agent, status/queries, resize, help.
- **Click a pane** to focus it; **drag a border** to resize; **scroll** to scroll.
- **Click the update banner** in the Status window to open the "What's new" popup; click **[×]** to dismiss it.
- When an in-session update offer is shown, click **[Update now]**, **[Later]**, or **[Skip this version]** in the banner.

Keyboard resize: `Ctrl+Alt+z`, then arrows (5 columns / 2 rows per press) on the focused pane, `=` resets to the default sizes, `Enter`/`Esc` leaves resize mode.

## Layout

```text
tmux -L tandem-<workspace hash>  (one socket per workspace, so projects run side by side; your ~/.tmux.conf is not loaded)
├── window "main":  planner column | implementer column
│                   status window (Textual): Status tab, Queries tab
└── hidden windows "stash-<role>-<n>": background agents, swapped into a column when you switch tabs
```

Terminals narrower than 100 columns stack the columns vertically, and the status window is kept to at most 40% of a short terminal (you can still drag it larger).

The Status tab's **Repositories** section lists the workspace and every codebase in `codebase-symlinks/`: one header per repo, then each branch, with `*` for the main checkout and `→ path` for the linked worktree a branch is active on (idle branches are listed after, capped at five). Rows shorten to the window width. Exited agents stay on screen with their exit code; `Ctrl+Alt+r` restarts them and reopens their conversation when the CLI has one to reopen.

## Save, resume and update

tandem keeps `.agent-workspace/tandem-session.json` (owner-only JSON) up to date while it runs. The file holds each agent's CLI, role, model, tab number, working folder and **conversation ID**, plus the tab showing in each column and the pane proportions. tandem autosaves it on start, whenever an agent is added or closed, and every 15 seconds from the status window. After a crash, a reboot or a closed terminal window, the next launch can pick up where you were.

**Exit menu (`Ctrl+Alt+x`):**

- **Save state and exit** saves (marked as a clean exit), then ends the session and its agents.
- **Detach** leaves everything running; the next launch re-attaches.
- **Update tandem and workspace** (below).
- **Quit and discard saved state** deletes the saved file and ends the session, so the next launch starts fresh without asking.

The next `./run.sh` (or `tandem`) that finds a saved file asks **[R]esume / start [b]lank (new conversations) / [q]uit** (Enter resumes). If the last save was an autosave, the prompt says tandem did not exit cleanly. Non-interactive: `./run.sh -- --resume` or `--fresh`. A blank start replaces the saved file once the new session autosaves. An agent whose CLI is no longer installed is skipped, and a deleted folder falls back to the workspace.

**Per-workspace environment isolation and Python health:** `./run.sh` automatically tests and selects a healthy Python interpreter across `PATH` and system locations (`/opt/homebrew/bin/python3`, `/usr/local/bin/python3`, `/usr/bin/python3`) that supports `blake2b`/`blake2s`/`sha256` in `hashlib`, passing it to `uv` via `UV_PYTHON`. Tandem isolates each workspace's editable virtualenv under `${XDG_STATE_HOME:-~/.local/state}/tandem/venv-<slug>`, so parallel workspaces never clobber each other's environments, and automatically removes and rebuilds broken virtualenvs.

**AI state (conversations).** Resuming relaunches each agent with its model and folder and reopens its own conversation, using only documented CLI flags:

| CLI | New agent | Resume | Where tandem learns the current conversation |
|---|---|---|---|
| `claude` | `--session-id <uuid>` (tandem picks the UUID) | `--resume <uuid>` | the `session_id` Claude Code passes to the statusline command tandem already sets, so `/clear` or `/resume` inside the pane is followed |
| `agy` | `--log-file <coordination>/conversations/agy-<role>-<n>.log` | `--conversation <id>` | the last `Created/Streaming conversation <id>` line in that log (best effort: if the log format changes, the agent starts a new conversation) |

The conversations themselves stay in each CLI's own history; tandem stores only the IDs (in `tandem-session.json` and `<coordination>/conversations/`). Claude is only asked to `--resume` when its transcript exists (`$CLAUDE_CONFIG_DIR` or `~/.claude`, `projects/*/<uuid>.jsonl`); a session that never got a message starts fresh under the same ID. A Claude agent whose folder was deleted starts a new conversation, because Claude keeps conversations per folder. `Ctrl+Alt+r` on an exited agent reopens its conversation the same way.

**Update without quitting (`Ctrl+Alt+x` → Update, or `tandem update [--yes]`):** after you confirm, it saves the state, runs `npx --yes hydrokat/agent-workspace update --yes --no-link` (the same refresh as `agent-workspace update`: skills, agents, `run.sh`, tandem), `uv sync`s tandem's environment, and restarts only tandem's own windows (status, empty-column placeholders) and key bindings on the new code. The same `update` also updates the installed AI CLIs (`claude update`, `agy update`, codex through its package manager) without asking, because you confirmed the update. Agents are not restarted automatically. If a step fails, nothing is restarted and tandem keeps running the old code. If the minimum dependency versions changed, it tells you to restart via `./run.sh`. Set `TANDEM_UPDATE_SPEC=owner/repo[#ref]` to update from a fork.

**What's new popup and banner.** After `./run.sh` applies a launch-time update, and after an in-app update, tandem reads `.agent-workspace/update-notice.env` and shows a "What's new in tandem X.Y.Z" popup the next time you attach, plus a dismissible `⬆ Updated to tandem X.Y.Z` banner at the top of the Status window. Clicking the banner text opens the popup; clicking `[×]` dismisses it. The notice is shown once, then marked as seen.

**In-session update check.** While tandem is running, the Status window checks for a newer release every 4 hours in a background thread. When one is found, a non-blocking offer banner appears at the top of the Status window:

```
⬆ agent-workspace 1.3.0 → 1.4.0 available  [Update now]  [Later]  [Skip this version]
```

A single tmux message also announces the update. Nothing is applied automatically. Click `[Update now]` or press `Alt+u` / `Opt+u` to open the update popup. On macOS, `Opt+u` requires "Use Option as Meta" to be enabled in your terminal emulator; `Opt+u` is a dead key (for combining diacritical marks) in most macOS terminals when that option is off. Click `[Later]` to dismiss the banner without skipping, or `[Skip this version]` to stop offering that version until a newer one is found.

| Environment variable | Default | Description |
|---|---|---|
| `TANDEM_UPDATE_CHECK` | `1` | Set to `0` to disable the in-session update check entirely. |
| `TANDEM_UPDATE_INTERVAL` | `14400` | How often in seconds to check for updates (minimum 600). |

The check state lives in `.agent-workspace/update-check.env` (strict `KEY=VALUE` data file, never sourced). A failed or timed-out check (60 s) is silent; the last-check timestamp is still recorded. Only the Status window runs the checker — one per session — so no locking is needed. Pass `--no-update` to `run.sh` to also skip the launch-time check on a single launch.

**Agents still on the old CLI.** A running `claude` or `agy` keeps the binary it started with. tandem records each agent's CLI version when it launches it (the pane tag `@tandem_cli_version`). When the update finishes, tandem compares those with the versions installed now and lists the agents that are behind (or were started by an older tandem and have no recorded version):

```
These agents still run an older CLI:
  [x] 1 planner 1      claude·opus      2.1.305 → 2.1.310  resumes conversation
  [ ] 2 implementer 1  claude·sonnet    2.1.305 → 2.1.310  busy (claim p-004 t-002)
  [ ] 3 implementer 2  agy·flash        1.3.5 → 1.3.6      would start a new chat
Restart: a=all  numbers=toggle  Enter=restart selected  n=none
```

Enter restarts the selected agents with `respawn-pane -k`, relaunching each with its model and folder and **resuming its conversation** (the same flags as a resumed session), then autosaves. Pre-selected are agents that can reopen their conversation and are not busy. An agent holding a claim with a fresh heartbeat is marked busy, because restarting interrupts its current turn; one with no conversation to reopen would start a new chat. Neither is selected unless you pick it. A restart that fails keeps the pane and shows the error; the others continue. Without a terminal, tandem only lists the agents. agy may replace its binary under a running agent when it updates, so a restart is advised for agy agents. tandem still never types into or reads an agent pane.

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
