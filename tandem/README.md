# tandem

The runtime of tandem (agent-workspace): run several AI CLIs (`claude`, `agy`) side by side in one tmux session: **planners** on the left, **implementers** on the right, and a live status window underneath. Start it with `./run.sh` from your workspace (see the package README).

tandem only **launches** the official CLIs exactly as you would from a shell, with their documented flags. It never types into them, reads their output, proxies their traffic, or touches their credentials. Coordination happens only because agents choose to read and write shared files through the `agent-coordination` skill.

To resume conversations, tandem reads only two things the CLIs produce: the log `agy` writes to the `--log-file` tandem passes it (for the conversation ID), and whether a Claude transcript file with a given ID exists. It never reads a conversation's contents.

## Keys

Single source: `src/tmux/keys.rs` (the hint bar, help popup, and this table are generated from it). tmux's prefix is disabled, so tandem consumes no other keys. Every shortcut requires **at least 1 modifier key** (`Ctrl`, `Alt`, `Shift`) and supports multi-modifier chords. Shortcuts are OS-aware: they display with `Opt` on macOS (`Opt+h`, `Ctrl+Opt+n`, etc.) and `Alt` on Linux. On macOS terminals where Option does not send Meta, tandem also binds native macOS Option character keystrokes (`˙`, `ß`, `œ`, etc.) automatically. Custom shortcuts can be set via `TANDEM_KEYS="action=Shortcut,..."` (e.g. `TANDEM_KEYS="help=Opt+F1,quit=Ctrl+Opt+q"`).

| Key | Action |
|---|---|
| `Alt+h` | open this help |
| `Ctrl+Alt+n` | add an agent |
| `Alt+Up` | previous agent (sidebar order), shown alone |
| `Alt+Down` | next agent (sidebar order), shown alone |
| `Alt+s` | focus the status window |
| `Alt+q` | open the Queries tab |
| `Alt+1` | show last planner |
| `Alt+2` | show last implementer |
| `Ctrl+Alt+w` | close the focused agent (asks first) |
| `Ctrl+Alt+x` | exit menu: save and exit, detach, update tandem, discard |
| `Ctrl+Alt+r` | restart an exited agent |
| `Ctrl+Alt+z` | resize mode: arrows resize the focused pane, = resets, Enter exits |
| `Alt+b` | focus the sidebar (popup when hidden) |
| `Alt+,` | open Settings (grid, sidebar, Reset UI) |

Mouse works everywhere:

- **Click an agent in the sidebar** to show it alone in the main area.
- **Shift+click or Opt/Alt+click** an agent (or press `Space` in the sidebar) to add or toggle it in the multi-agent grid.
- **Click a status-bar hint** to press its keyboard shortcut.
- **Double-click any inner border** to equalize sibling pane splits; double-click a structural border to reset sidebar width or status height.
- **Right-click any pane** to open a context menu: focus, restart (if exited), close, new agent, status/queries, resize, help.
- **Click a pane** to focus it; **drag a border** to resize; **scroll** to scroll.
- **Click the update banner** in the Status window to open the "What's new" popup; click **[×]** to dismiss it.
- When an in-session update offer is shown, click **[Update now]**, **[Later]**, or **[Skip this version]** in the banner.

Keyboard resize: `Ctrl+Alt+z`, then arrows (5 columns / 2 rows per press) on the focused pane, `e` equalizes the split, `=` resets to the default sizes, `Enter`/`Esc` leaves resize mode.

## Layout

```text
tmux -L tandem-<workspace hash>  (one socket per workspace, so projects run side by side; your ~/.tmux.conf is not loaded)
├── window "main":  agent sidebar | main multi-agent grid (responsive breakpoints)
│                   status window (ratatui): Status, Sprint, Spec, Task, Queries, Web, Settings tabs
└── hidden windows "stash-<role>-<n>": background agents, joined into grid when selected
```

The layout adapts across responsive breakpoints (Wide ≥120 cols, Medium 80–119 cols, Narrow <80 cols, Short <20 rows). The status window is kept to at most 40% of a short terminal (you can still drag it larger).

### Status window tabs

The status window provides six tabs:
- **Status:** Agent sessions, live claims, repository branches and worktrees, and active sprint summary.
- **Sprint:** Full ongoing sprint details including capacity vs committed effort, task progress bar, days left, and phase progress rows sorted in MoSCoW order.
- **Spec:** Ongoing spec phases with branch and merge target strategy, phase progress bar, and the task list ordered by task id with `▶` marking In Progress tasks. Shows `next up: ` when no task is in progress.
- **Task:** Detailed ongoing task inspection with objective checklist progress, owning session and pane id, heartbeat age, branch, worktree, claimed paths, and next step from `in-progress.md`. Press `Alt+g` to select the owning agent's tmux pane.
- **Queries:** Questions queued by agents via `coord.py ask`.
- **Web:** Remote Web (`tandem-web`) on/off with first-time dependency consent, live status, bind IP and source selector, port, `ttyd` terminal toggle, masked auth token (show/hide, regenerate with confirmation), and a full QR code overlay for connecting a phone. Changing any setting while running restarts the server; start errors appear in the pane. The QR needs about 37x19 cells; a smaller pane tells you the size it needs.
- **Settings:** UI layout options (grid columns, rows per page, fill order, sidebar mode) and interactive Reset UI.

**Navigation:**
- Cycle tabs with `Tab` and `Shift+Tab`.
- Jump directly to a tab with number keys `1-7`, click a tab header, or press `Alt+,` for Settings.
- Scroll tab content using `↑↓`, `PgUp`, `PgDn`, `Home`, `End`, and the mouse wheel.
- Focus the status window globally with `Alt+s`, jump to Queries with `Alt+q`, or jump to Settings with `Alt+,`.

**What "ongoing" means:**
- *Ongoing sprint:* The active sprint resolved under `sprints/` whose status is `open` or `locked`.
- *Ongoing spec:* Every phase with at least one task `In Progress` or `In Review`, an active claim, or an entry in `specs/in-progress.md`. When none exist, the first incomplete phase of the active sprint in MoSCoW order is shown labelled "next up".
- *Ongoing task:* Every task currently marked `In Progress`, joined with its coordination claim and `in-progress.md` entry. A claim on a non-in-progress task is flagged `status mismatch`, and an unclaimed in-progress task is flagged `unclaimed`.

The Status tab's **Repositories** section lists the workspace and every codebase in `codebase-symlinks/`: one header per repo, then each branch, with `*` for the main checkout and `→ path` for the linked worktree a branch is active on (idle branches are listed after, capped at five). Rows shorten to the window width. Exited agents stay on screen with their exit code; `Ctrl+Alt+r` restarts them and reopens their conversation when the CLI has one to reopen.

The Status tab also shows the **active sprint** when one exists: the last sprint under `sprints/` whose `sprint.md` says `open` or `locked` (the same rule as `sprints.py resolve`). A headline above "In progress" gives the sprint id and title, task progress across its phases, days left (`3 days left`, `ends today`, `overdue 2d`), the remaining Must Have tasks, `locked`, and `OVERLOADED 7.5/5.0d` when the committed effort is above the capacity. A **Sprint** section lists the dates, the capacity and one row per phase (MoSCoW, progress bar, done/total), and the summary line gets a short `s-003 4/14` token so the sprint stays visible in compact mode. Nothing is shown without an active sprint. tandem reads `sprint.md` and the phase specs directly, so no script runs on the refresh loop; committed effort comes from the scope table rows, as in `sprints.py`.

## Runtime

tandem is a native Rust binary (ratatui + crossterm) with zero external runtime dependencies, so launches are instant. `./run.sh` checks for and downloads the verified release binary for your platform; if it is unavailable it builds from the shipped sources with cargo.

### Supported platforms
- `linux-x86_64` and `linux-arm64` (static musl binaries, no glibc requirement)
- `darwin-arm64` and `darwin-x86_64` (macOS 13+)
- `windows-x86_64` (experimental support under MSYS2 with tmux, or WSL running the Linux binary)

### Binary cache and verification
Binaries are fetched from GitHub Releases, verified against `SHA256SUMS`, and cached under `${XDG_STATE_HOME:-~/.local/state}/tandem/bin/<version>-<platform>/` with the active version pointed to by `tandem/bin/current`. Stale versions are pruned.

### Source fallback
When the binary is unavailable (not downloadable, checksum mismatch, blocked by Gatekeeper or `noexec`, wrong version, or no prebuilt binary for the platform), `run.sh` first **asks whether to build tandem on this machine** (it says why and where the build goes; declining exits with code 2, non-interactive runs without `--yes` exit with 6). It never compiles without that yes, and a healthy earlier build of the same version is reused without asking. After you consent it needs `cargo` >= `TANDEM_MIN_CARGO` (it offers to install it through rustup, showing the command first) and runs `cargo build --release --locked --manifest-path <crate>/Cargo.toml` with its target directory under `${XDG_STATE_HOME:-~/.local/state}/tandem/build-<workspace>/`. The result must pass the same `--version` check before it is launched. Each workspace builds in its own directory, so parallel workspaces never clobber each other.

### Environment variables and flags
- `TANDEM_RUNTIME=auto|binary|source`:
  - `auto` (default): tries the binary first; builds from source if unavailable.
  - `binary`: requires the binary; fails with exit code 4 if unavailable or checksum fails.
  - `source`: skips the binary and builds with cargo.
- `TANDEM_RELEASE_BASE`: Base URL for binary archives and `SHA256SUMS` (default: `https://github.com/hydrokat/agent-workspace/releases/download`).
- `./run.sh --fetch-runtime`: Downloads and verifies the binary runtime non-interactively without launching tandem (exit 0 on success, exit 4 on failure).

`cargo` is only required for the source fallback and for development; the binary runs standalone.

## Save, resume and update

tandem keeps `.agent-workspace/tandem-session.json` (owner-only JSON) up to date while it runs. The file holds each agent's CLI, role, model, tab number, working folder and **conversation ID**, plus the tab showing in each column and the pane proportions. tandem autosaves it on start, whenever an agent is added or closed, and every 15 seconds from the status window. After a crash, a reboot or a closed terminal window, the next launch can pick up where you were.

**Exit menu (`Ctrl+Alt+x`):**

- **Save state and exit** saves (marked as a clean exit), then ends the session and its agents.
- **Detach** leaves everything running; the next launch re-attaches.
- **Update tandem and workspace** (below).
- **Quit and discard saved state** deletes the saved file and ends the session, so the next launch starts fresh without asking.

The next `./run.sh` (or `tandem`) that finds a saved file asks **[R]esume / start [b]lank (new conversations) / [q]uit** (Enter resumes). If the last save was an autosave, the prompt says tandem did not exit cleanly. Non-interactive: `./run.sh -- --resume` or `--fresh`. A blank start replaces the saved file once the new session autosaves; `--fresh` deletes it immediately. An agent whose CLI is no longer installed is skipped, and a deleted folder falls back to the workspace.

**Per-workspace builds:** source builds live under `${XDG_STATE_HOME:-~/.local/state}/tandem/build-<slug>`, one per workspace, so parallel workspaces never clobber each other. Leftover `venv` and `venv-*` folders from retired runtimes are removed on the first normal launch.

**AI state (conversations).** Resuming relaunches each agent with its model and folder and reopens its own conversation, using only documented CLI flags:

| CLI | New agent | Resume | Where tandem learns the current conversation |
|---|---|---|---|
| `claude` | `--session-id <uuid>` (tandem picks the UUID) | `--resume <uuid>` | the `session_id` Claude Code passes to the statusline command tandem already sets, so `/clear` or `/resume` inside the pane is followed |
| `agy` | `--log-file <coordination>/conversations/agy-<role>-<n>.log` | `--conversation <id>` | the last `Created/Streaming conversation <id>` line in that log (best effort: if the log format changes, the agent starts a new conversation) |

The conversations themselves stay in each CLI's own history; tandem stores only the IDs (in `tandem-session.json` and `<coordination>/conversations/`). Claude is only asked to `--resume` when its transcript exists (`$CLAUDE_CONFIG_DIR` or `~/.claude`, `projects/*/<uuid>.jsonl`); a session that never got a message starts fresh under the same ID. A Claude agent whose folder was deleted starts a new conversation, because Claude keeps conversations per folder. `Ctrl+Alt+r` on an exited agent reopens its conversation the same way.

**Update without quitting (`Ctrl+Alt+x` → Update, or `tandem update [--yes]`):** after you confirm, it saves the state, runs `npx --yes hydrokat/agent-workspace update --yes --no-link` (the same refresh as `agent-workspace update`: skills, agents, `run.sh`, tandem), refreshes the binary runtime with `./run.sh --fetch-runtime`, and restarts only tandem's own windows (status, empty-column placeholders) and key bindings on the new code. The same `update` also updates the installed AI CLIs (`claude update`, `agy update`, codex through its package manager) without asking, because you confirmed the update. Agents are not restarted automatically. If a step fails, nothing is restarted and tandem keeps running the old code. If the minimum dependency versions changed, it tells you to restart via `./run.sh`. Set `TANDEM_UPDATE_SPEC=owner/repo[#ref]` to update from a fork.

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
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test           # needs tmux for the integration tests (skipped when absent)
cargo run -- --help
```

Minimum tool versions live in `min-versions.env` (read by `../run.sh` as data, never sourced, and by `src/versions.rs`). The pytest suites for the template skill scripts live in `../test/skills`. Rust modules:

| Module | Role |
|---|---|
| `adapters/` | Build exact argv for `claude` / `agy`; list models |
| `wizard/` | Launch and add-agent screens |
| `tmux/` | Allow-listed tmux client, layout, tabs, keymap |
| `coordination/` | Typed reader/answer-writer for the protocol files |
| `sources/`, `snapshot.rs`, `status/` | Status data sources and the ratatui UI |
| `state.rs`, `conversations.rs`, `quota.rs` | Saved sessions, conversation IDs, quota feed |
| `updater.rs`, `update_check.rs`, `notice.rs` | In-session update, version check, what's-new notice |

The golden fixtures in `tests/fixtures/parity/` were captured from legacy tandem 1.5.0 and are frozen.

`tests/tos_guard.rs` fails the build if code could send keys to or capture output from an AI pane.
