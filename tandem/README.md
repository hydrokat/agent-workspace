# tandem

The runtime of tandem (agent-workspace): run several AI CLIs (`claude`, `agy`, `opencode`) side by side in one tmux session: **planners** on the left, **implementers** on the right, and a live status window underneath. Start it with `./run.sh` from your workspace (see the package README).

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
| `Alt+f` | open the Factory tab (Factory Mode sessions only) |

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
- **Web:** Remote Web (`tandem-web`) on/off with first-time dependency consent, live status, bind IP and source selector, port, `ttyd` terminal toggle, masked auth token (show/hide, regenerate with confirmation), and a full QR code overlay for connecting a phone. Changing any setting while running restarts the server; start errors appear in the pane. Enabling checks that `bun` runs on this host first; if it is missing you choose between *Let tandem install* (a generated, reviewable script that never uses `sudo`; progress shows in the pane) and *I'll install it myself* (the commands are shown). The QR needs about 37x19 cells; a smaller pane tells you the size it needs.
- **Settings:** UI layout options (grid columns, rows per page, fill order, sidebar mode), Quit button (`[ Quit… ]` opens the exit menu), and interactive Reset UI.

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
| `opencode` | none (always a fresh session) | not yet | none. `opencode --session <id>` exists, but the id shape is unverified and learning it would need a process per autosave, so tandem does not resume opencode agents (see `knowledgebase/context-history/opencode-cli-surface.md` in the agent-workspace repo) |

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

The second row of tmux's bottom bar shows subscription quota for the CLIs that have agents running, e.g. `Quota  claude: 5h 42% (resets 14:30) · 7d 12% (resets Mon 09:00)  │  agy: quota not available  │  opencode: quota not available`. It refreshes every 10 seconds from local files.

- **Claude Code:** tandem starts each Claude session with `--settings` pointing the documented *statusline* at `tandem quota-feed`. Claude Code itself passes `rate_limits` (the 5-hour and weekly windows) to that command; tandem stores only the percentages and reset times under `<coordination>/quota/`. Your own statusline is not lost: the feed runs the one from your local, project or user settings on the same input and prints its output. Claude only supplies this for Pro/Max subscribers and after a session's first reply; otherwise the line says `quota not available`. An old reading is labelled `[as of HH:MM]`.
- **Antigravity (`agy`):** always `quota not available`. Its only quota view is the interactive `/usage` panel, and Google has not said that third parties may read its statusline data.
- **opencode:** always `quota not available`. tandem does not read opencode's usage.
- **What tandem does not do:** read or type into a pane, read an OAuth token, or call a usage endpoint.

## Coordination

Agents and tandem share `.agent-workspace/coordination/` (override with `TANDEM_COORD_DIR`): `sessions/`, `claims/`, `queries/`. The schema and state machines are in `templates/skills/agent-coordination/references/protocol.md`. Questions an agent asks appear in the **Queries** tab; your answer is written to the query file and the agent picks it up at its next checkpoint. Nothing is typed into a pane.

## Factory Mode (`./run.sh --factory`)

Start Factory Mode with `./run.sh --factory` (or `tandem --factory`). Normal and Factory Mode sessions cannot share one workspace at the same time:

- Starting in Factory Mode while a normal session runs exits with code `2`: `A normal tandem session is running in this workspace. Quit it first (Settings → Quit), then run ./run.sh --factory.`
- Starting in normal mode while a Factory Mode session runs exits with code `2`: `A Factory Mode session is running in this workspace. Quit it first, or run ./run.sh --factory to attach.`
- Factory sessions save to `.agent-workspace/tandem-session-factory.json` instead of `tandem-session.json`.
- In Factory Mode, the Status window adds an eighth **Factory** tab (`Alt+f` / `8`) showing a read-only ideas board, and `tandem snapshot --json` includes a `factory` block (omitted in normal mode).

### `tandem factory` commands

| Command | Purpose |
|---|---|
| `tandem factory status --json` | Report `enabled`, idea counts by state, settings, and log status |
| `tandem factory idea new --title T --text X [--text-file PATH] [--owner O] --json` | Create a new idea in Inbox |
| `tandem factory idea start --cli <claude\|agy> --model M [--title T] --json` | Create an idea and launch its bound Orchestrator pane |
| `tandem factory idea resume ID --json` | Reopen or focus a non-terminal idea's Orchestrator pane |
| `tandem factory idea describe ID --title T --text-file F --json` | Set title and description (moves Inbox → Ideation) |
| `tandem factory idea assume ID --text T --json` | Record an assumption (up to 30, 500 chars each) |
| `tandem factory idea select-direction ID --json` | Advance Ideation → Planning (Gate 1: requires non-infeasible feasibility study) |
| `tandem factory idea ready ID --confirm ID --json` | Freeze `plan-snapshot.md` and mark `ready_for_development` (Gate 2, user-only; `--via` removed) |
| `tandem factory idea revoke-ready ID --json` | Revoke `ready_for_development` back to Planning before a run exists (user-only) |
| `tandem factory idea list [--state S] --json` | List ideas sorted by `updated` descending |
| `tandem factory idea show ID --json` | Show idea, events, `questions`, and `plan_snapshot` verification |
| `tandem factory artifact put ID --kind <feasibility\|plan> --file F [--verdict V] [--risk R] [--workflow W] --json` | Store versioned Markdown artifact (<= 256 KiB, UTF-8, no symlinks/control chars) |
| `tandem factory artifact show ID --kind <feasibility\|plan> [--version N]` | Print a stored artifact |
| `tandem factory question check ID --json` | Check open-question ceiling (exits `3` when >= 5 open questions) |
| `tandem factory events [--since SEQ] [--idea ID] --json` | Read event log entries |
| `tandem factory rebuild --json` | Rebuild materialized idea and run files from the event log |
| `tandem factory workflow validate <NAME\|PATH> [--json]` | Validate a workflow YAML file against factory settings |
| `tandem factory workflow list [--json]` | List workflows in `<workspace>/workflows/` with validity |
| `tandem factory workflow default` | Print the embedded `default-delivery.yaml` to stdout |
| `tandem factory run create <IDEA> [--workflow NAME] [--deploy] --json` | Create and queue a run from a verified plan snapshot (user-only) |
| `tandem factory run show <RUN> --json` | Show a run and its stages |
| `tandem factory run list [--idea ID] --json` | List runs sorted by creation time descending |
| `tandem factory run withdraw <RUN> --confirm <RUN> --json` | Withdraw a queued run before any stage starts (user-only) |
| `tandem factory run simulate [IDEA] [--workflow NAME] [--ui-changed] [--deploy] [--approve-deploy] [--fail STAGE[:N]] [--retries N] [--json]` | Dry-run a workflow in memory (writes nothing, spawns nothing) |
| `tandem factory run target <RUN> <NAME> [--json]` | Select the target repository when multiple git codebases exist |
| `tandem factory run pause <RUN> [--now] [--json]` | Pause a running or queued run (`--now` stops active jobs immediately) |
| `tandem factory run resume <RUN> [--json]` | Resume a paused run from the last valid stage |
| `tandem factory run retry <RUN> <STAGE> --hypothesis TEXT [--json]` | Grant one more retry attempt on a paused stage with a new hypothesis |
| `tandem factory run cancel <RUN> --confirm <RUN> [--json]` | Cancel an active run while keeping its worktree and branch |
| `tandem factory run cleanup <RUN> --confirm <RUN> [--json]` | Remove a completed, cancelled, or withdrawn run's clean worktree |
| `tandem factory run log <RUN> <JOB> [--tail N] [--json]` | Tail a job's `log.txt` (default last `65536` bytes) |
| `tandem factory run evidence <RUN> [--show ID] [--json]` | List or inspect recorded evidence items for a run |
| `tandem factory run prs <RUN> [--refresh] [--json]` | List or refresh pull request status for a run |
| `tandem factory run export-specs <RUN> [--json]` | Copy `plan` stage specs into workspace `specs/` under the next free phase |
| `tandem factory checks discover [--target NAME] [--json]` | Propose `test` and `lint` commands by reading target project files |
| `tandem factory checks set --target NAME [--test CMD] [--lint CMD] [--deploy JSON_CMD] [--timeout-secs N] [--ui-paths GLOBS] --confirm NAME [--json]` | Confirm `test`, `lint`, or `deploy` commands for a target (user-only) |
| `tandem factory checks show [--target NAME] [--json]` | Show confirmed check commands |
| `tandem factory login status [--json]` | Check installed/signed-in status for `claude`, `agy`, and `opencode` |
| `tandem factory login open <CLI> [--json]` | Open an isolated tmux login pane (`@tandem_role=login`) using the CLI's own login command |
| `tandem factory agent assign --run <RUN> --stage <STAGE> --cli <CLI> --model <MODEL> [--by user\|orchestrator] [--json]` | Assign or override the interactive CLI/model for a stage (`user` always overrides `orchestrator`) |
| `tandem factory agent list --run <RUN> [--json]` | List recorded worker jobs, pane liveness, assignment source, and activity status |
| `tandem factory agent send --run <RUN> --job <JOB> --file <PATH> [--json]` | Deliver a file-backed message to an owned worker pane via guarded `paste-buffer` |
| `tandem factory agent capture --run <RUN> --job <JOB> [--lines N] [--json]` | Capture output from an owned worker pane (fails closed on login/non-worker panes) |
| `tandem factory agent terminate --run <RUN> --job <JOB> --confirm <JOB> [--json]` | Stop an owned worker pane and mark its job interrupted |
| `tandem factory agent relaunch --run <RUN> --job <JOB> [--json]` | Relaunch a worker pane using the active `assignments.json` entry |
| `tandem factory provider status [--json]` | Show legacy headless terms verdicts and gate state |
| `tandem factory settings get [--json]` | Show current factory settings |
| `tandem factory settings set <KEY> <VALUE> [--json]` | Update a non-protected factory setting within valid bounds (user-only) |
| `tandem factory run budget <RUN> [--max-cost USD] [--max-duration SECS] [--max-retries N] [--json]` | View or update budget caps for a run (mutating is user-only) |
| `tandem factory run usage <RUN> [--json]` | Show usage, cost basis (`actual`/`estimated`/`unknown`), and rework signals for a run |
| `tandem factory run approve <RUN> --action <ACTION> --confirm <RUN> [--stage STAGE] [--json]` | Grant a one-time Gate 3 approval (`production_deploy`, `secrets_access`, `production_access`; user-only) |
| `tandem factory run data <RUN> [--json]` | Show what data each job sent vs. what it could read and default exclusions |
| `tandem factory permissions allow <SUBJECT> <TOOL> [--target] [--agent] [--json]` | Allow a tool for an agent or un-deny it for `--target` (user-only) |
| `tandem factory permissions deny <SUBJECT> <TOOL> [--target] [--agent] [--json]` | Deny a tool for a target repository or `--agent` (user-only) |
| `tandem factory permissions show [--json]` | Show configured and default tool permissions |
| `tandem factory redact test <FILE> [--json]` | Scan a file and report what secret redaction would mask |
| `tandem factory audit [IDEA] [--run RUN] [--kind KIND] [--actor ACTOR] [--json\|--markdown]` | Inspect or export the audit timeline (JSON or redacted Markdown) |
| `tandem factory audit verify [--json]` | Verify the SHA-256 `prev` hash chain of `events.jsonl` |
| `tandem factory doctor [--fix] [--json]` | Diagnose and optionally repair interrupted Factory Mode runs, locks, or workspace state |

Exit codes: `0` success, `1` not found, invalid input, audit chain mismatch, or simulation stopped/blocked, `2` Factory Mode is off, `3` actor not allowed (agent caller/pane or non-terminal on user-only commands such as `ready`, `revoke-ready`, `checks set`, `settings set`, `permissions allow|deny`, `run budget` mutation, `run approve`, `provider consent|revoke`), gate not met, checks not confirmed, or question ceiling reached.

- **Required plan sections (`##`)**: `Scope`, `Acceptance criteria`, `Risks`, `Estimates`, `Workflow recommendation`.
- **Question tagging**: Questions asked with `task = "idea:<id>"` feed `blockers`, `needs_you`, and Gate 2.
- **User-only rule**: User-only commands route through one shared approval gate (`approval::require_user`) enforcing a terminal check (`IsTerminal`), tmux `@tandem_role` pane check, derived `via` surface (`--via` removed), `approval_grace_secs` undo window, and audit logging. See [Approval boundary](#approval-boundary-threat-model--five-measures) for what it stops and does not stop.
- **Orchestrator recommendation**: The Orchestrator may recommend a workflow by name in the plan, but cannot edit workflow policy; the validator's upper bounds always apply to whatever the user selects.
- **Factory tab & Web views**: TUI Factory tab (`Alt+f`) provides new idea (`Ctrl+Alt+n` choosing `claude`, `agy`, or `opencode`), lifecycle filter with active team count and sign-in badges, detail/artifact reader, mark ready (`Ctrl+Alt+y`), Run Monitor with **Team** panel (`[v]iew`, `[m]essage`, `[s]top`, `[x] relaunch`), **Stage Assignments** modal (`a`, labeling user overrides `"you"`), `"cost: not tracked"`, and **Agents & sign-in** in Settings; `tandem-web` mirrors these under `/api/workspaces/:id/factory/login/...` and `/api/workspaces/:id/factory/runs/:runId/agents/...` with `<TeamPanel>` and `<AssignmentEditor>`.

### Workflow reference (`workflows/*.yaml`)

`init` installs `<workspace>/workflows/default-delivery.yaml`, which is **user-owned** after creation (`update` never overwrites it; run `tandem factory workflow default` to view the shipped default). If removed, the engine falls back to the embedded default (`workflow source: embedded`).

- **Top-level keys**: `name` (`string`, `^[a-z][a-z0-9-]{0,47}$`), `version` (`u32 >= 1`), `trigger` (`ready-for-development`), `policy` (object), `stages` (non-empty list, max 16 stages).
- **Policy keys**: `max_parallel_agents` (`u32`, `<= factorySettings.max_parallel_agents`), `default_temperature` (`0.0..=1.0`), `require_human_approval_for` (list of protected actions: `deploy`, `merge`, `force_push`).
- **Stage keys**: `name` (`^[a-z][a-z0-9-]{0,47}$`), `agent` (vocab), `depends_on` (list of prior stage names), `tools` (non-empty list from tool vocab), `when` (optional condition), `requires` (optional requirements list), `produces` (optional evidence kinds list), `budget` (`max_cost_usd`, `max_duration_secs`, `max_retries`).
- **Vocabularies**:
  - **Agents**: `orchestrator`, `test-engineer`, `implementer`, `verifier`, `reviewer`, `demo-maker`, `release-engineer`.
  - **Tools (class)**: `read_file` (`read`), `search_code` (`read`), `git_diff` (`read`), `write_file` (`write`), `edit_file` (`write`), `run_tests` (`exec`), `run_checks` (`exec`), `diff_review` (`review`), `capture_demo` (`artifact`), `deploy` (`deploy`).
  - **Conditions (`when`)**: `ui_changed`, `deploy_requested`.
  - **Requirements (`requires`)**: `human_approval`.
  - **Evidence (`produces`)**: `plan`, `tests`, `patch`, `test_report`, `review_report`, `demo_artifact`, `deploy_record`.
- **Execution rules**: (1) A stage starts only when every `depends_on` stage has succeeded or was skipped. (2) False `when` skips the stage with a reason; unknown `when` waits. (3) `requires: [human_approval]` blocks until a matching approval exists. (4) A failed stage never advances dependents. (5) Running stages never exceed `min(policy.max_parallel_agents, settings.max_parallel_agents)` (file order breaks ties).
- **Validation diagnostics (`W001`–`W013`)**:
  - `W001`: Invalid workflow `name` (fix: lowercase `[a-z0-9-]`, <= 48 chars).
  - `W002`: Missing or invalid `version` (fix: positive integer `version: 1`).
  - `W003`: Unknown `trigger` (fix: `trigger: ready-for-development`).
  - `W004`: Duplicate or invalid stage `name` (fix: unique lowercase identifier per stage).
  - `W005`: Unknown `depends_on` reference (fix: reference an existing stage name).
  - `W006`: Dependency cycle detected (fix: break the cycle so stages form a DAG).
  - `W007`: Unknown `agent` or `tools` entry (fix: use values from the vocabularies above).
  - `W008`: Unknown `when`, `requires`, or `produces` entry (fix: use supported vocab values).
  - `W009`: `max_parallel_agents` is 0 or exceeds `settings.max_parallel_agents` (fix: lower `max_parallel_agents`).
  - `W010`: `default_temperature` outside `0.0..=1.0` or negative budget (fix: clamp temperature to `0.0..=1.0`).
  - `W011`: No stage grants `diff_review` or `write`-class stage lacks `test` coverage (fix: include a `reviewer` stage with `diff_review` and a `run_tests` stage).
  - `W012`: `deploy` tool used without `requires: [human_approval]` (fix: add `requires: [human_approval]` on the deploy stage).
  - `W013`: Stage count is 0 or exceeds 16 (fix: keep 1–16 stages).

### Storage layout

```text
.agent-workspace/factory/
├── .gitignore            # "*"
├── events.jsonl          # append-only event log (0600)
├── settings.json         # factory settings and provider gates (all closed by default)
├── ideas/<idea-id>.json  # materialized idea state (0600)
└── runs/<run-id>/
    ├── workflow.yaml     # byte-identical copy of the resolved workflow
    ├── run.json          # materialized run state
    └── jobs/<run-id>.<stage>.<attempt>/
        ├── state         # prepared | running | finished | interrupted
        ├── input.json    # correlation_id, stage, attempt, model, tool_grants, cwd, context_manifest
        ├── prompt.md     # assembled prompt for the stage job
        ├── result.json   # structured outcome written by the job
        └── log.txt       # raw stdout/stderr output
```

### Login choice, interactive team & stage assignments

> **Governance stance:** Factory Mode accelerates execution; it does not transfer accountability. Teams keep approval, merge, deployment, and compliance responsibility.

- **Worker logins & terms notice**: Interactive worker panes (`claude`, `agy`, `opencode`) use whatever login each CLI already holds on the host (`tandem` handles no credentials and reads no OAuth tokens or API keys). Subscription logins may conflict with provider terms of service — see the 2026-11-20 summary in `knowledgebase/architecture-decisions/factory-provider-terms.md` (`claude`: API key/Console `allowed-with-conditions`, consumer subscription `not-allowed` / gray; `agy`: Gemini Enterprise / AI Studio API key / Vertex AI `allowed-with-conditions`, consumer OAuth `unclear`; `opencode`: configured provider auth). You can sign in with an API key through **Settings → Agents & sign-in** (`tandem factory login open <cli>`).
- **Guarded tmux ownership**: Worker panes (`@tandem_role=worker`, `@tandem_run_id=<run>`) are launched interactively inside tmux (`TmuxClient::launch_worker`). `send_worker` / `paste_worker` / `capture_worker` fail closed on non-worker panes, including `@tandem_role=login` panes opened for sign-in.
- **Assignment precedence (`assignments.json`)**: `user` > `orchestrator` > workflow `default`. Agents cannot pass `--by user` and cannot overwrite an existing user override.
- **Activity signals & hooks**: When a worker uses `claude`, tandem installs an isolated worktree `Stop` hook (`.claude/settings.local.json`, excluded via git `info/exclude`) updating `<job_dir>/activity.json`; `team_watch.py` polls files first before `agent capture`.

### Runner, retries & action requests

- **Operating loop**: The runner acquires `.agent-workspace/factory/`'s runner lock, checks CLI readiness (`broker::gate::interactive_ready`), verifies `plan-snapshot.md` against its recorded SHA-256, prepares the run worktree, and steps the workflow scheduler through interactive worker panes (`assignments.json`). Interactive runs record `"cost: not tracked"` (`max_duration_secs` and `max_retries` caps still enforce).
- **Retries & new-hypothesis rule**: A failed stage pauses the run unless a retry provides a non-empty, distinct **new hypothesis** explaining what changed. Grant one more attempt with `tandem factory run retry <run> <stage> --hypothesis "<new hypothesis>"`.
- **Action requests**: When a stage requires user action or input, `run.json` records the exact commands in `action_commands` so the user can resolve the blocker before running `tandem factory run resume <run>`.
- **`result.json` schema**: `status` (`succeeded` | `failed` | `needs_input`), `summary`, `files_changed`, `artifacts`, `rationale` (required when `failed`), and `hypothesis` (required on retry).

### Run worktree, target selection & cleanup

- **Worktree & branch**: Each run executes in an isolated git worktree at `worktrees/factory-<run-id>` on branch `feat/<idea-id>-<slug>` (suffixed `-2`, `-3`, ... if that branch already exists), branched from the repository's primary branch (`main`, `trunk`, `master`, or `HEAD`).
- **Target rule**: If 0 or 1 git codebases exist under `codebase-symlinks/`, tandem selects it automatically. If multiple git codebases exist, select one before starting with `tandem factory run target <run> <name>`.
- **Cleanup**: `tandem factory run cleanup <run> --confirm <run>` removes the worktree for completed, cancelled, or withdrawn runs (refusing dirty worktrees with uncommitted changes).

### Known limits (execution)

- Jobs are stopped with a kill after a grace period and their own child processes may survive.
- Cost is unknown unless the provider reports usage.

### Check commands (`checks discover|set|show`)

- **Discovery**: `tandem factory checks discover [--target NAME]` inspects project manifests (such as `package.json`, `Cargo.toml`, `go.mod`, or `Makefile`) and proposes `test` and `lint` commands.
- **User-only confirmation**: `tandem factory checks set --target NAME --test "..." --lint "..." --confirm NAME` saves the confirmed commands in `settings.json`. Only the user can set check commands (agent environments exit with code `3`) because the `test` stage runs those commands directly without an LLM agent.
- **Inspect**: `tandem factory checks show [--target NAME]` displays confirmed check configurations.

### Stage contracts, scope check & review loop

| Stage | Contract |
|---|---|
| `create-tests` | Scope check passes; records `create-tests-evidence.json` with test files (or an explicit `no_behaviour_change` rationale). |
| `develop` | Scope check passes; non-empty diff; preserves tests created in `create-tests` and existing test files unless justified. |
| `test` | Runs user-confirmed `test` and `lint` commands directly via the check broker and records check logs under `checks/`. |
| `review` | Produces `findings.json`; any `blocker` or `major` finding forces `changes_requested` regardless of stated verdict. |
| `demo` | Conditional (`when: ui_changed`); validates `demo-manifest.json` and local PNG screenshots (<= 20 files, <= 5 MiB each). |

- **Scope check & protected paths**: Every write-capable stage verifies changed files against `slices.json` paths (plus allowed lockfiles). Changes to protected paths are always rejected: `.agent-workspace/`, `workflows/`, `.github/workflows/`, `.git/hooks/`, `.githooks/`, and `checks.json`.
- **Review loop & `max_review_rounds`**: When `review` returns `changes_requested`, the runner loops back through `develop` → `test` → `review` up to `max_review_rounds` (default `2` in `settings.json`), pausing the run with remaining findings if the limit is exhausted.

### Evidence & pull requests

- **Evidence completeness**: Stages record typed evidence (`plan`, `tests`, `patch`, `test_report`, `review_report`, `demo_artifact`, `deploy_record`). Before opening a PR, tandem verifies every `produces` kind in the workflow is present (`tandem factory run evidence <run>`).
- **Local-only logs and screenshots**: Raw `log.txt` files and `demo` screenshots stay on the machine and are never uploaded or embedded in pull requests until secret/log redaction exists (`p-018`).
- **Pull request preconditions**: `review` verdict must be `approve`, required evidence must be complete, the run branch must be non-protected with commits beyond base, the worktree must be clean, and `origin` must be a GitHub remote.
- **`gh` prerequisite & scope**: `gh` must be installed and authenticated by the user (`gh auth login`). Pull request creation supports **GitHub only** for now and opens at most **one pull request per run** (`tandem factory run prs <run> [--refresh]`).
- **What tandem never does**:
  - Never **merges** a pull request.
  - Never **approves** a pull request.
  - Never **force pushes**.
  - Never **pushes to a protected branch** (`main`, `master`, `trunk`).

### Settings & workflow tightening (`settings get|set`)

Inspect settings with `tandem factory settings get [--json]` and update non-protected keys with `tandem factory settings set <KEY> <VALUE> [--json]` (user-only).

| Setting | Default | Valid range |
|---|---|---|
| `max_parallel_agents` | `2` | `1..=8` |
| `default_temperature` | `0.2` | `0.0..=1.0` |
| `max_review_rounds` | `2` | `0..=5` |
| `approval_grace_secs` | `30` | `0..=600` |
| `default_workflow` | `default-delivery` | `^[a-z][a-z0-9-]{0,47}$` |
| `default_budget` (`max_cost_usd`, `max_duration_secs`, `max_retries`) | unset / optional | `max_cost_usd >= 0.0` |
| `model_tiers`, `price_table` | per provider/model | valid model IDs & non-negative rates |

- **Protected keys**: `provider_gates`, `providers`, `checks`, `deploy`, `deploy_command`, `version`, `allowed_tools`, and `denied_tools` cannot be changed with `settings set`; use their dedicated user-only commands (`provider`, `checks`, `permissions`).
- **Workflows can only tighten**: A workflow's `policy` and stage budgets cannot relax `settings.json` bounds (`settings::effective` takes `min` for `max_parallel_agents`, `default_temperature`, and budget caps).

### Budgets & telemetry (`run budget`, `run usage`)

- **Run & stage budgets**: View or update run caps (`max_cost_usd`, `max_duration_secs`, `max_retries`) with `tandem factory run budget <RUN> [--max-cost USD] [--max-duration SECS] [--max-retries N]`. Mutating caps is user-only.
- **Pre-run estimates & basis**: Before starting, `run simulate` and run creation compute an estimate from `price_table` using a typical stage baseline (`25,000` input tokens and `5,000` output tokens per stage) and report whether the estimate basis is `estimated` or `unknown`.
- **Budget breach**: When `max_cost_usd`, `max_duration_secs`, or `max_retries` is reached, the runner pauses the run and records `action_commands` so the user can either raise the cap (`run budget`) and `run resume`, or `run cancel`.
- **Unenforceable cost cap limit**: A `max_cost_usd` cap is **not enforceable** for a provider that reports no usage (displayed as `not enforceable for <provider>`). Duration and retry caps still apply.
- **Telemetry (`run usage`)**: `tandem factory run usage <RUN>` reports per-stage tokens, cost, cost basis, and rework signals:
  - `actual`: the provider reported `cost_usd` directly in `result.json`.
  - `estimated`: computed from reported token counts (or the pre-run stage baseline) using `price_table`.
  - `unknown`: the provider reported no usage or the model has no `price_table` entry.

### Secret & log redaction (`redact test`)

- **What is found**: `redact` scans exported text, audit entries, and job summaries for `PrivateKey`, `EnvSecret`, `Token`, `UrlCredential`, `KeyValue` (secret key names such as `token`, `password`, `api_key`, `credential`), and `HighEntropy` strings, replacing matches with `«redacted:<kind>»` while skipping known hash-context keys (`sha256`, `commit`, `digest`, `checksum`).
- **What is not redacted**: `demo` **screenshots** (PNG images) and **raw local logs** (`log.txt` on disk) are not rewritten by redaction; they stay local to the machine.
- **Test before sharing**: Run `tandem factory redact test <FILE> [--json]` to inspect every match and preview what would be redacted.

### Permissions, Gate 3 & deploy preconditions (`permissions`, `run approve`)

- **Tool permissions**: Manage per-agent allowlists and per-target denylists with `tandem factory permissions allow|deny <SUBJECT> <TOOL> [--target] [--agent]` (user-only) and inspect with `tandem factory permissions show`.
- **Denials**: If a stage requests a tool that is not granted (`NotAllowedForAgent`), denied for the target (`DeniedForTarget`), or requires Gate 3 (`Gate3Required`), the runner pauses and surfaces exact `action_commands` to resolve it.
- **Gate 3 (protected actions)**: Protected actions (`production_deploy`, `secrets_access`, `production_access`) require a one-time user-only approval via `tandem factory run approve <RUN> --action <ACTION> --confirm <RUN> [--stage STAGE]`. Approvals are bound to `run_id`, `stage`, and `workflow_sha256`, and expire after 1 hour (`3600` seconds).
- **Deploy stage preconditions**: All four must hold before a `deploy` stage executes:
  1. `--deploy` was requested when the idea was marked ready (`deploy_requested`).
  2. At least one pull request exists on the run (`run prs`).
  3. A deploy command is confirmed for the target (`tandem factory checks set --target <NAME> --deploy '["<cmd>"]' --confirm <NAME>`).
  4. A matching, unexpired Gate 3 `production_deploy` approval exists (`tandem factory run approve <RUN> --action production_deploy --confirm <RUN>`).

### Audit timeline, export & integrity (`audit`, `audit verify`)

- **Timeline & export**: `tandem factory audit [IDEA] [--run RUN] [--kind KIND] [--actor ACTOR] [--json|--markdown]` renders the chronological decision, approval, lifecycle, and job timeline for an idea. `--markdown` exports a redacted Markdown report; `--json` exports structured entries plus verification status.
- **Hash-chain verification**: `tandem factory audit verify [--json]` verifies the SHA-256 `prev` hash chain across `events.jsonl` and exits `1` if any entry is missing, malformed, or tampered with.

### Data handling (`run data`)

- **Inspect per-job data**: `tandem factory run data <RUN> [--json]` shows what tandem explicitly sent (`sent_files` in `context_manifest` and total bytes), what the job could read (target/worktree scope), and default exclusions.
- **What tandem cannot see**: The provider CLI can read any file in the readable scope. tandem does not see which.
- **Default exclusions**: `.env*`, `*.pem`, `*.key`, `id_rsa*`, `secrets/**`, and `.git/**` are excluded from `context_manifest` by default.

### Approval boundary (threat model & five measures)

- **Threat model**: Protects against an agent approving or mutating a user-only control by mistake or via untrusted text (such as prompt injection in an issue, PR, or log). It **does not stop** a deliberately hostile process running as the **same OS user** (preventing a same-user process from forging state requires OS isolation such as a separate OS user, container, or sandbox).
- **Five defence-in-depth measures** (`approval::require_user`):
  1. **Terminal required on CLI**: CLI user-only commands refuse when standard input is not a terminal (`std::io::IsTerminal`), returning `"this action needs a terminal"` (exit `3`).
  2. **Not from an agent pane**: When `TMUX_PANE` is set, tandem queries the pane's `@tandem_role` tmux option and refuses if it names an agent role (cannot be bypassed by clearing shell env vars).
  3. **Derived surface (`--via` removed)**: `--via` was removed from the CLI; CLI calls always record `cli`, Status window actions pass `tui` in-process, and `tandem-web` passes `web` through an authenticated internal gateway.
  4. **Grace window (`approval_grace_secs`, default `30`)**: After an idea is marked ready, the runner waits `approval_grace_secs` seconds while Status (`Ctrl+Opt+u`) and `tandem-web` (`[Undo]`) show an undo banner.
  5. **Prominent audit timeline**: Every approval and revocation is recorded in `events.jsonl` and surfaced in `tandem factory audit`.

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
| `adapters/` | Build exact argv (and, for opencode, the model environment) for `claude` / `agy` / `opencode`; list models |
| `wizard/` | Launch and add-agent screens |
| `tmux/` | Allow-listed tmux client, layout, tabs, keymap |
| `coordination/` | Typed reader/answer-writer for the protocol files |
| `sources/`, `snapshot.rs`, `status/` | Status data sources and the ratatui UI |
| `state.rs`, `conversations.rs`, `quota.rs` | Saved sessions, conversation IDs, quota feed |
| `updater.rs`, `update_check.rs`, `notice.rs` | In-session update, version check, what's-new notice |

The golden fixtures in `tests/fixtures/parity/` were captured from legacy tandem 1.5.0 and are frozen.

`tests/tos_guard.rs` fails the build if code could send keys to or capture output from an AI pane.
