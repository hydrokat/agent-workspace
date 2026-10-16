import argparse
import sys
from collections.abc import Sequence
from pathlib import Path

from tandem import __version__
from tandem import commands
from tandem.config import Config


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="tandem",
        description="Launch planner and implementer AI CLIs side-by-side in tmux.",
    )
    parser.add_argument("--version", action="version", version=f"tandem {__version__}")
    sub = parser.add_subparsers(dest="command", metavar="command")
    sub.add_parser("add-agent", help="add an agent to the running session (opened by F2)")
    tab = sub.add_parser("tab", help="switch agent tab in the focused column")
    tab.add_argument("direction", choices=["prev", "next", "select"])
    tab.add_argument("number", nargs="?", type=int)
    focus = sub.add_parser("focus", help="focus a column or the status window")
    focus.add_argument("target", choices=["planner", "implementer", "status", "queries"])
    for name, text in (("close-agent", "close the focused agent"), ("restart", "restart an exited agent"),
                       ("refresh", "redraw tab titles"), ("reflow", "re-stack columns for the terminal size"),
                       ("quit", "detach or quit"), ("help", "show the key help"),
                       ("first-run", "open help once on first attach"), ("status", "run the status window")):
        sub.add_parser(name, help=text)
    sub.add_parser("placeholder", help="empty-column placeholder").add_argument("role")
    return parser


def main(argv: Sequence[str] | None = None) -> None:
    args = build_parser().parse_args(argv)
    config = Config.from_env()
    workspace = config.workspace or Path.cwd()
    command = args.command
    if command is None:
        _launch(workspace, config)
    elif command == "help":
        commands.show_help()
    elif command == "placeholder":
        commands.placeholder(args.role)
    elif command == "status":
        from tandem.status.app import run_status

        run_status(workspace, config)
    elif command == "add-agent":
        from tandem.wizard.runner import run_add_agent

        spec = run_add_agent()
        if spec is not None:
            commands.add_agent(spec, workspace, config)
    else:
        _tmux_command(command, args, workspace, config)


def _launch(workspace: Path, config: Config) -> None:
    from tandem.wizard.runner import run_launch_wizard

    layout = commands.make_layout(workspace, config)
    if layout.exists():
        commands.attach()
    specs = run_launch_wizard()
    if not specs:
        print("Cancelled.", file=sys.stderr)
        sys.exit(1)
    commands.start(specs, workspace, config)


def _tmux_command(command: str, args: argparse.Namespace, workspace: Path, config: Config) -> None:
    layout = commands.make_layout(workspace, config)
    if command == "tab":
        commands.tab(layout, config, args.direction, args.number)
    elif command == "focus":
        commands.focus(layout, config, args.target)
    elif command == "close-agent":
        commands.close_agent(layout, config)
    elif command == "restart":
        pane = config.tmux_pane
        if pane:
            layout.restart(pane, str(commands.coord_dir_for(workspace, config)))
    elif command == "refresh":
        layout.refresh()
    elif command == "reflow":
        width = layout.client.run_checked("display-message", "-p", "#{client_width}").strip()
        layout.reflow(int(width) if width.isdigit() else 200)
    elif command == "quit":
        commands.quit_menu(layout)
    elif command == "first-run":
        commands.first_run(layout, config)
