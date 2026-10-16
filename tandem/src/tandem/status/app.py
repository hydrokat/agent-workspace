import re
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
from tandem.status.queries_tab import QueriesTab
from tandem.tmux.client import TmuxClient

COMPACT_BELOW_ROWS = 8
_PANE_ID = re.compile(r"^%\d+$")
HINTS = "Tab switch tabs · ↑↓ move · g go to agent's pane · r refresh · F6 queries"


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
        Binding("r", "refresh_now", "Refresh"),
        Binding("g", "goto", "Go to agent"),
        Binding("f6", "show_queries", "Queries"),
    ]

    def __init__(self, workspace: Path, store: CoordStore, tmux: TmuxClient | None = None,
                 refresh_seconds: float = 2.0) -> None:
        super().__init__()
        self.workspace = workspace
        self.store = store
        self.tmux = tmux
        self.refresh_seconds = refresh_seconds
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

    def action_goto(self) -> None:
        sessions = self.query_one("#sessions", OptionList)
        if self.tmux is None or sessions.highlighted is None:
            return
        pane = self.snapshot.sessions[int(str(sessions.get_option_at_index(sessions.highlighted).id))].pane
        if _PANE_ID.match(pane):
            self.tmux.run_checked("select-pane", "-t", pane)


def run_status(workspace: Path, config: Config) -> None:
    from tandem.commands import coord_dir_for

    StatusApp(workspace, CoordStore(coord_dir_for(workspace, config)), tmux=TmuxClient()).run()
