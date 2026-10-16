import json
import os
import shutil
import signal
import sys
import subprocess
import time
from collections.abc import Callable
from pathlib import Path

from tandem.adapters import adapter_for, session_env, session_id
from tandem.adapters.base import AgentSpec
from tandem.config import Config
from tandem.coordination.store import CoordStore
from tandem import quota, state, updater
from tandem.tmux import keys
from tandem.tmux.client import TmuxClient
from tandem.tmux.layout import SESSION, Launch, Layout, ROLES


def agent_launch(spec: AgentSpec, coord_dir: str, workspace: Path) -> Launch:
    return Launch(
        argv=adapter_for(spec.cli).build_argv(spec), role=spec.role, index=spec.index,
        label=f"{spec.cli}·{spec.model}", env=session_env(spec, coord_dir),
        cwd=spec.cwd or str(workspace), session_id=session_id(spec),
        spec=json.dumps({"cli": spec.cli, "model": spec.model, "skip_permissions": spec.skip_permissions,
                         "cwd": spec.cwd}),
    )


def placeholder_launch(role: str, python: str = sys.executable) -> Launch:
    return Launch([python, "-m", "tandem", "placeholder", role], role, 0, "placeholder", kind="own")


def status_launch(python: str = sys.executable) -> Launch:
    return Launch([python, "-m", "tandem", "status"], "status", 0, "status", kind="own")


def coord_dir_for(workspace: Path, config: Config) -> Path:
    return config.coord_dir or workspace / ".agent-workspace" / "coordination"


def make_layout(workspace: Path, config: Config) -> Layout:
    conf = config.state_dir / "tandem.tmux.conf"
    layout = Layout(TmuxClient(), coord=CoordStore(coord_dir_for(workspace, config)), workspace=str(workspace))
    layout.write_conf(conf)
    layout.client = TmuxClient(conf=conf)
    return layout


def start(specs: list[AgentSpec], workspace: Path, config: Config, saved: state.SavedState | None = None) -> None:
    """Create the tmux session for ``specs`` and attach to it (replaces this process)."""
    layout = make_layout(workspace, config)
    coord_dir = str(coord_dir_for(workspace, config))
    by_role = {role: [s for s in specs if s.role == role] for role in ROLES}
    first = {role: (agent_launch(group[0], coord_dir, workspace) if group else placeholder_launch(role))
             for role, group in by_role.items()}
    size = shutil.get_terminal_size((160, 45))
    layout.build(size.columns, size.lines, first["planner"], first["implementer"], status_launch())
    for role in ROLES:
        for spec in by_role[role][1:]:
            layout.add_agent(agent_launch(spec, coord_dir, workspace), activate=False)
    if saved is not None:
        for role, index in saved.active.items():
            layout.select(role, index)
        layout.apply_sizes(saved.columns_percent, saved.status_rows)
        layout.focus("planner" if by_role["planner"] else "implementer")
    attach()


def attach() -> None:
    os.execvp("tmux", ["tmux", "-L", "tandem", "attach-session", "-t", SESSION])


def add_agent(spec: AgentSpec, workspace: Path, config: Config) -> None:
    layout = make_layout(workspace, config)
    panes = layout.panes()
    same_role = [p.index for p in panes if p.role == spec.role and p.kind == "agent"]
    indexed = AgentSpec(spec.cli, spec.role, spec.model, max(same_role, default=0) + 1,
                        spec.skip_permissions, spec.cwd)
    layout.add_agent(agent_launch(indexed, str(coord_dir_for(workspace, config)), workspace))


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
        layout.client.send_keys_own(status.id, "F6")


def close_agent(layout: Layout, config: Config) -> None:
    pane = config.tmux_pane or layout.client.run_checked("display-message", "-p", "#{pane_id}").strip()
    role = _own_role(layout, config)
    if role in ROLES:
        layout.close_agent(pane, placeholder_launch(role))


def capture_state(layout: Layout) -> state.SavedState:
    """Snapshot the lineup, visible tabs and proportions of the running session."""
    panes = layout.panes()
    agents: list[AgentSpec] = []
    for pane in sorted((p for p in panes if p.kind == "agent"), key=lambda p: (ROLES.index(p.role), p.index)):
        try:
            raw = json.loads(pane.spec)
            agents.append(AgentSpec(raw["cli"], pane.role, raw["model"], pane.index,  # type: ignore[arg-type]
                                    bool(raw.get("skip_permissions")), raw.get("cwd")))
        except (ValueError, KeyError, TypeError):
            continue
    active = {role: shown.index for role in ROLES
              if (shown := next((p for p in panes if p.role == role and p.window == "main" and p.kind == "agent"), None))}
    columns, rows = layout.sizes()
    return state.SavedState(tuple(agents), active, columns, rows, time.time())


def save_state(layout: Layout, workspace: Path) -> state.SavedState:
    snapshot = capture_state(layout)
    if not snapshot.agents:
        raise ValueError("no agents to save")
    state.save(state.state_path(workspace), snapshot)
    return snapshot


def save_exit(layout: Layout, workspace: Path) -> None:
    """Save the session, then end it. If saving fails the session is left running."""
    try:
        snapshot = save_state(layout, workspace)
    except (OSError, ValueError) as error:
        layout.client.run_checked("display-message", f"Not saved, session kept: {error}")
        return
    layout.client.run_checked("display-message", f"Saved {len(snapshot.agents)} agents. Closing.")
    layout.client.run_checked("kill-session", "-t", SESSION)


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
            a = AgentSpec(a.cli, a.role, a.model, a.index, a.skip_permissions, None)
        specs.append(a)
    return specs, notes


def quit_menu(layout: Layout, workspace: str | None = None) -> None:
    launcher = keys.launcher_for(sys.executable, workspace)
    layout.client.run_checked(
        "display-menu", "-T", "tandem",
        "Save state and exit (resume later)", "s", f"run-shell -b '{launcher} save-exit'",
        "Detach (agents keep running)", "d", "detach-client",
        "Update tandem and workspace", "u", f"display-popup -E -w 80% -h 70% '{launcher} update'",
        "Kill session without saving", "k", f"kill-session -t {SESSION}",
        "Cancel", "c", "")


def reload_ui(layout: Layout, workspace: Path, config: Config) -> None:
    """Re-apply key bindings/hooks and restart tandem's own windows on the code now on disk."""
    layout.apply_chrome()
    layout.respawn_own({"status": status_launch(), **{f"placeholder-{r}": placeholder_launch(r) for r in ROLES}})
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
            save_state(layout, workspace)
            print("Saved current state first.")
        except Exception as error:  # saving is a courtesy, never a blocker
            print(f"(state not saved: {error})")
    result = updater.run_update(workspace, config.update_spec, lambda: _reload_in_fresh_process(workspace, layout))
    print(f"\n{'OK' if result.ok else 'FAILED'}: {result.message}")
    if sys.stdin.isatty():
        input("\nPress Enter to close.")
    return 0 if result.ok else 1


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
    print(f"No {role}s yet. Press F2 to add one.")
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
