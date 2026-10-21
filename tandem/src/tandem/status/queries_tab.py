import re
import time

from textual import on
from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Vertical
from textual.content import Content
from textual.events import Key
from textual.widgets import OptionList, Static, TextArea
from textual.widgets.option_list import Option

from tandem.coordination.schema import Query, Session
from tandem.coordination.store import AnswerConflictError, CoordStore
from tandem.sanitize import clean
from tandem.status.notify import order_queries
from tandem.status.render import age
from tandem.tmux.client import TmuxClient

_PANE_ID = re.compile(r"^%\d+$")
HINTS = "↑↓ move · Enter answer · 1-9 pick option · Ctrl+S send · Esc cancel · Alt+w withdraw · Alt+g go to agent"
EMPTY = "No open questions. Agents keep working while they wait."


class QueriesTab(Vertical):
    """List, read, and answer agent questions; answers are written to the coordination files only."""

    BINDINGS = [
        Binding("ctrl+s", "send", "Send"),
        Binding("escape", "cancel", "Cancel"),
        Binding("alt+w", "withdraw", "Withdraw"),
        Binding("alt+g", "goto", "Go to agent"),
    ]
    DEFAULT_CSS = """
    QueriesTab #q-list { height: 1fr; min-height: 3; }
    QueriesTab #q-detail { height: auto; padding: 0 1; }
    QueriesTab #q-answer { height: 4; }
    QueriesTab #q-notice { height: 1; padding: 0 1; }
    QueriesTab #q-hints { height: 1; padding: 0 1; text-style: dim; }
    """

    def __init__(self, store: CoordStore, tmux: TmuxClient | None = None) -> None:
        super().__init__(id="queries")
        self.store = store
        self.tmux = tmux
        self.queries: list[Query] = []
        self.sessions: list[Session] = []
        self._current: str | None = None
        self._drafts: dict[str, str] = {}

    def compose(self) -> ComposeResult:
        yield OptionList(id="q-list")
        yield Static(EMPTY, id="q-detail", markup=False)
        yield TextArea(id="q-answer")
        yield Static("", id="q-notice", markup=False)
        yield Static(HINTS, id="q-hints", markup=False)

    def update_data(self, queries: list[Query], sessions: list[Session]) -> None:
        self.queries, self.sessions = queries, sessions
        listing = self.query_one("#q-list", OptionList)
        keep = self._current
        listing.clear_options()
        now = time.time()
        for name, items in order_queries(queries):
            listing.add_option(Option(Content(f"{name} ({len(items)})"), disabled=True))
            for item in items:
                listing.add_option(Option(Content(self._row(item, now)), id=item.id))
        self._reselect(keep)
        if not queries:
            self._show(None)

    def _row(self, q: Query, now: float) -> str:
        who = clean(q.asked_by)
        where = clean(q.task)
        if q.status == "open":
            return f"{q.id}  {who}  {where}  {age(now - q.created)} ago  blocks: \"{clean(q.blocking_scope)}\""
        if q.status == "answered":
            via = "answered in CLI" if q.answered_via == "cli" else "answered via tandem"
            return f"{q.id}  {who}  {where}  ✓ {via}"
        return f"{q.id}  {who}  {where}  acknowledged"

    def _reselect(self, query_id: str | None) -> None:
        listing = self.query_one("#q-list", OptionList)
        if query_id is not None:
            for index in range(listing.option_count):
                if listing.get_option_at_index(index).id == query_id:
                    listing.highlighted = index
                    return
        for index in range(listing.option_count):
            if not listing.get_option_at_index(index).disabled:
                listing.highlighted = index
                return

    def _find(self, query_id: str | None) -> Query | None:
        return next((q for q in self.queries if q.id == query_id), None)

    @on(OptionList.OptionHighlighted, "#q-list")
    def _highlighted(self, event: OptionList.OptionHighlighted) -> None:
        self._save_draft()
        self._show(self._find(str(event.option.id) if event.option.id else None))

    @on(OptionList.OptionSelected, "#q-list")
    def _selected(self, event: OptionList.OptionSelected) -> None:
        query = self._find(str(event.option.id) if event.option.id else None)
        self._show(query)
        if query is not None and query.status == "open":
            self.query_one("#q-answer", TextArea).focus()

    def _save_draft(self) -> None:
        if self._current is not None:
            self._drafts[self._current] = self.query_one("#q-answer", TextArea).text

    def _show(self, query: Query | None) -> None:
        detail = self.query_one("#q-detail", Static)
        answer = self.query_one("#q-answer", TextArea)
        self._current = query.id if query else None
        if query is None:
            detail.update(EMPTY)
            answer.text = ""
            return
        lines = [f"{query.id} · asked by {clean(query.asked_by)} · task {clean(query.task)}", clean(query.question)]
        if query.options:
            lines.append("Options: " + "   ".join(f"[{i + 1}] {clean(o)}" for i, o in enumerate(query.options)))
        if query.status != "open":
            how = "in CLI" if query.answered_via == "cli" else "via tandem"
            lines.append(f"Status: {query.status} {how}. Answer: {clean(query.answer)}" if query.answer
                         else f"Status: {query.status}")
        detail.update("\n".join(lines))
        answer.text = self._drafts.get(query.id, "")
        answer.read_only = query.status != "open"

    def on_key(self, event: Key) -> None:
        if event.key.isdigit() and event.key != "0" and isinstance(self.app.focused, OptionList):
            query = self._find(self._current)
            index = int(event.key) - 1
            if query and query.status == "open" and index < len(query.options):
                self.query_one("#q-answer", TextArea).text = clean(query.options[index])
                self._save_draft()
                event.stop()

    def _notice(self, text: str) -> None:
        self.query_one("#q-notice", Static).update(text)

    def action_send(self) -> None:
        query = self._find(self._current)
        text = self.query_one("#q-answer", TextArea).text.strip()
        if query is None:
            return
        if not text:
            self._notice("Warning: type an answer first.")
            return
        try:
            self.store.answer(query.id, text, via="tandem")
        except AnswerConflictError as exc:
            self._notice(f"Warning: this question is already {exc.status}; your draft is kept.")
            return
        except (OSError, ValueError) as exc:
            self._notice(f"Warning: could not save the answer ({exc}); your draft is kept.")
            return
        self._drafts.pop(query.id, None)
        self._notice(f"Answer sent to {clean(query.asked_by)}. They pick it up at their next checkpoint.")
        self.query_one("#q-list", OptionList).focus()
        self.app.action_refresh_now()  # type: ignore[attr-defined]

    def action_cancel(self) -> None:
        self.query_one("#q-answer", TextArea).text = ""
        self._notice("")
        self.query_one("#q-list", OptionList).focus()

    def action_withdraw(self) -> None:
        query = self._find(self._current)
        if query is None or isinstance(self.app.focused, TextArea):
            return
        try:
            self.store.withdraw(query.id)
        except AnswerConflictError as exc:
            self._notice(f"Warning: this question is already {exc.status}.")
            return
        self._notice("Marked withdrawn.")
        self.app.action_refresh_now()  # type: ignore[attr-defined]

    def action_goto(self) -> None:
        query = self._find(self._current)
        if query is None or self.tmux is None or isinstance(self.app.focused, TextArea):
            return
        session = next((s for s in self.sessions if s.id == query.asked_by), None)
        if session is not None and _PANE_ID.match(session.pane):
            self.tmux.run_checked("select-pane", "-t", session.pane)
