import os
import shutil
import signal
import sys
import time
from pathlib import Path

from tandem.adapters import adapter_for, session_env, session_id
from tandem.adapters.base import AgentSpec
from tandem.config import Config
from tandem.coordination.store import CoordStore
from tandem.tmux import keys
from tandem.tmux.client import TmuxClient
from tandem.tmux.layout import SESSION, Launch, Layout, ROLES


def agent_launch(spec: AgentSpec, coord_dir: str, workspace: Path) -> Launch:
    return Launch(
        argv=adapter_for(spec.cli).build_argv(spec), role=spec.role, index=spec.index,
        label=f"{spec.cli}·{spec.model}", env=session_env(spec, coord_dir),
        cwd=spec.cwd or str(workspace), session_id=session_id(spec),
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


def start(specs: list[AgentSpec], workspace: Path, config: Config) -> None:
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


def quit_menu(layout: Layout) -> None:
    layout.client.run_checked(
        "display-menu", "-T", "Quit tandem", "Detach (agents keep running)", "d", "detach-client",
        "Kill session and all agents", "k", f"kill-session -t {SESSION}", "Cancel", "c", "")


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
