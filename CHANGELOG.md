# Changelog

## [Unreleased]

### Added: dependency check when enabling tandem-web

- Enabling tandem-web (the tandem **Web** tab, `./run.sh --web` and `agent-workspace web`) now checks that **bun runs on the current host** (not just that a file named `bun` exists) and, when the terminal feature is on, whether `ttyd` is installed.
- If bun is missing, tandem **asks**: *Let tandem install* or *I'll install it myself*. Installing generates a platform script from the official bun guide (`curl -fsSL https://bun.com/install | bash` on macOS/Linux, the PowerShell one-liner on Windows), saves it as `install-web-deps.sh` / `.ps1` in the state directory, shows its path, runs it (in the background in the TUI, with progress and errors shown in the pane), and verifies bun works before enabling. Choosing to install it yourself keeps the script for review and lists the commands. Scripts never use `sudo`.
- Platform handling: Linux glibc/musl and x64/arm64, macOS 13+, Windows. Missing `curl`/`unzip` and unsupported platforms or hosts (old macOS, x64 CPU without SSE4.2, no official build) are explained with the right package-manager command instead of an install that cannot work. `ttyd` is auto-installed only via Homebrew on macOS and is otherwise manual; it never blocks enabling.
- New flags for `agent-workspace web`: `--yes` installs without asking, `--no-install-deps` never installs. `run.sh` passes them from its own `--yes` / `--no` modes and no longer downloads bun itself (it used the legacy `bun.sh` URL).
- After an install, tandem finds bun in `$BUN_INSTALL/bin` or `~/.bun/bin` without requiring a PATH change, in both the Node launcher and the TUI.
- The Node and Rust planners are pinned to each other by a shared 16-case fixture (`tandem/tests/fixtures/parity/web-deps.json`).

### Fixed

- **The web CLI tests could stop a real tandem-web.** `test/web.test.js` ran `web --status` and `web --stop` against the real state directory, so with a server running two tests failed and `--stop` terminated the server and rewrote its config. They now use a throwaway state directory, with a regression test (`test/web-hermetic.test.js`).
- tandem no longer panics when the status pane is narrower than 2 columns (summary line width underflow).

## [1.11.1] — 2026-11-08

### Fixed

- **`./run.sh --web` silently did nothing in a normal workspace.** `init` does not ship `tandem-web`, and `run.sh` returned early when the folder was missing. It now installs `tandem-web` on first use: the bundled source is copied once into the state directory (`~/.local/state/tandem/tandem-web`, stamped with the package version and refreshed after an upgrade), then dependencies are installed and the app is built only if missing. Without a local CLI, `run.sh` launches through `npx --yes <TANDEM_UPDATE_SPEC> web` (spec validated). Missing `npx` or Node.js, an invalid spec, and a declined `bun` install each print a message instead of doing nothing.
- The tandem Web tab now finds the installed copy as well as a `tandem-web` folder in the workspace, and says when it is not installed, not built, or from an older version, with the command to run (`agent-workspace web`).

### Added (tests)

- `test/run-sh-web.test.js`: end to end `./run.sh --web` with stub `bun`, `curl` and `npx` covering fresh install, partial installs, idempotence, version change, failures, bun missing/declined/accepted, and the init-style workspace.
- Resolver and install unit tests (`test/web-launch.test.js`) and `tandem/tests/test_web_consent.rs`: the TUI first-time prompt says additional dependencies may be installed, nothing runs before consent, declining is asked again, consent is remembered across restarts, plus the Rust resolver and its error messages.

### Fixed

- **Test suite no longer scaffolds workspace files into the repository.** `test/baseline.test.js` launched the repo's own `run.sh` with updates enabled. In a checkout that has a `.agent-workspace/manifest.json`, and once a newer release is published, `run.sh` ran `npx agent-workspace update` in the current directory, creating `AGENTS.md`, `.agents/`, `specs/`, `sprints/`, `knowledgebase/` and `WORKFLOW.md`, and then made the `bad-runtime` scenario exit 1 instead of 4. `run.sh` is now started through a shared helper (`runRunSh`) that sets `TANDEM_SKIP_UPDATE=1` and uses a throwaway working directory, with a regression test (`test/run-sh-hermetic.test.js`) that proves it. Baseline scenarios also no longer hit the network.

## [1.11.0] — 2026-11-08

### Added: Web pane in tandem (Phase p-009)

- New **Web** status tab (key `6`; **Settings is now tab `7`**) that owns all `tandem-web` configuration, moved out of Settings: Enable/disable with the first-time dependency consent, live status (Running / Starting / Error), Bind IP and source, **Port** (editable, 1024-65535), **Terminal** (`ttyd`) toggle, **Token** (masked, show/hide), **Connect** and **Regenerate token** (with confirmation).
- **QR overlay:** the Connect row opens a full QR code for the access URL (black on white, readable on any terminal theme). It never prints the token as text. If the pane is too small it reports the size it needs; loopback binds explain that a VPN or LAN IP is required.
- Changing the bind IP, port, terminal toggle or token while the server is running now **restarts it**, so the change takes effect. Before, the running server was left untouched.
- Start failures (for example "tandem-web is not built") are **shown in the pane** instead of being silently dropped.
- Settings tab now has only the grid options, the Quit button and Reset UI (Reset UI is now visible at the default pane height). `tandem focus settings` selects tab `7`.
- Number-key hint in the footer and help window corrected to `1-7`. Golden snapshots for the tab bar, footer and key help were regenerated for the extra tab; no other lines changed.
- New dependency: `qrcodegen` 1.8 (no transitive dependencies).

### Fixed

- Settings-tab Changed events no longer start or stop the web daemon as a side effect.

### Added: Quit button (Phase p-008)

- **Quit button in tandem Settings tab**: Added a `[ Quit… ]` button on body row 0 of the tandem Settings tab beside the `Grid layout` header. Activating via Enter, Space, or left click opens the exit menu (save and exit, detach, update tandem, discard, cancel), providing full keyboard and mouse parity with `Ctrl+Alt+x` directly inside the Settings tab.

## [1.10.0] — 2026-11-08

### Security and reliability fixes for `tandem-web` (found in post-merge review of #39)

- **Authentication was never enforced.** The auth gate lived in `src/proxy.ts`, which Next 15 does not load; it is now `src/middleware.ts`. The gate fails closed: with no `TANDEM_AUTH_TOKEN` configured the server returns 503 instead of serving. Unauthenticated pages return 401 instead of redirecting to a nonexistent `/login`. Token comparison is constant-time and edge-runtime safe.
- **Bind address was ignored.** `next start` does not read `HOSTNAME`, so the dashboard listened on all interfaces. Both launchers now pass `-H <ip> -p <port>` and refuse wildcard addresses.
- **`agent-workspace web` did not pass the auth token** to the server and generated tokens with `Math.random`. It now uses `crypto.randomBytes` (`tandem_<32 hex>`, shared with the Rust launcher), passes `TANDEM_AUTH_TOKEN`, and regenerates weak tokens found in an existing `web-config.json`.
- **Optional `ttyd` terminal had no credentials.** It now requires a credential, refuses wildcard binds, and starts with `-c tandem:<token>`.
- Action arguments beginning with `-` are rejected (CLI flag injection).
- **tandem-web could not build from a clean install:** `autoprefixer` was missing from `package.json` and the home page referenced `window` during prerender. Both fixed.
- **Nothing ever installed or built tandem-web.** `agent-workspace web` now runs `bun install --frozen-lockfile` and `bun run build` on first use; the TUI toggle reports a clear error if the build is missing.
- Removed unused duplicate `infrastructure/gateway` and `infrastructure/registry` modules and the committed `tsconfig.tsbuildinfo`.

### Fixed: sidebar panic on narrow widths

- The tandem sidebar sliced rendered text by byte length, which panics when a row contains a multi-byte glyph (`▶`, `☑`, `↗`) and the pane is narrow. The compact brand (`▶tdm 1.10 ↗`) triggered it as soon as the minor version reached two digits. All five render sites now truncate on character boundaries by display width.

### Connect from your phone with a QR code

- `agent-workspace web` (start, already-running, and `--status`) prints the access URL and a terminal QR code. Scanning it opens the dashboard and exchanges the one-time `?token=` for an HttpOnly cookie. Loopback binds print a hint to use `--bind <vpn-or-lan-ip>` instead. Suppress with `--no-qr`.
- New dependency: `qrcode-terminal@0.12.0` (loaded lazily; the URL is still printed if it is unavailable).
- The tandem Settings tab points at `agent-workspace web --status` for the QR code.

### Remote Multi-Workspace Web Interface (`tandem-web`) (Phase p-007)

- **Remote Multi-Workspace Web Dashboard (`tandem-web`)**:
  - Web dashboard built with Bun, Next.js (App Router), Tailwind CSS, and shadcn/ui.
  - Multi-workspace switcher: discover, monitor, and switch across running `tandem` workspaces from phone or browser.
  - Responsive ergonomics: sticky 5-tab thumb-zone bottom navigation bar on phone (`<768px`) with `44px` minimum touch targets; persistent split-pane sidebar on tablet and desktop (`≥768px`).
  - Real-time updates: live Status, Sprint, Spec, Task, and Quota metrics streamed via Server-Sent Events (SSE).
  - 100% ToS Air-Gap: zero network imports or shell strings in the `tandem` Rust crate (`tests/tos_guard.rs` clean); agent controls (`Focus`, `Toggle Grid`, `Restart`, `Close`, `Equalize`, `Reset UI`) invoke existing allow-listed CLI subcommands with zero scraping or keystroke injection into AI panes.
  - Touch-first Queries inbox: answer agent questions (`coord.py ask`) from your phone with one-tap quick-option chips or freeform text.
  - Optional `ttyd` terminal attachment: attach directly to the workspace tmux session with a touch-friendly modifier toolbar (`Opt`/`Alt`, `Ctrl`, `Esc`, `Tab`, arrows, and tandem shortcuts).
  - Settings Tab integration: Remote Web toggle in TUI Tab 6 Settings with first-time dependency consent modal, IP discovery, and network source badges (`tailscale`, `wireguard`, `zerotier`, `vpn`, `lan`, `localhost`).
  - Strict security guardrails: opaque `tandem-<10-hex-hash>` routing, timing-safe CSPRNG token authentication, blocked `0.0.0.0` wildcard binding, and comprehensive 8-category OWASP security test suite.
  - CLI entrypoints: `./run.sh --web [--bind <ip>] [--port <port>]` and `agent-workspace web [--status] [--stop] [--list-ips]`.
- `AGENTS.md` template (Branch Protection): worktrees must be created inside the `worktrees/` directory at the workspace root, created first if missing.

## [1.9.4] — 2026-11-08

### Changed

- `--fresh` now ends a tandem session that is already running for the workspace and starts from a clean state, instead of refusing (1.9.3) or attaching to it (1.9.2 and earlier). `./run.sh --fresh` asks first (skip with `--yes`), then ends the session, clears the coordination directory and discards the saved session. Declining leaves everything untouched (exit 2).

## [1.9.3] — 2026-11-08

### Fixed

- `--fresh` no longer silently attaches to a tandem session that is already running (which showed the old layout and dead panes instead of a fresh state). tandem now exits with code 2 and asks you to quit the running session first.
- `./run.sh --fresh` now detects a running session even when the coordination directory does not exist yet, and compares workspace paths by their real path so symlinked or `/private` path forms are recognised.

## [1.9.2] — 2026-11-08

### Fixed

- `./run.sh --fresh` now starts tandem on a fully fresh state: besides clearing the coordination directory, tandem deletes the saved session (`.agent-workspace/tandem-session.json`), so a later plain `./run.sh` no longer offers to resume the discarded session.

## [1.9.1] — 2026-11-08

### Fixed

- `run.sh --fresh --check` no longer creates the coordination directory when it is missing; `--check` now makes no changes, as it reports.

## [1.9.0] — 2026-11-08

### Responsive UI, Sidebar & Multi-Agent Grid (Phase p-006)

- **Dedicated Agent Sidebar (`tandem sidebar`):** Replaces the previous rigid 2-column layout with a left sidebar displaying running agents categorized by role (Planners, Implementers), active conversation status badges (`▶` focused, `●` running, `○` stashed, `✕` exited), question badges, and quick actions (`+ Add agent`, `? Help`).
- **Multi-Agent Grid View in Main Window:** Select and tile multiple agents simultaneously in the main area.
  - Press `Enter` on a sidebar row to show the agent alone.
  - Press `Space` or `Shift+Click` / `Opt+Click` to toggle agents in and out of the multi-agent grid.
  - Overflow scrollbar (`gridbar`): automatically displays a vertical scrollbar when selected agents exceed visible grid cells; page with `Opt+PgUp` / `Opt+PgDn` or mouse wheel.
- **Responsive Terminal Breakpoints:** Layouts adapt dynamically to terminal geometry:
  - **Wide (≥120 cols):** Full sidebar with brand line, role sections, and multi-column grid.
  - **Medium (80–119 cols):** Compact 14-col sidebar with glyphs and 4-character truncated agent names.
  - **Narrow (<80 cols) / Short (<20 rows):** Sidebar auto-hides; grid collapses to single-pane view.
- **Double-Click Border Equalize:** Double-clicking any inner pane border equalizes sibling split dimensions; double-clicking structural borders resets sidebar width or status height. In resize mode (`Ctrl+Opt+z`), press `e` to equalize via keyboard.
- **Status Window Settings Tab (6th Tab):** Introduces `[6 Settings]` tab (`Alt+,` / `≤` or direct key `6`):
  - Configure grid columns (`Auto`, `1`–`4`), rows per page (`Auto`, `1`–`3`), fill order (`Row first`, `Column first`), and sidebar mode (`Auto`, `Wide`, `Compact`, `Hidden`).
  - Interactive **Reset UI** action with confirmation modal (`[ Reset ] [ Cancel ]`) to restore default layout geometry without interrupting running agents.
  - Preferences persist across restarts in `SavedState` schema v3.
- **Clickable Brand & Version Link:** Header displays `tandem 1.9.0 ↗` (compact `tdm 1.9 ↗`) linking to `https://github.com/hydrokat/agent-workspace` via OSC 8 terminal hyperlinks and platform browser spawners (`open` / `xdg-open`).
- **Keymap & Keyboard-Only Navigation:**
  - `Opt+Up` / `Opt+Down`: Navigate previous/next agent in sidebar order (shown alone).
  - `Opt+1` / `Opt+2`: Show last planner / last implementer agent.
  - `Opt+b` (new, macOS `∫`): Focus sidebar (pops up when hidden).
  - `Opt+,` (new, macOS `≤`): Jump directly to Settings tab.
  - `Opt+PgUp` / `Opt+PgDn` (new): Scroll multi-agent grid one row.
  - `Esc`: In sidebar, returns focus immediately to the active agent pane.
- **Python Runtime Removal:** Deprecated Python runtime completely removed. `TANDEM_RUNTIME=python` is no longer accepted (use `auto`, `binary`, or `source`). All dependencies on `uv` or Python in tandem binaries eliminated.
- **`run.sh --fresh` Flag:** Starts tandem fresh by wiping all stale sessions, claims, and queries in `.agent-workspace/coordination/` before launching.
- **Performance Optimizations:**
  - Dirty-only TUI redraw loop eliminates 90% of idle redraws (0.5 draws/sec idle).
  - Batched tmux invocations (`\;`) reduce process spawns by up to 88% on build, close, and refresh.
  - Git mtime caching speeds up snapshot collection by >500x (39ms → 74µs warm).
- **Intended Behavior Change:** Single-click on a pane title border now focuses the pane rather than popping up the agent menu (resolving double-click equalize ambiguity). Agent menus remain accessible via right-click and the sidebar.

### Spec Compile Archiving & Guarded Phase Pruning (Phase p-015)

- **Spec Compile Archiving (`specs.py compile`):** Preserves historical archived phase sections in `specs/compiled/completed.md` when compiling, eliminating compile-wipes-archive data loss and ensuring pruned completed phases remain documented.
- **Guarded Phase Pruning (`specs.py prune`):** Provides a guarded command to clean up on-disk completed phase directories once archived in `completed.md`.
  - Dry run by default with explicit candidate and path summaries.
  - Requires explicit `--phases <names...>` and `--apply` flags to execute deletion.
  - Fail-closed safety guards: blocks if the git working tree is dirty, if any target phase is missing or incomplete in `completed.md`, if active agent coordination claims exist (`coord.py`), if active `in-progress.md` entries exist, or if uncompleted specs or active sprints reference the target phase.
- **Sprint Tooling Archived Phase Support:**
  - `sprints.py` (`close`, `status`, `next-work`) treats archived phases as complete with 0 remaining tasks, allowing sprint close and tracking without on-disk phase directories.
  - Missing or unreadable phases that are neither on disk nor archived block `sprints.py close` with an `unreadable_phases` error.
  - Sprint markdown rendering rewrites `Spec Link` to `../../specs/compiled/completed.md` for pruned phases.
  - `sprints.py lint` flags broken spec links pointing to phases that are neither on disk nor archived (`broken-spec-link`).
- **tandem Archived Phase Visibility:** Parses `specs/compiled/completed.md` alongside on-disk specs (with on-disk specs taking precedence on collision) and displays `(archived)` markers on phase rows in the Sprint tab and text summary.
- **`pm-sprint-implement` Step 4 Integration:** Automated compilation (`specs.py compile all`) followed by dry-run pruning (`specs.py prune --phases ...`) and a single interactive confirmation prompt (defaulting to keeping phases) before applying prune.

## [1.7.0] — 2026-11-02

### Sprint & Backlog Tooling (Phase p-016)

- **Backlog Scanner (`sprints.py backlog`):** Scans all incomplete specs and tasks outside the active sprint, classifying backlog items into unscheduled, outside-subset, and deferred (`Won't Have`) sources with active claim and dependency tracking. Supports `--scope specs|tasks|all`.
- **Sprint Task Subsets (`add-phase --tasks`):** Allows sprints to commit individual tasks within a phase rather than the entire phase.
  - Automatically resolves in-phase dependencies with `--with-deps`.
  - Flags `delivery_excluded: true` when the delivery task is not part of the subset.
  - Renders a 10th `Tasks` column in `sprint.md` only when subsets are present, preserving byte-identical formatting for sprints without subsets.
  - Subsets are fully respected by `status`, `next-work`, `close` (carry-over lists only subset tasks), and `lint` (flags unknown tasks in subsets).
  - Preserves custom hand-written markdown sections (such as `Business Objectives & Context`) during sprint file rewrites.
- **`/pm-backlogs` Skill:** Interactive AI skill to scan unscheduled work, inspect sprint capacity, select phases or task subsets, and either fold items into an open sprint or hand off to `/pm-new-sprint`.
- **Sprint Commands Catalog:** `/pm-commands` and `sprints.py commands` updated to include `/pm-backlogs` (expanding the PM command suite from nine to ten commands).

## [1.6.0] — 2026-11-02

### CLI Package & tandem

- **Five-tab Status window (Sprint s-004):** the status window expands from two tabs (Status, Queries) to five (Status, Sprint, Spec, Task, Queries). Navigate with `Tab` / `Shift+Tab`, `1`..`5`, mouse click, and the existing `Alt+s` and `Alt+q`. Content is scrollable with `↑↓`, `PgUp`/`PgDn`, `Home`/`End` and the mouse wheel.
  - **Sprint tab:** full ongoing sprint view with capacity against committed days, progress bar, days left, and phase progress rows sorted in MoSCoW order with missing-phase detection.
  - **Spec tab:** ongoing spec(s) with branch and merge target strategy, phase progress bar, task list ordered by task id with `▶` marking In Progress tasks, and next-up labelling.
  - **Task tab:** detailed ongoing task view with objective checklist progress (`objectives X / Y` with bar), owning session and pane, heartbeat age, branch, worktree, claimed paths, and next step from `in-progress.md`. `Alt+g` selects the owning agent's tmux pane. Bottom summary displays counts of In Review and Blocked tasks across ongoing specs.
- **tandem is now a native Rust binary (sprints s-001 to s-003):** the Python tandem is removed. The Rust port (ratatui + crossterm) keeps every feature and the on-disk formats, so saved sessions, coordination files, update notices and tmux sockets carry over, and running 1.5.0 sessions upgrade through Exit → Update.
- **Static Linux binaries:** `linux-x86_64` and `linux-arm64` are built for musl, which removes the glibc >= 2.31 requirement. macOS binaries target macOS 13+. Release asset names, archive layout and `SHA256SUMS` are unchanged.
- **Build on your machine when no binary is usable, with your consent (`run.sh`):** if the verified binary is unavailable (offline, checksum mismatch, blocked, or no prebuilt binary for your platform), `run.sh` asks whether to build tandem here with `cargo build --release --locked`, says why and where, and compiles only after a yes. Declining exits 2; non-interactive runs without `--yes` exit 6; an earlier healthy build of the same version is reused without asking; `TANDEM_RUNTIME=source` counts as consent. cargo is offered through rustup with the usual ask/describe/execute prompt. uv and Python are no longer needed, and `TANDEM_MIN_CARGO` replaces `TANDEM_MIN_PYTHON` and `TANDEM_MIN_UV`.
- **`TANDEM_RUNTIME=auto|binary|source`:** `python` still works as a deprecated alias of `source` and prints a notice.
- **Installer ships the crate:** `init` and `update` install `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` and `src/**/*.rs` under `.agent-workspace/tandem/`, and remove the legacy Python files (a `.bak` is kept for any you edited).
- **Release build and CI:** `tandem/packaging/build.sh` builds the five targets with cargo; CI runs fmt, clippy and tests for the crate, runs the skill-script pytest suites from `test/skills/`, and the release workflow verifies the tag against `package.json` and `Cargo.toml`.
- **`run.sh --check`:** a missing cargo is only a warning while a binary download is still possible.
- **Fix:** the Status tab showed claims and `in-progress.md` entries on one row.
- **`test-failures` skill:** `run_tests.py` runs any test command and prints only what failed (cargo, pytest, node:test, jest, vitest, go, phpunit, TAP and a generic fallback). Passing tests are never printed, the exit code is passed through, a non-zero exit with nothing recognised always prints `ERROR` plus the output tail, and the full log is kept (owner-only, newest 20) under `.agent-workspace/test-runs/`. `AGENTS.md`, `WORKFLOW.md` and the `code-cleanup`, `code-review-remediation`, `pull-request`, `spec-implement` and `pm-sprint-implement` skills now route test runs through it, and the installer gitignores `test-runs/`.
- **Active sprint in the Status window:** tandem shows the open or locked sprint with progress, days left, remaining Must Have tasks, a locked marker and an overload warning, in a headline, a Sprint section and a short summary-line token.
- **Fix:** `sprints.py list` and `show` double-counted committed effort (a 5.0-day sprint reported 10.0). The scope table rows are now the single source.
- **Branding:** the product is presented as "tandem (agent-workspace)" in the README, `--help` and the package description. The command and the package name stay `agent-workspace`, and `./run.sh` starts the tandem runtime.
- **Docs:** README screenshots are regenerated from the Rust binary with plain `python3` (no uv) and no longer show a real project name.

## [1.5.0] — 2026-11-01

### CLI Package & tandem

- **Compiled Binary Runtime via PyInstaller onedir:** Shipped standalone, compiled binary distribution of tandem across five platforms (`linux-x86_64`, `linux-arm64`, `darwin-arm64`, `darwin-x86_64`, `windows-x86_64`) delivering ~75ms warm launches with zero external Python runtime requirements.
- **Verified Download and Atomic Installation (`run.sh`):** Added automatic release archive downloading from GitHub Releases, cryptographic `SHA256SUMS` verification, atomic extraction to `${XDG_STATE_HOME:-~/.local/state}/tandem/bin/<version>/`, symlinked `current` management, and automatic pruning of obsolete binary versions.
- **Seamless Python and uv Fallback (`run.sh`):** Gracefully falls back to Python execution via `uv run` if precompiled binaries are unavailable, checksum verification fails, or execution fails smoke tests. `uv` is only required when using the Python runtime fallback or developing from source.
- **Runtime Configuration and Non-Interactive Fetch (`run.sh`):** Added `TANDEM_RUNTIME` (`auto|binary|python`), `TANDEM_RELEASE_BASE` override, and `./run.sh --fetch-runtime` for pre-fetching and verifying the binary runtime without launching tmux.
- **Frozen-Aware Self-Execution (`tandem/src/tandem/selfexec.py`):** Added `is_frozen()`, `self_argv()`, and `self_command()` to dynamically resolve entry point commands, ensuring child processes, statusline quota feeds, status panes, and reload handlers launch directly from the frozen binary executable.
- **Portable Cross-Platform File Locking (`tandem/src/tandem/coordination/filelock.py`):** Replaced POSIX-only `fcntl` in coordination storage with portable file locking supporting Windows `msvcrt.locking` alongside POSIX `fcntl`.
- **Windows Platform Support via MSYS2 / Git Bash (`run.sh`):** Added support for Windows shells (`MINGW*|MSYS*|CYGWIN*`) with MSYS2 `pacman` dependency resolution (`tmux`), `.zip` archive extraction, `tandem.exe` execution, and WSL Linux binary path compatibility.
- **In-App Updater Binary Refresh (`tandem/src/tandem/updater.py`):** In frozen mode, F10 / in-app updates invoke `./run.sh --fetch-runtime`, validate the downloaded binary path, set `TANDEM_SELF`, skip unnecessary `uv sync` calls, and reload panes seamlessly.
- **CI Build Matrix and Automated Release Workflows:** Added reusable GitHub Actions binary build workflow (`build-binary.yml`), CI matrix smoke testing across all 5 operating systems and architectures, and automated tag-triggered release workflow (`release.yml`) generating verified `SHA256SUMS`.

## [1.4.0] — 2026-11-01

### CLI Package

- **BA and PM Sprint Management Skills (`/pm-*`):** Added nine first-class sprint planning and governance skills:
  - `/pm-new-sprint`: Scaffolds a complete 1-week sprint deliverable directory (`sprints/s-###-<slug>/`) with dual RICE (`Reach`, `Impact`, `Confidence`, `Effort`) and MoSCoW prioritization, capacity analysis (`sprints.py discover`), `sprint.md`, and three self-contained stakeholder documents (BRD, FRD, and FSD) rendered in semantic HTML.
  - `/pm-sprint-explain`: Provides clear, non-technical explanations of sprint value, business goals, scope, and MoSCoW deliverables.
  - `/pm-sprint-lock`: Locks active sprint scope to enforce commitment discipline; prevents ad-hoc scope creep.
  - `/pm-sprint-unlock`: Unlocks a sprint with mandatory user confirmation and rationale logging.
  - `/pm-sprint-close`: Formally closes a completed sprint, evaluates deliverables, and optionally carries incomplete work over to the product backlog.
  - `/pm-sprint-implement`: Coordinates autonomous multi-agent sprint execution across phases in MoSCoW priority order without collisions using atomic `coord.py claim` locking, worktree isolation, and single-writer plan safety.
  - `/pm-sprint-status`: Real-time sprint progress dashboard tracking live agent claims, completed deliverables, remaining tasks, and sequential vs parallel-wave ETAs.
  - `/pm-sprint-list`: Lists all historical, active, and planned sprints with capacity and status.
  - `/pm-commands`: Catalogs all available PM and sprint commands with contextual availability.
- **Deterministic Python Sprints Engine (`sprints.py`):** Shipped stdlib-only, zero-`hashlib` CLI engine (`templates/skills/pm-new-sprint/scripts/sprints.py`) powering all sprint lifecycle operations, RICE/MoSCoW calculations, collision-free work selection (`next-work`), and deliverable linting.
- **`project-manager` Specialist Agent:** Added the canonical 13th specialist agent role (`templates/agents/project-manager/AGENT.md`) with provider model mappings in `src/data/models.json` (`claude`: `claude-sonnet-4-6`, `antigravity`: `gemini-2.5-pro`, `codex`: `gpt-5.4`). Updated `business-analyst` and `tech-lead-orchestrator` with realistic, non-optimistic sprint routing.
- **First-Class Workspace Sprints Directory (`sprints/`):** Workspace initialization and updates now scaffold `sprints/` with `sprints/README.md`. Sprints are protected from workspace cleanup in `scan_workspace.py`.
- **Sprint-Aware Spec Workflows:**
  - `/spec-planner`: Supports `/spec-planning` alias, evaluates 1-week AI capacity (`> 5.0` days or `> 12` tasks triggers splitting into `/pm-new-sprint`), and integrates spec phases into active sprints via `sprints.py add-phase` (with locked sprint hotfix protection).
  - `/spec-implement`: Default no-argument invocation queries `sprints.py next-work` to automatically claim and implement the highest-priority pending task in the active sprint not currently in development, falling back to `specs.py resolve` when no active sprint exists.
- **Strict Project Boundary & Scope Isolation Guideline (`templates/AGENTS.md`, `templates/WORKFLOW.md`, `templates/knowledgebase/`, skills, and agents):** Enforced that an agent workspace belongs strictly to a specific project. Agents must not cross project boundaries into parent (`..`) or sibling directories to search for, modify, commit to, or create pull requests for unlinked projects unless explicitly linked under `codebase-symlinks/`. Added Principle 9 ("Project Scope & Boundary Isolation") to `templates/AGENTS.md`, reinforced scope checks in `pull-request` and `code-review-remediation`, and added boundary mandates across specialist agent roles.

## [1.3.1] — 2026-10-31

### tandem

- **Fixed agent tab switching:** Clicking a column title no longer fails with `click-tab ... returned 2`. tmux reports no click column on a pane border, so clicking a column title now opens a menu of that column's agents (the visible one is marked `▶`, plus **New agent**) and switches to the one you pick. Keyboard tab switching, drag-to-resize, and the other mouse actions are unchanged.

## [1.3.0] — 2026-10-29

### CLI Package

- **Auto-update on launch (`run.sh`):** `run.sh` checks for a newer release before doing anything else. When one is found it applies the update (`npx --yes <spec> update --yes --no-link --no-update-clis --no-commit`), prints the old → new version and a "What's new" block from the changelog, writes `.agent-workspace/update-notice.env`, then re-execs the updated `run.sh`. Offline, timeout, missing `npx`, or a failed update never block launch; `--check` reports without applying. Pass `--no-update` for a single-launch opt-out; set `TANDEM_AUTO_UPDATE=0` to disable permanently. `TANDEM_UPDATE_SPEC=owner/repo[#ref]` selects an alternate source.
- **Python selection and per-workspace virtualenv isolation (`run.sh`, `tandem/src/tandem/updater.py`):** `run.sh` automatically probes and selects a healthy Python interpreter across `PATH` and standard system paths (`/opt/homebrew/bin/python3`, `/usr/local/bin/python3`, `/usr/bin/python3`) that has a functional `hashlib` with `blake2b`, `blake2s`, and `sha256`, preventing noisy tracebacks from pyenv/OpenSSL builds on startup. It isolates the tandem editable virtualenv per workspace (`~/.local/state/tandem/venv-<workspace>`) so workspaces do not clobber each other's environments, and automatically removes and rebuilds broken virtualenvs. In-app update (`updater.py`) synchronizes directly into the active virtualenv.
- **Zero-`hashlib` coordination script (`coord.py`):** Removed `import secrets` and switched token generation to `os.urandom(4).hex()`, ensuring `coord.py` and all skill scripts start cleanly without touching Python's `hashlib` or triggering OpenSSL warnings.
- **Universal stale skills and agents cleanup:** `update` and `sync` prune deleted templates across all providers (`.agents/skills`, `.claude/skills`, and legacy `.gemini/skills`). Files removed from a template skill are cleaned up within existing skill directories (preserving `.bak` files). Stale agent definitions removed from templates are pruned from `.agents/agents` and provider directories (`.claude/agents`, etc.). Removed files and directories are pruned from `manifest.managedFiles`.
- **`doctor` stale-skill and stale-agent checks:** `doctor` reports stale skills and agents in `.agents/skills` and provider directories, and flags any existing `.gemini/skills` directory as an error.
- **Tool and dependency minimums raised (`min-versions.env`):** tmux minimum raised to 3.4 (required for `range=user` mouse bindings); `textual`, `pytest`, and `mypy` floors raised to current stable releases. Package and tandem versions unified at `1.3.0`.

### tandem

- **What's new notice after an update (`notice.py`, `status/app.py`):** After a launch-time or in-app update, tandem reads `.agent-workspace/update-notice.env` and shows a "What's new in tandem X.Y.Z" popup on the next attach plus a dismissible banner in the Status window. Clicking the banner text opens the popup; `[×]` dismisses it. Both update paths write the same notice so the experience is identical.
- **In-session update check (`update_check.py`, `status/app.py`):** The Status window checks every 4 hours (`TANDEM_UPDATE_INTERVAL`, minimum 600 s; opt out with `TANDEM_UPDATE_CHECK=0`) in a background thread. When a newer release is found, a non-blocking offer banner (`[Update now] [Later] [Skip this version]`) appears plus a single tmux message. Nothing applies without a click or `Alt+u` / `Opt+u`. A failed check still stamps the last-check time. Check state lives in `.agent-workspace/update-check.env`.
- **Clickable status-bar buttons (`tmux/keys.py`, `tmux/layout.py`):** Every hint in the bottom status bar is a clickable button (`range=user` in tmux's status-format; ≤ 15-byte action names). Clicking a hint is equivalent to pressing its keyboard shortcut.
- **Right-click pane menu (`tmux/keys.py`):** Right-clicking any pane opens a tandem context menu (focus, restart if exited, close, new agent, status, queries, resize, help), replacing tmux's default right-click menus.
- **Clickable agent tabs (`tmux/tabs.py`):** Clicking an agent's tab in a column title switches to that agent; clicking "No planners yet…" opens the add-agent wizard. Drag-to-resize on the border is unchanged.
- **Clickable footers and buttons (`status/app.py`, `queries_tab.py`, `wizard/app.py`):** Status window and launch wizard use a Textual `Footer` with OS-aware `Opt`/`Alt` labels. The Queries tab has compact `Send`, `Withdraw`, `Go to agent`, and `Cancel` buttons; numbered answer options are clickable and fill the answer box.
- **OS-aware modifier display and native macOS Option bindings (`tmux/keys.py`, `status/app.py`, `wizard/app.py`):** Tandem renders shortcuts using `Opt` on macOS (`darwin`) and `Alt` on Linux across the status bar hint, Help window, placeholder panes, Status window, Queries tab, and Launch wizard. `parse_shortcut()` accepts `Opt`, `Option`, `⌥`, `⌃`, and `⇧` so macOS configurations in `TANDEM_KEYS` parse naturally. On macOS terminals where "Use Option as Meta" is disabled, tandem binds native macOS Option characters (`˙`, `ß`, `œ`, `¡`, `™`, `©`, `®`, `∑`, `ƒ`, `å`, `∂`) across tmux and Textual.
- **Tandem shortcut and shell quoting fixes:**
  - Fall back to `display-message -p "#{pane_id}"` for `Ctrl+Opt+r` / `Ctrl+Alt+r` (`restart`) when running inside tmux `run-shell` where `TMUX_PANE` is empty, and unset `TMUX_PANE` in tmux configuration to avoid leaking outer tmux session pane IDs.
  - Wrap `run-shell -b` and `display-popup` commands in double quotes so workspace and Python paths containing spaces are cleanly quoted without bash syntax errors.
  - Fixed focus switching in `StatusApp`: `Opt+q`/`Alt+q` focuses `#q-list` immediately, and `Opt+s`/`Alt+s` returns to the Status tab and focuses `#sessions`.
  - Added `?` and `Opt+h`/`Alt+h` help popup support in `WizardApp`.
  - Cleaned up duplicate headers and documented all window, wizard, resize, and customization shortcuts in the expanded Help popup (`Opt+h`/`Alt+h`).
- **Modifier key requirements and multi-modifier chords:** All tandem keyboard shortcuts now require at least one modifier key (`Ctrl`, `Alt`, `Shift`) with support for multi-modifier combinations (`Ctrl+Alt+<key>`). Bare function keys (`F1`–`F12`) are replaced with `Alt+<key>` and `Ctrl+Alt+<key>` shortcuts (e.g. `Alt+h` for help, `Ctrl+Alt+n` for add-agent, `Alt+s` for status window, `Alt+q` for queries tab, `Ctrl+Alt+w` for status window focus, `Ctrl+Alt+x` for quit menu, `Ctrl+Alt+r` for reset layout, `Ctrl+Alt+z` for reflow).
- Enabled `set-option -g extended-keys on` in tmux layout to ensure terminal pass-through of multi-modifier combinations.
- Updated Textual app shortcut keys and action bindings across `StatusApp`, `QueriesTab`, and `WizardApp`.
- Updated SVG screenshots and documentation across the README and tandem README.

## [1.2.0] — 2026-10-21

### CLI Package

- `update` now offers to update the AI CLIs in your workspace (`claude`, `agy`, and `codex` when it is a configured provider; `claude` and `agy` are always checked because tandem launches them). Only installed CLIs are considered; nothing is installed and `sudo` is never used. Each CLI is updated with its own documented command (`claude update`, `agy update`, `npm install -g @openai/codex@latest` or `brew upgrade --cask codex`); a command that would need `sudo` or a piped install script is shown for you to run instead. One question, default No: `--update-clis` or `--yes` applies, `--no-update-clis` skips, `--dry-run` only lists, and with no terminal it prints how to apply. `DISABLE_UPDATES=1` is respected for claude. A failed CLI update is reported and never fails `update`; a CLI below tandem's minimum version gets a warning. `--json` adds an `aiClis` record. `AGENT_WORKSPACE_INTERACTIVE=1` reads the answer from piped input.

### tandem

- After an update, tandem lists the agents still running an older CLI version and offers to restart them with their conversations resumed. Agents with no conversation to reopen, and agents holding a fresh claim, are not selected by default; without a terminal tandem only lists them. The update (`F10` → Update) now also updates the installed AI CLIs. Each pane records the CLI version it started with (`@tandem_cli_version`).

## [1.1.0] — 2026-10-21

### CI

- Added a GitHub Actions workflow: `npm test` on Node 20, and `mypy` plus `pytest` for tandem on Python (with tmux installed), for every pull request and push to `trunk`.

### tandem

- **Resume conversations (AI state):** saved sessions now carry each agent's conversation ID, so resuming reopens the same chats. Claude agents start with `--session-id <uuid>` and resume with `--resume <uuid>`. The ID follows `/clear` and `/resume` through the `session_id` Claude Code passes to the statusline feed. A session with no transcript yet starts fresh under the same ID. `agy` agents write their log to a per-agent `--log-file`; tandem takes the latest conversation ID from it and resumes with `--conversation <id>`. tandem never reads a conversation's contents.
- **Crash recovery:** the session is autosaved on start, on add/close agent, and every 15 s from the status window. If the last save was an autosave, the launch prompt says tandem did not exit cleanly and still offers resume or a blank start (new conversations).
- `F11` (restart an exited agent) reopens its conversation instead of starting a new one.
- `F10` → *Kill session without saving* is now *Quit and discard saved state*: it deletes `tandem-session.json`, so the next launch starts fresh.
- `tandem-session.json` is now version 2 (adds `conversation` per agent and `clean`). Version 1 files still load.

### CLI Package

- Fixed: `update` replaced knowledgebase notes you had edited (for example `context-history/key-decisions.md`, `best-practices/lessons-learned.md`, `guidelines/README.md`) with the shipped template, leaving only a `.bak`. It now only adds notes that are missing and leaves existing ones alone (`↷ … yours; not overwritten`). The guideline documents in `knowledgebase/guidelines/` are still refreshed, with a `.bak` of local edits.

### Base Skills

- Added four commands, each read-only except `/code-cleanup`:
  - `/spec-explain [phase]` explains a phase from its specs: what it fixes, what it develops, why, where, and where it stands.
  - `/explain <module or feature> [--non-technical]` explains how part of a codebase works, with `file:line` references and a diagram, or in plain language. Code is found with the new `locate.py`, which searches the repo and the linked codebases and skips secrets, binaries, and vendored files.
  - `/feasibility <feature>` decides whether something can be built here and how: verdict, effort, options with a recommendation, risks, and unknowns. `locate.py stack` reports the languages, frameworks, and test runners it relies on. Third-party terms and ban risk count as hard blockers.
  - `/code-cleanup [paths] [--diff <base>]` lists unnecessary comments (commented-out code, restating or narrating comments, dividers, empty comments, TODOs with no owner), asks which to delete, then removes only the approved ones. Documentation-generator comments, directives, and license headers are never listed. Deletion refuses files that changed since the scan and runs the tests afterwards.
- Script-first skills: repetitive steps now run as stdlib Python scripts with JSON output, with the manual steps kept as a fallback. `spec-planner/scripts/specs.py` reads and writes specs (`list`, `show`, `resolve`, `new-phase`, `new-task`, `lint`, `set-status`, `normalize`, `compile`, `waves`). `workspace-cleanup/scripts/scan_workspace.py` produces the candidate list and deletes only approved, unchanged, unprotected paths.
- Template-driven skills: the planner's phase, task, and delivery templates, the spec-compile output, the pull request description, the knowledgebase entries, and the new commands' chat layouts moved into each skill's `assets/`. Script-filled fields use `{{field}}`; the agent fills the `[...]` placeholders, and `specs.py lint` fails while any is left.
- `spec-implement` plans parallel waves with `specs.py waves` (dependencies, overlapping Affected Paths, shared hot files, gates, unknown footprints) and runs each wave in parallel through the CLI's own subagents with one worktree per task and ordered, tested merges, or sequentially when subagents are unavailable. Status changes go through `specs.py set-status`, which keeps `t-###.md` and `impl.md` in sync and refuses `Done` until objectives, tests, and security are checked.
- The skill scripts (`specs.py`, `locate.py`, `comments.py`, `scan_workspace.py`, and the existing `coord.py`) now start on Python 3.9, such as macOS's system `python3`; before, they failed with a `TypeError` on startup.
- `spec-implement` and the delivery task now run `/code-cleanup --diff <merge-target>` instead of an unguided comment cleanup pass. `spec-planner` adds a generated `## Parallel Waves` section to `impl.md`.

### Base Files

- Added best-practice notes: deterministic skill scripts, parallel spec execution, and comment classification.

## [2026-10-23]

### Docs

- README: screenshots of tandem (overview, Status and Queries tabs, help) rendered from a fictional demo project; `docs/screenshots/` holds the generator.

### tandem

- Fixed: the status window's border title was tmux's default pane title (the machine's hostname). It now reads `STATUS`.
- Fixed: agent rows in the status window repeated the CLI (`agy·agy·gemini…`).
- Singular counts ("1 agent", "1 worktree", "1 open query").

## [2026-10-22]

### tandem

- Fixed: the help popup (and every other tandem window) began with `ERROR:root:code for hash blake2b was not found` and a traceback on Pythons built against a limited OpenSSL. tandem no longer imports `hashlib` (a test enforces it). The per-workspace tmux socket name is now derived differently, so a session started with the previous build is not reused; end it with `tmux -L <socket> kill-server` or just detach.

## [2026-10-21]

### tandem

- Fixed: starting tandem in a second project attached to the first project's session. Every workspace now gets its own tmux server (`tandem-<hash of the workspace path>`), so projects run side by side. The status and placeholder windows are pinned to their workspace.
- A session started by an older version still lives on the shared `tandem` socket and is not reused; tandem prints a note with the command to end it (`tmux -L tandem kill-server`).

## [2026-10-20]

### CLI Package

- `init` asks whether to create a blank local git repo when the folder isn't one (`--git` / `--no-git` skip the question). It never nests a repo inside another.
- `update` offers to commit only the files the update changed (`--commit` / `--no-commit`); other uncommitted work and `.bak` files are left alone. `--yes` alone never commits.
- New `.agent-workspace/.gitignore` keeps `tandem-session.json`, `coordination/` and backups out of git.
- spec-planner / spec-implement: when the agent workspace is itself a git repo, the delivery task also commits (and, with a remote, PRs and merges) the workspace's own changes.

## [2026-10-19]

### tandem

- Quota line under the window (second tmux status row). Claude: fed by Claude Code's own documented statusline data (`rate_limits` 5-hour and 7-day), via `--settings`, chaining to your existing statusline. Antigravity and anything else: `quota not available`. No pane reading, no token use, no usage endpoints.

## [2026-10-18]

### tandem

- **Save and resume:** `F10` → *Save state and exit* writes `.agent-workspace/tandem-session.json` (owner-only JSON: agents' CLI, role, model, tab number, folder, visible tabs, pane proportions) and ends the session. The next launch asks to resume or start blank; `--resume` / `--fresh` skip the question. Conversations are not restored (each CLI has its own resume). Missing CLIs are skipped and deleted folders fall back to the workspace.
- **Update without quitting:** `F10` → *Update tandem and workspace* (or `tandem update [--yes]`) saves state, runs `npx --yes hydrokat/agent-workspace update --yes --no-link`, syncs tandem's Python environment, and restarts only tandem's own windows and key bindings. Agents keep running. A failed step restarts nothing. `TANDEM_UPDATE_SPEC=owner/repo[#ref]` selects another source.
- The `F10` menu now offers save and exit, detach, update, and kill (no save).

## [2026-10-17]

### tandem

- Status window: new **Repositories** section (expanded by default) lists the workspace and every codebase in `codebase-symlinks/`, one header per repo with each branch below it: `* branch` for the main checkout, `branch → ../worktree` for a branch active in a linked worktree, then idle branches (capped at five). Rows shorten to the window width and re-render on resize. Replaces the collapsed Codebases/Worktrees/Branches sections, which only described the workspace repo.
- Fixed: branch names containing `/` were truncated in worktree listings; `codebase-map.md` inside `codebase-symlinks/` was shown as a missing codebase.
- Keyboard resize: `F12` enters resize mode (arrows resize the focused pane, `=` resets, `Enter`/`Esc` leaves); mouse border-drag still works.
- Responsive: the status window is clamped to 40% of a short terminal on resize; new `tandem reset-layout`.

## [2026-10-16]

### CLI Package

- Added `run.sh`: checks `git`, `tmux`, `uv`, `claude`, and `agy`, asks before every install or update, enforces the minimum versions in `tandem/min-versions.env`, then launches tandem. Supports `--check`, `--yes`, `--no`.
- Added **tandem** (`tandem/`, Python + uv): launches `claude` / `agy` side by side in tmux as planners (left) and implementers (right), with a Textual status window (Status and Queries tabs), per-column agent tabs, a launch wizard, and an F-key keymap. It only spawns the official CLIs; it never sends keys to or reads from their panes.
- `package.json` `files` now ships `run.sh` and the tandem sources. `init` and `update` now install `run.sh` (workspace root, executable) and the tandem project (`.agent-workspace/tandem/`), so `./run.sh` works in any workspace. A modified `run.sh` is backed up to `run.sh.bak`. They still do not require Python; only `run.sh` does.
- `run.sh` prefers `.agent-workspace/tandem/` and falls back to a sibling `tandem/` (package checkout).

### Base Skills

- Added `agent-coordination` (with stdlib `scripts/coord.py`): session registry, atomic task claims, heartbeats, and non-blocking agent questions stored under `.agent-workspace/coordination/`.
- Enhanced `spec-planner`: new "Check Active & Planned Work" step; task template gains `## Affected Paths` and `## Depends On`.
- Enhanced `spec-implement`: claims a task before working, uses a worktree when another implementer is active, heartbeats and checks answers at milestones, blocks only the affected objective, and releases the claim afterwards. Both skills fall back to `in-progress.md` when coordination is unavailable.

### Base Files

- `AGENTS.md` §8 gains the multi-agent coordination principle and the session-start `whoami` rule.
- `specs/in-progress.md` entries gain optional Session and Claim fields.
- Added best-practice notes: ToS-safe orchestration, non-blocking agent questions, and testing bash with a stubbed PATH.

## [2026-10-10]

### Base Skills

- Enhanced `code-review-remediation` into a full PR loop: pull open PRs/MRs (all relevant repositories unless one is named), review, comment, remediate, and repeat until all clear. It now bundles the code review rubric (`references/review-criteria.md`), runs CI locally first and posts evidence when remote CI fails but local passes, produces a merge report, asks before merging, and replies only `Merged` after a successful merge.

## [2026-10-09]

### Base Skills

- Enhanced `specs-planner` to create new phases as `p-###-[name-of-repo]`, use `p-###-multi` for multi-repo phases, create task files as `t-###.md`, and record involved repositories in `impl.md`.
- Updated `spec-implement` and `spec-compile` to read the new `p-###-*` phase directories and `t-###.md` task files while preserving read-only support for existing legacy specs.

### Base Files

- Updated workspace, specs, progress tracking, compiled-output, and package documentation to describe the new repo-scoped phase and task naming conventions.

## [2026-09-22]

### Base Skills

- Enhanced `spec-implement` to use the `caveman` skill whenever possible during spec execution and to document exceptions when terse mode is unavailable or inappropriate.
- Added strict `spec-implement` code-comment discipline: source comments are for durable documentation only, with implementation reasoning kept in task files, plans, commits, or knowledgebase entries.

### Base Files

- Updated package documentation to describe spec implementation's caveman-mode execution and code-comment cleanup expectations.

## [2026-09-02]

### Base Skills

- Enhanced `specs-planner` to require low-fidelity UI mockups for frontend/UI phases and tasks, including responsive variants, important states, interaction notes, and accessibility notes.

### Base Files

- Updated specs documentation to remind agents to include UI mockups before frontend/UI implementation starts.

## [2026-08-16]

### Base Skills

- Enhanced `specs-planner` to prompt for a working branch and merge target before planning, record the selected branch naming pattern for future phases, and always include a final delivery task covering commit, PR creation, code review/remediation loops, pipeline fix loops, and merge.

## [2026-07-16]

### Base Files

- Enhanced `specs/in-progress.md` tracking to be multi-agent aware: agents must explicitly record their role, ID, and working branch/worktree to prevent collisions when running concurrently.
- Updated `AGENTS.md` Progress Tracking rules and `specs/in-progress.md` template to enforce explicitly declaring the agent context.

## [2026-07-03]

### Base Files

- Added a Branch Protection operating principle to `AGENTS.md`: contributors must never commit or make changes directly on `main`, `master`, or `trunk` unless the user explicitly instructs it, and must use a worktree or a new branch before implementing.
- Added a Concurrent Agent Awareness operating principle to `AGENTS.md`: contributors must not assume exclusive access to the codebase — check for other agents' in-progress work via `git status`, task files, and branches/worktrees before starting, and coordinate instead of overwriting.
- Added a Progress Tracking rule to `AGENTS.md`'s Documentation Sync section and a new `specs/in-progress.md` starter file, scaffolded by `init`: agents must list every active task or spec (labeled `adhoc` if it has no spec) with its ongoing status and next step, and remove entries as soon as the task is complete.

### Base Skills

- Updated `spec-implement` to create or switch to a feature branch before implementing (never directly on `main`, `master`, or `trunk`), reusing an existing non-protected branch when already checked out on one.
- Added `workspace-cleanup` skill: scans the workspace for files and directories that may not belong (OS/editor cruft, stale `.bak` backups, orphaned generated provider output, empty directories, stale `.tmp/` scratch files, and other unrecognized files), lists every candidate with a reason, and asks the user which ones to delete before removing anything. Trigger with `/workspace-cleanup`.

## [2026-07-02]

### Base Skills

- Added `code-review` skill: production-grade PR/diff review covering correctness, business flow, architecture, security, performance, database safety, API contracts, tests, deployment risk, and blast radius, with a structured findings-first output and an approval/changes-requested verdict.
- Added `code-review-remediation` skill: takes a pull request from reviewer feedback to a validated remediation commit — gathers unresolved review threads, validates reviewer intent before changing code, implements scoped fixes, reconciles with the base branch, and reports outcomes.
- Added `pull-request` skill: turns completed implementation work into a PR — branch protection checks, atomic commits, validation, a structured PR description template, and a final review checklist.

### Base Files

- Added `.tmp/` as a scaffolded workspace directory for agent working files. It is created by `init`, safe to delete at any time, and documented in `AGENTS.md`'s Core Directory Map.

## [2026-06-04]

### CLI Package

- Introduced `agent-workspace` CLI (`npx hydrokat/agent-workspace`).
- Added `init` command: scaffolds directory structure, installs agents/skills/guidelines from bundled templates, generates provider outputs, optionally links sibling codebases, runs `doctor`, and writes `.agent-workspace/manifest.json`.
- Added `update` command: refreshes managed library files (with `.bak` backups for local edits), refreshes the `AGENTS.md` managed block, fully regenerates provider outputs, never touches project content (`specs/`, `knowledgebase/business-flows/`, user entries).
- Added `doctor` command: validates required dirs, per-provider agent files, frontmatter (`name`, `description`, `model`), model correctness against `models.json`, compat doc symlinks, and codebase symlink resolution. Exits non-zero on failure.
- Added provider adapters for Claude, Gemini, and Codex with per-adapter agent transforms and model injection.
- Added `src/data/models.json` encoding the agent × provider model map sourced from the `AGENTS.md` recommendations table.
- Added `templates/` as the bundled canonical payload (12 agents, 5 skills, knowledgebase starters).
- Ported `link_codebases.py` to JS (`src/core/link-codebases.js`); the package now requires only Node ≥ 18.
- Reduced `workspace-init` and `workspace-sync` skill files to thin wrappers that invoke the CLI.
- Added 22 smoke tests (`test/`) covering init, update, doctor, CLI flags, backup contract, and no-clobber guarantees.

## [2026-05-05]

### Base Files

- Added an Agent Model Recommendations section to `AGENTS.md` for cross-provider role selection guidance.
- Refreshed the Codex/OpenAI recommendations in `AGENTS.md` to use current, role-appropriate model IDs: `gpt-5.5`, `gpt-5.3-codex`, `gpt-5.4`, and `gpt-5.4-mini` instead of stale `o3`, `GPT-4.1`, and `codex-mini` references.

## [2026-04-24]

### Base Files

- Reframed the workspace around vendor-agnostic base sources of truth using `AGENTS.md`, `.agents/agents/`, and `.agents/skills/`.
- Updated `AGENTS.md` with research-first rules, skill-first execution, compatibility sync policy, and base-file precedence.
- Added `WORKFLOW.md` as the shared execution policy for development and orchestration behavior.
- Added `README.md` with setup, initialization, usage, sync, and contribution guidance for humans and agents.
- Neutralized workspace documentation so it describes a reusable project workspace rather than a Gemini-specific agent environment.
- Updated knowledgebase and specs documentation to align with the vendor-agnostic workspace model.

### Base Skills

- Added `workspace-init` to bootstrap a new workspace with starter directories and files.
- Added `workspace-sync` to regenerate Gemini, Claude, and Codex compatibility outputs from base files.
- Updated `workspace-sync` to sync both agents and skills from the base workspace.
- Hardened `workspace-sync` so it preserves base files and avoids overwriting existing non-generated compatibility documents.
- Renamed `knowledgebase-manager` to `knowledge`.
- Updated `knowledge` so it can be triggered with `/knowledge` and so it reviews and updates `guidelines`, `best-practices`, `business-flows`, and `context-history` as needed.
- Updated `knowledge` to require collaboration between `business-analyst` and `technical-writer` for meaningful knowledgebase maintenance.
- Added `find-skills` skill to find skills using the skills MP website.

### Base Agents

- Added a vendor-agnostic agent catalog under `.agents/agents/`.
- Added role definitions for `codebase_investigator`, `generalist`, `business-analyst`, `code-reviewer`, `frontend-dev`, `qa-engineer`, `security-auditor`, `senior-backend-dev`, `tech-lead-orchestrator`, `technical-writer`, and `terminal-agent`.
- Updated `tech-lead-orchestrator` to require `WORKFLOW.md`, explicit task-to-agent mapping, and mandatory pre-report and post-report outputs.
- Added `api-pentester`

## Changelog Rules

- Record only changes to base files, base skills, and base agents.
- Do not log regenerated `.gemini/` or `.claude/` outputs unless the underlying source behavior changed.
- Prefer concise, high-signal entries that describe what changed and why it matters.
