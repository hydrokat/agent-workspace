import re
import sys
from collections.abc import Callable
from pathlib import Path

from textual import on
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import VerticalScroll
from textual.content import Content
from textual.events import Resize
from textual.widgets import Collapsible, OptionList, Static, TabbedContent, TabPane
from textual.widgets.option_list import Option

from tandem.config import Config
from tandem.coordination.store import CoordStore
from tandem.snapshot import StatusSnapshot, collect
from tandem.status import render
from tandem.status.notify import Notifier
from tandem.tmux import keys
from tandem.status.queries_tab import QueriesTab
from tandem.tmux.client import TmuxClient, socket_for

COMPACT_BELOW_ROWS = 8
AUTOSAVE_SECONDS = 15.0
_PANE_ID = re.compile(r"^%\d+$")


def hints(platform: str = sys.platform) -> str:
    def d(raw: str) -> str:
        return keys.display_shortcut(raw, platform)

    return (f"Tab switch tabs · ↑↓ move · {d(keys.GO_TO_AGENT)} go to agent's pane · {d(keys.STATUS_REFRESH)} refresh · "
            f"{d('Alt+s')} status · {d('Alt+q')} queries")


HINTS = hints()


class StatusApp(App[None]):
    """Bottom-pane dashboard: Status tab (and Queries tab, see queries_tab)."""

    CSS = """
    #summary { height: 1; padding: 0 1; text-style: bold; }
    #hints { height: 1; padding: 0 1; text-style: dim; }
    #sessions { height: auto; max-height: 6; }
    .compact #body { display: none; }
    .compact TabbedContent { height: auto; }
    """
    BINDINGS = [
        Binding(keys.textual_binding_keys(keys.STATUS_REFRESH), "refresh_now", "Refresh"),
        Binding(keys.textual_binding_keys(keys.GO_TO_AGENT), "goto", "Go to agent"),
        Binding(keys.textual_binding_keys("Alt+q"), "show_queries", "Queries"),
        Binding(keys.textual_binding_keys("Alt+s"), "show_status", "Status"),
    ]

    def __init__(self, workspace: Path, store: CoordStore, tmux: TmuxClient | None = None,
                 refresh_seconds: float = 2.0, autosave: Callable[[], object] | None = None,
                 autosave_seconds: float = AUTOSAVE_SECONDS) -> None:
        super().__init__()
        self.workspace = workspace
        self.store = store
        self.tmux = tmux
        self.refresh_seconds = refresh_seconds
        self.autosave = autosave
        """Saves the running session so a crash can be resumed; runs off the UI thread."""
        self.autosave_seconds = autosave_seconds
        self.snapshot = StatusSnapshot()
        self.notifier = Notifier(tmux)

    def compose(self) -> ComposeResult:
        with TabbedContent(initial="status-pane"):
            with TabPane("Status", id="status-pane"):
                yield Static("Loading…", id="summary", markup=False)
                with VerticalScroll(id="body"):
                    yield Static("In progress", classes="label")
                    yield OptionList(id="sessions")
                    yield Static("", id="inprogress", markup=False)
                    with Collapsible(title="Repositories · * main checkout · → linked worktree", collapsed=False):
                        yield Static("", id="repos", markup=False)
                    with Collapsible(title="Phases", collapsed=False):
                        yield Static("", id="phases", markup=False)
                    with Collapsible(title="Conflicts"):
                        yield Static("", id="conflicts", markup=False)
            with TabPane("Queries", id="queries-pane"):
                yield QueriesTab(self.store, self.tmux)
        yield Static(HINTS, id="hints", markup=False)

    def on_mount(self) -> None:
        self.action_refresh_now()
        self.set_interval(self.refresh_seconds, self.action_refresh_now)
        if self.autosave is not None:
            self.set_interval(self.autosave_seconds, self._autosave)

    def _autosave(self) -> None:
        if self.autosave is not None:
            self.run_worker(self.autosave, thread=True, exclusive=True, group="autosave")

    def action_refresh_now(self) -> None:
        self.run_worker(self._collect, thread=True, exclusive=True)

    def _collect(self) -> None:
        snapshot = collect(self.workspace, self.store)
        self.call_from_thread(self._apply, snapshot)

    def _apply(self, snapshot: StatusSnapshot) -> None:
        self.snapshot = snapshot
        self._set("#summary", render.summary_line(snapshot))
        self._set("#inprogress", render.in_progress_text(snapshot))
        self._set("#phases", render.phases_text(snapshot))
        self._render_repos()
        self._set("#conflicts", render.conflicts_text(snapshot))
        sessions = self.query_one("#sessions", OptionList)
        highlighted = sessions.highlighted
        sessions.clear_options()
        for index in range(len(snapshot.sessions)):
            sessions.add_option(Option(Content(render.session_row(snapshot, index)), id=str(index)))
        if highlighted is not None and highlighted < sessions.option_count:
            sessions.highlighted = highlighted
        self._on_snapshot(snapshot)

    def _render_repos(self) -> None:
        self._set("#repos", render.repos_text(self.snapshot, max(self.size.width - 6, render.MIN_WIDTH)))

    def _set(self, selector: str, content: str) -> None:
        self.query_one(selector, Static).update(content)

    def _on_snapshot(self, snapshot: StatusSnapshot) -> None:
        self.query_one(QueriesTab).update_data(snapshot.queries, snapshot.sessions)
        label = f"Queries ({snapshot.open_queries})" if snapshot.open_queries else "Queries"
        self.query_one(TabbedContent).get_tab("queries-pane").label = label
        self.notifier.notify(snapshot.queries)

    @on(Resize)
    def _resized(self, event: Resize) -> None:
        self.set_class(event.size.height < COMPACT_BELOW_ROWS, "compact")
        self.query_one("#body").display = event.size.height >= COMPACT_BELOW_ROWS
        self._render_repos()

    def action_show_queries(self) -> None:
        self.query_one(TabbedContent).active = "queries-pane"
        self.call_after_refresh(self.query_one("#q-list", OptionList).focus)

    def action_show_status(self) -> None:
        self.query_one(TabbedContent).active = "status-pane"
        self.call_after_refresh(self.query_one("#sessions", OptionList).focus)

    def action_goto(self) -> None:
        sessions = self.query_one("#sessions", OptionList)
        if self.tmux is None or sessions.highlighted is None:
            return
        pane = self.snapshot.sessions[int(str(sessions.get_option_at_index(sessions.highlighted).id))].pane
        if _PANE_ID.match(pane):
            self.tmux.run_checked("select-pane", "-t", pane)


def run_status(workspace: Path, config: Config) -> None:
    from tandem.commands import autosave, coord_dir_for, make_layout

    layout = make_layout(workspace, config)
    StatusApp(workspace, CoordStore(coord_dir_for(workspace, config)), tmux=TmuxClient(socket=socket_for(workspace)),
              autosave=lambda: autosave(layout, workspace)).run()
