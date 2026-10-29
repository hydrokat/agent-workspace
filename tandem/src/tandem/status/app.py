import re
import sys
import time
from collections.abc import Callable
from pathlib import Path

from textual import on
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, VerticalScroll
from textual.content import Content
from textual.events import Click, Resize
from textual.widgets import Collapsible, Footer, OptionList, Static, TabbedContent, TabPane
from textual.widgets.option_list import Option

from tandem.config import Config
from tandem.coordination.store import CoordStore
from tandem.notice import Notice, mark_seen, read_notice
from tandem.snapshot import StatusSnapshot, collect
from tandem.status import render
from tandem.status.notify import Notifier
from tandem.tmux import keys
from tandem.status.queries_tab import QueriesTab
from tandem.tmux.client import TmuxClient, socket_for
from tandem.update_check import (
    CheckState,
    Runner,
    due,
    installed_version_from_manifest,
    latest_version,
    read_state,
    record,
    should_offer,
    write_state,
)

COMPACT_BELOW_ROWS = 8
AUTOSAVE_SECONDS = 15.0
_PANE_ID = re.compile(r"^%\d+$")


class UpdateBanner(Horizontal):
    """One-line notice shown above the tabs while an update notice is unseen."""

    DEFAULT_CSS = """
    UpdateBanner { height: 1; }
    UpdateBanner #banner-text { padding: 0 1; width: 1fr; }
    UpdateBanner #banner-dismiss { width: 3; }
    """

    def __init__(self, notice: Notice, workspace: Path, tmux: TmuxClient | None = None) -> None:
        super().__init__()
        self._notice = notice
        self._workspace = workspace
        self._tmux = tmux

    def compose(self) -> ComposeResult:
        yield Static(
            f"⬆ Updated to tandem {self._notice.to_version} — click for what's new",
            id="banner-text",
            markup=False,
        )
        yield Static("[×]", id="banner-dismiss", markup=False)

    def on_click(self, event: Click) -> None:
        widget_id = event.widget.id if event.widget is not None else None
        if widget_id == "banner-dismiss":
            mark_seen(self._workspace)
            self.remove()
        elif widget_id == "banner-text":
            if self._tmux is not None:
                launcher = keys.launcher_for(sys.executable, str(self._workspace))
                self._tmux.run_checked(
                    "display-popup", "-E", "-w", "70%", "-h", "70%", f"{launcher} whats-new"
                )


class OfferBanner(Horizontal):
    """One-line offer shown when a newer release is available during the session."""

    DEFAULT_CSS = """
    OfferBanner { height: 1; }
    OfferBanner #offer-text { padding: 0 1; width: 1fr; }
    OfferBanner #offer-update { width: auto; padding: 0 1; }
    OfferBanner #offer-later { width: auto; padding: 0 1; }
    OfferBanner #offer-skip { width: auto; padding: 0 1; }
    """

    def __init__(self, installed: str, latest: str, workspace: Path,
                 tmux: TmuxClient | None = None) -> None:
        super().__init__()
        self._installed = installed
        self._latest = latest
        self._workspace = workspace
        self._tmux = tmux

    def compose(self) -> ComposeResult:
        yield Static(
            f"⬆ agent-workspace {self._installed} → {self._latest} available",
            id="offer-text",
            markup=False,
        )
        yield Static("[Update now]", id="offer-update", markup=False)
        yield Static("[Later]", id="offer-later", markup=False)
        yield Static("[Skip this version]", id="offer-skip", markup=False)

    def on_click(self, event: Click) -> None:
        widget_id = event.widget.id if event.widget is not None else None
        if widget_id == "offer-update":
            self._run_update_popup()
        elif widget_id == "offer-later":
            self.remove()
            self.app.refresh_bindings()
        elif widget_id == "offer-skip":
            state = read_state(self._workspace)
            write_state(self._workspace, CheckState(last=state.last, latest=state.latest, skip=self._latest))
            self.remove()
            self.app.refresh_bindings()

    def _run_update_popup(self) -> None:
        if self._tmux is not None:
            launcher = keys.launcher_for(sys.executable, str(self._workspace))
            self._tmux.run_checked(
                "display-popup", "-E", "-w", "80%", "-h", "70%", f"{launcher} update --yes"
            )


def hints(platform: str = sys.platform) -> str:
    def d(raw: str) -> str:
        return keys.display_shortcut(raw, platform)

    return (f"Tab switch tabs · ↑↓ move · {d(keys.GO_TO_AGENT)} go to agent's pane · {d(keys.STATUS_REFRESH)} refresh · "
            f"{d('Alt+s')} status · {d('Alt+q')} queries")


HINTS = hints()


_UPDATE_NOW_KEY = "Alt+u"
_CHECK_INTERVAL_SECONDS = 600.0


class StatusApp(App[None]):
    """Bottom-pane dashboard: Status tab (and Queries tab, see queries_tab)."""

    CSS = """
    #summary { height: 1; padding: 0 1; text-style: bold; }
    #sessions { height: auto; max-height: 6; }
    .compact #body { display: none; }
    .compact TabbedContent { height: auto; }
    .compact UpdateBanner { display: none; }
    .compact OfferBanner { display: none; }
    """
    BINDINGS = [
        Binding(
            keys.textual_binding_keys(keys.STATUS_REFRESH), "refresh_now", "Refresh",
            key_display=keys.display_shortcut(keys.STATUS_REFRESH),
        ),
        Binding(
            keys.textual_binding_keys(keys.GO_TO_AGENT), "goto", "Go to agent",
            key_display=keys.display_shortcut(keys.GO_TO_AGENT),
        ),
        Binding(
            keys.textual_binding_keys(_UPDATE_NOW_KEY), "update_now", "Update now",
            key_display=keys.display_shortcut(_UPDATE_NOW_KEY),
            show=False,
        ),
        Binding(
            keys.textual_binding_keys("Alt+s"), "show_status", "Status",
            key_display=keys.display_shortcut("Alt+s"),
        ),
        Binding(
            keys.textual_binding_keys("Alt+q"), "show_queries", "Queries",
            key_display=keys.display_shortcut("Alt+q"),
        ),
    ]

    def __init__(self, workspace: Path, store: CoordStore, tmux: TmuxClient | None = None,
                 refresh_seconds: float = 2.0, autosave: Callable[[], object] | None = None,
                 autosave_seconds: float = AUTOSAVE_SECONDS,
                 config: Config | None = None,
                 update_runner: Runner | None = None,
                 now: Callable[[], float] | None = None) -> None:
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
        self._config = config
        self._update_runner = update_runner
        self._now: Callable[[], float] = now if now is not None else time.time
        self._notified_versions: set[str] = set()
        self._offered_versions: set[str] = set()

    def compose(self) -> ComposeResult:
        notice = read_notice(self.workspace)
        if notice is not None and not notice.seen:
            yield UpdateBanner(notice, self.workspace, self.tmux)
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
        yield Footer()

    def on_mount(self) -> None:
        self.action_refresh_now()
        self.set_interval(self.refresh_seconds, self.action_refresh_now)
        if self.autosave is not None:
            self.set_interval(self.autosave_seconds, self._autosave)
        if self._config is not None and self._config.update_check:
            self._maybe_check_update()
            self.set_interval(_CHECK_INTERVAL_SECONDS, self._maybe_check_update)

    def _autosave(self) -> None:
        if self.autosave is not None:
            self.run_worker(self.autosave, thread=True, exclusive=True, group="autosave")

    def _maybe_check_update(self) -> None:
        if self._config is None or not self._config.update_check:
            return
        state = read_state(self.workspace)
        if due(state, self._now(), self._config.update_interval):
            self.run_worker(self._run_check_worker, thread=True, exclusive=True, group="update-check")

    def _run_check_worker(self) -> None:
        from tandem.updater import run_capturing as _run_capturing

        if self._config is None:
            return
        spec = self._config.update_spec
        runner = self._update_runner if self._update_runner is not None else _run_capturing
        now = self._now()
        latest = latest_version(spec, self.workspace, runner)
        state = read_state(self.workspace)
        new_state = record(self.workspace, state, now, latest)
        installed = installed_version_from_manifest(self.workspace)
        if should_offer(installed, new_state.latest, new_state.skip):
            self.call_from_thread(self._show_offer, installed or "", new_state.latest)

    def _show_offer(self, installed: str, latest: str) -> None:
        if latest in self._offered_versions:
            return
        self._offered_versions.add(latest)
        for banner in self.query(OfferBanner):
            banner.remove()
        tabbed = self.query_one(TabbedContent)
        self.mount(OfferBanner(installed, latest, self.workspace, self.tmux), before=tabbed)
        if self.tmux is not None and latest not in self._notified_versions:
            self._notified_versions.add(latest)
            self.tmux.run_checked("display-message", f"tandem: update {latest} available — see the Status window")
        self.refresh_bindings()

    def check_action(self, action: str, parameters: tuple[object, ...]) -> bool | None:
        if action == "update_now":
            return bool(self.query(OfferBanner))
        return True

    def action_update_now(self) -> None:
        banners = self.query(OfferBanner)
        if not banners:
            return
        banners.first()._run_update_popup()

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
        compact = event.size.height < COMPACT_BELOW_ROWS
        self.set_class(compact, "compact")
        self.query_one("#body").display = not compact
        for banner in self.query(UpdateBanner):
            banner.display = not compact
        for offer in self.query(OfferBanner):
            offer.display = not compact
        self._render_repos()

    @on(OptionList.OptionSelected, "#sessions")
    def _session_selected(self, event: OptionList.OptionSelected) -> None:
        self.action_goto()

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
              autosave=lambda: autosave(layout, workspace), config=config).run()
