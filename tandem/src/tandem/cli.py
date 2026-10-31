import argparse
import sys
from collections.abc import Sequence
from pathlib import Path

from tandem import __version__
from tandem import commands, state
from tandem.config import Config


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="tandem",
        description="Launch planner and implementer AI CLIs side-by-side in tmux.",
    )
    parser.add_argument("--version", action="version", version=f"tandem {__version__}")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--resume", action="store_true", help="resume the saved session without asking")
    mode.add_argument("--fresh", action="store_true",
                      help="start blank with new conversations, without asking (the saved session is replaced)")
    sub = parser.add_subparsers(dest="command", metavar="command")
    sub.add_parser("add-agent", help="add an agent to the running session (opened by the add-agent key)")
    tab = sub.add_parser("tab", help="switch agent tab in the focused column")
    tab.add_argument("direction", choices=["prev", "next", "select"])
    tab.add_argument("number", nargs="?", type=int)
    focus = sub.add_parser("focus", help="focus a column or the status window")
    focus.add_argument("target", choices=["planner", "implementer", "status", "queries"])
    for name, text in (("close-agent", "close the focused agent"), ("restart", "restart an exited agent"),
                       ("refresh", "redraw tab titles"), ("reflow", "re-stack columns for the terminal size"),
                       ("reset-layout", "reset pane sizes to the defaults"),
                       ("quit", "detach or quit"), ("help", "show the key help"),
                       ("first-run", "open help once on first attach"), ("status", "run the status window"),
                       ("whats-new", "show what changed in the last update")):
        sub.add_parser(name, help=text)
    update = sub.add_parser("update", help="update tandem and the workspace; agents keep running (F10 menu)")
    update.add_argument("--yes", action="store_true", help="do not ask for confirmation")
    click = sub.add_parser("click", help="dispatch a status-bar or pane-menu click action")
    click.add_argument("action", nargs="?", default="", help="action name from the hint bar")
    click_tab_p = sub.add_parser("click-tab", help="open the agent menu for a clicked column title")
    click_tab_p.add_argument("pane", help="pane id (%%N format)")
    click_tab_p.add_argument("--select", default="", help="pane id of the agent to switch to")
    sub.add_parser("quota-feed", help="Claude Code statusline hook: record quota, then run your own statusline")
    sub.add_parser("quota-line", help="print the quota line shown under the window")
    sub.add_parser("save-exit", help="save the session state and end the session")
    sub.add_parser("discard-exit", help="end the session and delete the saved state")
    sub.add_parser("reload", help="restart tandem's own windows on the installed code")
    sub.add_parser("placeholder", help="empty-column placeholder").add_argument("role")
    return parser


def main(argv: Sequence[str] | None = None) -> None:
    args = build_parser().parse_args(argv)
    config = Config.from_env()
    workspace = config.workspace or Path.cwd()
    command = args.command
    if command is None:
        _launch(workspace, config, resume=args.resume, fresh=args.fresh)
    elif command == "help":
        commands.show_help()
    elif command == "whats-new":
        commands.show_whats_new(workspace)
    elif command == "placeholder":
        commands.placeholder(args.role)
    elif command == "status":
        from tandem.status.app import run_status

        run_status(workspace, config)
    elif command == "quota-feed":
        commands.quota_feed(config)
    elif command == "quota-line":
        commands.quota_line(workspace, config)
    elif command == "update":
        sys.exit(commands.run_update(workspace, config, args.yes))
    elif command == "add-agent":
        from tandem.wizard.runner import run_add_agent

        spec = run_add_agent()
        if spec is not None:
            commands.add_agent(spec, workspace, config)
    else:
        _tmux_command(command, args, workspace, config)


def _launch(workspace: Path, config: Config, resume: bool = False, fresh: bool = False) -> None:
    from tandem.wizard.runner import run_launch_wizard

    layout = commands.make_layout(workspace, config)
    if layout.exists():
        from tandem.notice import read_notice
        notice = read_notice(workspace)
        if notice is not None and not notice.seen:
            commands._reload_in_fresh_process(workspace, layout)
        commands.attach(workspace)
    _note_legacy_session()
    saved = None if fresh else state.load(state.state_path(workspace))
    if resume and saved is None:
        print("No saved session to resume.", file=sys.stderr)
        sys.exit(1)
    if saved is not None and not resume:
        resume = _ask_resume(saved)
    specs = None
    if resume and saved is not None:
        specs, notes = commands.resume_specs(saved, workspace)
        for note in notes:
            print(note, file=sys.stderr)
        if not specs:
            print("Nothing from the saved session can be started. Starting blank.", file=sys.stderr)
            saved = None
    if not specs:
        saved = None
        specs = run_launch_wizard()
    if not specs:
        print("Cancelled.", file=sys.stderr)
        sys.exit(1)
    commands.start(specs, workspace, config, saved)


def _note_legacy_session() -> None:
    """Sessions started before per-project sockets live on the shared 'tandem' socket and are not reused."""
    from tandem.tmux.client import TmuxClient
    from tandem.tmux.layout import SESSION

    try:
        found = TmuxClient(socket="tandem").run("has-session", "-t", SESSION).code == 0
    except OSError:
        return
    if found:
        print("Note: an older tandem session (from before per-project sessions) is still running. "
              "Detach and end it with `tmux -L tandem kill-server` once you no longer need it; "
              "this project gets its own session.", file=sys.stderr)


def _ask_resume(saved: state.SavedState) -> bool:
    """Resume or start blank; defaults to resume. Without a terminal, start blank."""
    if not sys.stdin.isatty():
        return False
    print(state.describe(saved))
    while True:
        try:
            answer = input("\n[R]esume it, start [b]lank (new conversations), or [q]uit? ").strip().lower()
        except EOFError:
            return False
        if answer in ("", "r", "resume"):
            return True
        if answer in ("b", "blank"):
            return False
        if answer in ("q", "quit"):
            sys.exit(0)


def _tmux_command(command: str, args: argparse.Namespace, workspace: Path, config: Config) -> None:
    layout = commands.make_layout(workspace, config)
    if command == "tab":
        commands.tab(layout, config, args.direction, args.number)
    elif command == "focus":
        commands.focus(layout, config, args.target)
    elif command == "close-agent":
        commands.close_agent(layout, config)
    elif command == "restart":
        pane = config.tmux_pane or layout.client.run_checked("display-message", "-p", "#{pane_id}").strip()
        if pane:
            commands.restart(layout, pane, workspace, config)
    elif command == "refresh":
        layout.refresh()
    elif command == "reflow":
        size = layout.client.run_checked("display-message", "-p", "#{client_width} #{client_height}").split()
        ok = len(size) == 2 and all(x.isdigit() for x in size)
        layout.reflow(int(size[0]) if ok else 200, int(size[1]) if ok else 0)
    elif command == "reset-layout":
        layout.reset()
    elif command == "quit":
        commands.quit_menu(layout, str(workspace))
    elif command == "save-exit":
        commands.save_exit(layout, workspace)
    elif command == "discard-exit":
        commands.discard_exit(layout, workspace)
    elif command == "reload":
        commands.reload_ui(layout, workspace, config)
    elif command == "click":
        commands.click(layout, config, args.action)
    elif command == "click-tab":
        commands.click_tab(layout, config, args.pane, args.select)
    elif command == "first-run":
        commands.first_run(layout, config)
