import json
import os
import shutil
import signal
import sys
import subprocess
import time
from collections.abc import Callable
from dataclasses import replace
from pathlib import Path

from tandem.adapters import adapter_for, session_env, session_id
from tandem.adapters.base import AgentSpec
from tandem.config import Config
from tandem.coordination.schema import STALE_SECONDS
from tandem.coordination.store import CoordStore
import re

from tandem import conversations, quota, state, updater, versions
from tandem.tmux import keys
from tandem.tmux.client import TmuxClient, TmuxError, socket_for
from tandem.tmux.layout import SESSION, Launch, Layout, ROLES
from tandem.tmux.tabs import PaneInfo

cli_version: Callable[[str], str] = versions.installed_version


CLOSING = "@tandem_closing"


def agent_launch(spec: AgentSpec, coord_dir: str, workspace: Path, resume: bool = False,
                 claude_dir: Path | None = None) -> Launch:
    """``resume`` reopens the agent's saved conversation when its CLI still has it; otherwise a new chat starts."""
    if spec.cli == "claude" and spec.conversation is None:
        spec = replace(spec, conversation=conversations.new_id())
    reopen = resume and conversations.can_resume(spec.cli, spec.conversation, claude_dir or Path.home() / ".claude")
    if spec.cli == "agy" and not reopen:
        spec = replace(spec, conversation=None)
    coord = Path(coord_dir)
    log_file = str(conversations.agy_log_path(coord, session_id(spec))) if spec.cli == "agy" else None
    if not reopen:
        conversations.forget(coord, session_id(spec))
    return Launch(
        argv=adapter_for(spec.cli).build_argv(spec, resume=reopen, log_file=log_file), role=spec.role,
        index=spec.index, label=f"{spec.cli}·{spec.model}", env=session_env(spec, coord_dir),
        cwd=spec.cwd or str(workspace), session_id=session_id(spec), cli_version=cli_version(spec.cli),
        spec=json.dumps({"cli": spec.cli, "model": spec.model, "skip_permissions": spec.skip_permissions,
                         "cwd": spec.cwd, "conversation": spec.conversation}),
    )


def _own_env(workspace: Path | None) -> dict[str, str]:
    return {"TANDEM_WORKSPACE": str(workspace)} if workspace else {}


def placeholder_launch(role: str, python: str = sys.executable, workspace: Path | None = None) -> Launch:
    return Launch([python, "-m", "tandem", "placeholder", role], role, 0, "placeholder", kind="own",
                  env=_own_env(workspace))


def status_launch(python: str = sys.executable, workspace: Path | None = None) -> Launch:
    return Launch([python, "-m", "tandem", "status"], "status", 0, "status", kind="own", env=_own_env(workspace))


def coord_dir_for(workspace: Path, config: Config) -> Path:
    return config.coord_dir or workspace / ".agent-workspace" / "coordination"


def make_layout(workspace: Path, config: Config) -> Layout:
    conf = config.state_dir / "tandem.tmux.conf"
    socket = socket_for(workspace)
    layout = Layout(TmuxClient(socket=socket), coord=CoordStore(coord_dir_for(workspace, config)),
                    workspace=str(workspace))
    layout.write_conf(conf)
    layout.client = TmuxClient(socket=socket, conf=conf)
    return layout


def start(specs: list[AgentSpec], workspace: Path, config: Config, saved: state.SavedState | None = None) -> None:
    """Create the tmux session for ``specs`` and attach to it (replaces this process)."""
    layout = make_layout(workspace, config)
    coord_dir = str(coord_dir_for(workspace, config))
    conversations.prepare(Path(coord_dir))
    resume = saved is not None

    def launch(spec: AgentSpec) -> Launch:
        return agent_launch(spec, coord_dir, workspace, resume=resume, claude_dir=config.claude_dir)

    by_role = {role: [s for s in specs if s.role == role] for role in ROLES}
    first = {role: (launch(group[0]) if group else placeholder_launch(role, workspace=workspace))
             for role, group in by_role.items()}
    size = shutil.get_terminal_size((160, 45))
    layout.build(size.columns, size.lines, first["planner"], first["implementer"], status_launch(workspace=workspace))
    for role in ROLES:
        for spec in by_role[role][1:]:
            layout.add_agent(launch(spec), activate=False)
    if saved is not None:
        for role, index in saved.active.items():
            layout.select(role, index)
        layout.apply_sizes(saved.columns_percent, saved.status_rows)
        layout.focus("planner" if by_role["planner"] else "implementer")
    autosave(layout, workspace)
    attach(workspace)


def attach(workspace: Path) -> None:
    os.execvp("tmux", ["tmux", "-L", socket_for(workspace), "attach-session", "-t", SESSION])


def add_agent(spec: AgentSpec, workspace: Path, config: Config) -> None:
    layout = make_layout(workspace, config)
    panes = layout.panes()
    same_role = [p.index for p in panes if p.role == spec.role and p.kind == "agent"]
    indexed = AgentSpec(spec.cli, spec.role, spec.model, max(same_role, default=0) + 1,
                        spec.skip_permissions, spec.cwd)
    coord_dir = coord_dir_for(workspace, config)
    conversations.prepare(coord_dir)
    layout.add_agent(agent_launch(indexed, str(coord_dir), workspace))
    autosave(layout, workspace)


def _own_role(layout: Layout, config: Config) -> str:
    args = ["-p"] + (["-t", config.tmux_pane] if config.tmux_pane else []) + ["#{@tandem_role}"]
    return layout.client.run_checked("display-message", *args).strip()


def tab(layout: Layout, config: Config, direction: str, number: int | None = None) -> None:
    role = _own_role(layout, config)
    if role not in ROLES:
        return
    if direction == "select" and number is not None:
        layout.select(role, number)
    else:
        layout.step(role, 1 if direction == "next" else -1)


def focus(layout: Layout, config: Config, target: str) -> None:
    if target in ROLES:
        layout.focus(target)
        return
    status = next((p for p in layout.panes() if p.role == "status"), None)
    if status is None:
        return
    layout.client.run_checked("select-pane", "-t", status.id)
    if target == "queries":
        layout.client.send_keys_own(status.id, "M-q")


def close_agent(layout: Layout, config: Config) -> None:
    pane = config.tmux_pane or layout.client.run_checked("display-message", "-p", "#{pane_id}").strip()
    role = _own_role(layout, config)
    if role in ROLES:
        layout.close_agent(pane, placeholder_launch(role, workspace=Path(layout.workspace) if layout.workspace else None))
        if layout.workspace:
            autosave(layout, Path(layout.workspace))


def _pane_spec(pane: PaneInfo, coord_dir: Path | None) -> AgentSpec | None:
    """Rebuild a pane's AgentSpec; the conversation is the one its CLI last reported, else the one it started with."""
    try:
        raw = json.loads(pane.spec)
        cli = raw["cli"]
        live = conversations.current(coord_dir, pane.session, cli) if coord_dir is not None else None
        return AgentSpec(cli, pane.role, raw["model"], pane.index,  # type: ignore[arg-type]
                         bool(raw.get("skip_permissions")), raw.get("cwd"),
                         live or conversations.valid_id(raw.get("conversation")))
    except (ValueError, KeyError, TypeError, AttributeError):
        return None


def capture_state(layout: Layout, clean: bool = True) -> state.SavedState:
    """Snapshot the lineup (with each agent's conversation), visible tabs and proportions of the running session."""
    panes = layout.panes()
    coord_dir = layout.coord.root if layout.coord is not None else None
    agents = [spec for pane in sorted((p for p in panes if p.kind == "agent"), key=lambda p: (ROLES.index(p.role), p.index))
              if (spec := _pane_spec(pane, coord_dir)) is not None]
    active = {role: shown.index for role in ROLES
              if (shown := next((p for p in panes if p.role == role and p.window == "main" and p.kind == "agent"), None))}
    columns, rows = layout.sizes()
    return state.SavedState(tuple(agents), active, columns, rows, time.time(), clean)


def save_state(layout: Layout, workspace: Path, clean: bool = True) -> state.SavedState:
    snapshot = capture_state(layout, clean)
    if not snapshot.agents:
        raise ValueError("no agents to save")
    state.save(state.state_path(workspace), snapshot)
    return snapshot


def _closing(layout: Layout) -> bool:
    return layout.client.run("show-options", "-qv", "-t", SESSION, CLOSING).out.strip() == "1"


def autosave(layout: Layout, workspace: Path) -> bool:
    """Crash-safety save of the running session (``clean=False``); never raises, never runs while it is closing."""
    try:
        if not layout.exists() or _closing(layout):
            return False
        snapshot = capture_state(layout, clean=False)
        if not snapshot.agents or _closing(layout):
            return False
        state.save(state.state_path(workspace), snapshot)
        return True
    except (OSError, ValueError, TmuxError):
        return False


def save_exit(layout: Layout, workspace: Path) -> None:
    """Save the session, then end it. If saving fails the session is left running."""
    layout.client.run_checked("set-option", "-t", SESSION, CLOSING, "1")
    try:
        snapshot = save_state(layout, workspace, clean=True)
    except (OSError, ValueError) as error:
        layout.client.run_checked("set-option", "-u", "-t", SESSION, CLOSING)
        layout.client.run_checked("display-message", f"Not saved, session kept: {error}")
        return
    layout.client.run_checked("display-message", f"Saved {len(snapshot.agents)} agents. Closing.")
    layout.client.run_checked("kill-session", "-t", SESSION)


def discard_exit(layout: Layout, workspace: Path) -> None:
    """End the session and delete the saved state, so the next launch starts fresh without asking."""
    layout.client.run_checked("set-option", "-t", SESSION, CLOSING, "1")
    state.state_path(workspace).unlink(missing_ok=True)
    layout.client.run_checked("kill-session", "-t", SESSION)


def restart(layout: Layout, pane_id: str, workspace: Path, config: Config) -> None:
    """Relaunch an exited agent, reopening its conversation when the CLI has one to reopen."""
    pane = next((p for p in layout.panes() if p.id == pane_id and p.kind == "agent"), None)
    coord_dir = coord_dir_for(workspace, config)
    spec = _pane_spec(pane, coord_dir) if pane is not None else None
    if spec is None:
        layout.restart(pane_id, str(coord_dir))
        return
    conversations.prepare(coord_dir)
    layout.restart(pane_id, str(coord_dir),
                   agent_launch(spec, str(coord_dir), workspace, resume=True, claude_dir=config.claude_dir))


def resume_specs(saved: state.SavedState, workspace: Path,
                 which: Callable[[str], str | None] = shutil.which) -> tuple[list[AgentSpec], list[str]]:
    """Agents to relaunch from ``saved``; skips CLIs that are gone and falls back when a folder was deleted."""
    specs: list[AgentSpec] = []
    notes: list[str] = []
    for a in saved.agents:
        if which(a.cli) is None:
            notes.append(f"Skipped {a.role} {a.index}: {a.cli} is not installed.")
            continue
        if a.cwd is not None and not Path(a.cwd).is_dir():
            notes.append(f"{a.role} {a.index}: {a.cwd} no longer exists, using the workspace folder.")
            # Claude Code keeps conversations per folder, so its chat cannot follow the agent to a new one
            a = replace(a, cwd=None, conversation=None if a.cli == "claude" else a.conversation)
        specs.append(a)
    return specs, notes


def quit_menu(layout: Layout, workspace: str | None = None) -> None:
    launcher = keys.launcher_for(sys.executable, workspace)
    layout.client.run_checked(
        "display-menu", "-T", "tandem",
        "Save state and exit (resume later)", "s", f"run-shell -b '{launcher} save-exit'",
        "Detach (agents keep running)", "d", "detach-client",
        "Update tandem and workspace", "u", f"display-popup -E -w 80% -h 70% '{launcher} update'",
        "Quit and discard saved state (next launch is fresh)", "k", f"run-shell -b '{launcher} discard-exit'",
        "Cancel", "c", "")


def reload_ui(layout: Layout, workspace: Path, config: Config) -> None:
    """Re-apply key bindings/hooks and restart tandem's own windows on the code now on disk."""
    layout.apply_chrome()
    layout.respawn_own({"status": status_launch(workspace=workspace),
                            **{f"placeholder-{r}": placeholder_launch(r, workspace=workspace) for r in ROLES}})
    layout.refresh()


def _reload_in_fresh_process(workspace: Path, layout: Layout) -> bool:
    if not layout.exists():
        return False
    argv = ["env", f"TANDEM_WORKSPACE={workspace}", sys.executable, "-m", "tandem", "reload"]
    return subprocess.run(argv, cwd=workspace, stdin=subprocess.DEVNULL, capture_output=True).returncode == 0


def run_update(workspace: Path, config: Config, assume_yes: bool) -> int:
    """Interactive entry point (the F10 menu opens it in a popup): confirm, update, report."""
    layout = make_layout(workspace, config)
    print("\n".join(["tandem update", "", *updater.plan(workspace, config.update_spec)]))
    if not assume_yes:
        try:
            if input("\nContinue? [y/N] ").strip().lower() not in ("y", "yes"):
                print("Cancelled.")
                return 1
        except EOFError:
            return 1
    if layout.exists():
        try:
            save_state(layout, workspace, clean=False)
            print("Saved current state first.")
        except Exception as error:  # saving is a courtesy, never a blocker
            print(f"(state not saved: {error})")
    result = updater.run_update(workspace, config.update_spec, lambda: _reload_in_fresh_process(workspace, layout))
    print(f"\n{'OK' if result.ok else 'FAILED'}: {result.message}")
    if result.ok and layout.exists():
        try:
            offer_restarts(layout, workspace, config, sys.stdin.isatty())
        except (OSError, ValueError, TmuxError) as error:
            print(f"(could not check running agents: {error})")
    if sys.stdin.isatty():
        input("\nPress Enter to close.")
    return 0 if result.ok else 1


def find_stale(layout: Layout, workspace: Path, config: Config) -> list[updater.StaleAgent]:
    """Running agents that started on an older CLI version than the one installed now."""
    coord_dir = coord_dir_for(workspace, config)
    now = time.time()
    busy = {}
    if layout.coord is not None:
        for claim in layout.coord.claims():
            if now - claim.heartbeat < STALE_SECONDS:
                phase = re.match(r"p-\d+", claim.phase)
                busy[claim.session] = f"{phase.group(0) if phase else claim.phase} {claim.task}"

    def resumable(pane: PaneInfo) -> bool:
        spec = _pane_spec(pane, coord_dir)
        return spec is not None and conversations.can_resume(spec.cli, spec.conversation, config.claude_dir)

    return updater.stale_agents(layout.panes(), cli_version, resumable, busy)


def restart_stale(layout: Layout, workspace: Path, config: Config, chosen: list[updater.StaleAgent]) -> list[str]:
    """Respawn each chosen agent on the new CLI, reopening its conversation. One failure never stops the others."""
    coord_dir = coord_dir_for(workspace, config)
    conversations.prepare(coord_dir)
    panes = {p.id: p for p in layout.panes()}
    messages: list[str] = []
    for agent in chosen:
        name = f"{agent.role} {agent.index}"
        pane = panes.get(agent.pane_id)
        spec = _pane_spec(pane, coord_dir) if pane is not None else None
        if spec is None:
            messages.append(f"{name}: not restarted (its launch details are not available)")
            continue
        try:
            launch = agent_launch(spec, str(coord_dir), workspace, resume=True, claude_dir=config.claude_dir)
            layout.restart_live(agent.pane_id, launch)
            messages.append(f"{name}: restarted on {agent.installed}")
        except (TmuxError, ValueError, OSError) as error:
            messages.append(f"{name}: not restarted ({error})")
    return messages


def offer_restarts(layout: Layout, workspace: Path, config: Config, interactive: bool,
                   ask: Callable[[str], str] = input, out: Callable[[str], None] = print) -> None:
    """After an update: say which agents still run the old CLI and, at a terminal, restart the ones you pick."""
    stale = find_stale(layout, workspace, config)
    if not stale:
        return
    if not interactive:
        out("\nThese agents still run an older CLI (restart them from tandem when convenient):")
        for agent in stale:
            out(f"  {updater.describe_stale(agent)}")
        return
    chosen = updater.choose_restarts(stale, ask, out)
    if not chosen:
        out("No agents restarted.")
        return
    for message in restart_stale(layout, workspace, config, chosen):
        out(message)
    autosave(layout, workspace)


def first_run(layout: Layout, config: Config) -> None:
    marker = config.state_dir / "first-run-done"
    if marker.exists():
        return
    config.state_dir.mkdir(parents=True, exist_ok=True)
    marker.write_text(str(time.time()))
    layout.client.run_checked("display-popup", "-E", "-w", "70%", "-h", "70%", keys.launcher_for(sys.executable, str(config.workspace) if config.workspace else None) + " help")


def show_help() -> None:
    print("\n".join(keys.help_lines()))
    print("\nPress Enter to close.")
    try:
        input()
    except EOFError:
        pass


def placeholder(role: str) -> None:
    add_key = keys.key_for("add-agent") or "Ctrl+Alt+n"
    print(f"No {role}s yet. Press {add_key} to add one.")
    signal.pause()


def quota_feed(config: Config) -> None:
    """Statusline command for Claude panes. Never fails loudly: a broken feed must not break the CLI's statusline."""
    try:
        text = sys.stdin.read(quota.MAX_INPUT + 1)
        coord = config.coord_dir
        out = quota.run_feed(text, coord, config.session_id, Path.cwd(), Path.home(), time.time()) if coord else ""
    except Exception:
        out = ""
    sys.stdout.write(out)


def quota_line(workspace: Path, config: Config) -> None:
    """Text for tmux's second status row; fed by statusline data only, never by reading a pane."""
    layout = make_layout(workspace, config)
    try:
        panes = layout.panes() if layout.exists() else []
    except Exception:
        panes = []
    clis: list[str] = []
    for pane in panes:
        if pane.kind != "agent":
            continue
        try:
            cli = json.loads(pane.spec)["cli"]
        except (ValueError, KeyError, TypeError):
            continue
        if isinstance(cli, str) and cli not in clis:
            clis.append(cli)
    now = time.time()
    claude = quota.read_claude(coord_dir_for(workspace, config)) if "claude" in clis else None
    sys.stdout.write(quota.format_line(clis, claude, now))
