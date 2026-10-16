from tandem.coordination.schema import Query
from tandem.sanitize import clean
from tandem.tmux.client import TmuxClient

ACK_LIMIT = 20


def order_queries(queries: list[Query]) -> list[tuple[str, list[Query]]]:
    """Group for display: open (oldest first), answered, then the latest acknowledged."""
    open_ = sorted((q for q in queries if q.status == "open"), key=lambda q: q.created)
    answered = sorted((q for q in queries if q.status == "answered"), key=lambda q: q.created)
    acked = sorted((q for q in queries if q.status == "acknowledged"), key=lambda q: q.created, reverse=True)
    groups = [("Open", open_), ("Answered (awaiting pickup)", answered),
              (f"Acknowledged (last {ACK_LIMIT})", acked[:ACK_LIMIT])]
    return [(name, items) for name, items in groups if items]


class Notifier:
    """Announces newly opened questions with a tmux message; never opens a popup or steals focus."""

    def __init__(self, tmux: TmuxClient | None) -> None:
        self._tmux = tmux
        self._seen: set[str] | None = None

    def new_queries(self, queries: list[Query]) -> list[Query]:
        current = {q.id for q in queries if q.status == "open"}
        if self._seen is None:
            self._seen = current
            return []
        fresh = [q for q in queries if q.status == "open" and q.id not in self._seen]
        self._seen |= current
        return fresh

    def notify(self, queries: list[Query]) -> None:
        for q in self.new_queries(queries):
            if self._tmux is None:
                continue
            who = clean(q.asked_by).replace("#", "##")[:40]
            self._tmux.run("display-message", "-d", "4000", f"tandem: new question from {who} (F6 to answer)")
