import fcntl
import json
import os
import re
import tempfile
import time
from pathlib import Path
from typing import Any

from tandem.coordination.schema import (
    STALE_SECONDS,
    Claim,
    Query,
    Session,
    decode_query_fields,
    encode_query_fields,
)

_QUERY_ID = re.compile(r"^q-[a-z0-9]{4,32}$")
_SESSION_ID = re.compile(r"^[a-z0-9-]{1,64}$")


class AnswerConflictError(Exception):
    """The query was no longer open when the answer was about to be written."""

    def __init__(self, status: str) -> None:
        super().__init__(f"query is already {status}")
        self.status = status


def is_stale(heartbeat: float, now: float | None = None) -> bool:
    return (now or time.time()) - heartbeat > STALE_SECONDS


class CoordStore:
    """Tolerant reader and answer-writer for the coord.py file protocol."""

    def __init__(self, root: Path) -> None:
        self.root = root

    def exists(self) -> bool:
        return self.root.is_dir() and not self.root.is_symlink()

    def _read_json(self, path: Path) -> dict[str, Any] | None:
        try:
            data = json.loads(path.read_text())
        except (OSError, ValueError):
            return None
        return data if isinstance(data, dict) else None

    def _files(self, sub: str, pattern: str) -> list[Path]:
        directory = self.root / sub
        if directory.is_symlink() or not directory.is_dir():
            return []
        return sorted(p for p in directory.glob(pattern) if not p.is_symlink())

    def sessions(self) -> list[Session]:
        found = (Session.from_dict(d) for p in self._files("sessions", "*.json") if (d := self._read_json(p)))
        return [s for s in found if s]

    def claims(self) -> list[Claim]:
        found = (Claim.from_dict(d) for p in self._files("claims", "*.json") if (d := self._read_json(p)))
        return [c for c in found if c]

    def queries(self) -> list[Query]:
        found: list[Query | None] = []
        for p in self._files("queries", "q-*.md"):
            try:
                fields = decode_query_fields(p.read_text())
            except OSError:
                continue
            if fields:
                found.append(Query.from_dict(fields))
        return [q for q in found if q]

    def _transition(self, query_id: str, allowed: tuple[str, ...], updates: dict[str, Any]) -> None:
        if not _QUERY_ID.match(query_id):
            raise ValueError("invalid query id")
        path = self.root / "queries" / f"{query_id}.md"
        with open(self.root / ".lock", "a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            fields = decode_query_fields(path.read_text())
            if fields is None:
                raise ValueError("unreadable query")
            if fields.get("status") not in allowed:
                raise AnswerConflictError(str(fields.get("status")))
            fields.update(updates)
            fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=".tmp-")
            with os.fdopen(fd, "w") as handle:
                handle.write(encode_query_fields(fields))
            os.chmod(tmp, 0o600)
            os.replace(tmp, path)

    def answer(self, query_id: str, text: str, via: str) -> None:
        """Atomically record an answer; raises AnswerConflictError unless the query is still open."""
        if via not in ("tandem", "cli"):
            raise ValueError("via must be tandem or cli")
        self._transition(query_id, ("open",), {"status": "answered", "answer": text,
                                                "answered_via": via, "answered_at": time.time()})

    def withdraw(self, query_id: str) -> None:
        """Mark an open query withdrawn; raises AnswerConflictError if it is no longer open."""
        self._transition(query_id, ("open",), {"status": "withdrawn"})

    def write_session(self, session_id: str, cli: str, role: str, pane: str, cwd: str, label: str) -> None:
        """Record a session at spawn time (same file format coord.py register writes)."""
        if not _SESSION_ID.match(session_id):
            raise ValueError("invalid session id")
        directory = self.root / "sessions"
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        now = time.time()
        record = {"id": session_id, "cli": cli, "role": role, "model": label, "pane": pane, "cwd": cwd,
                  "branch": "", "worktree": "", "task": "", "started": now, "heartbeat": now}
        fd, tmp = tempfile.mkstemp(dir=directory, prefix=".tmp-")
        with os.fdopen(fd, "w") as handle:
            handle.write(json.dumps(record))
        os.chmod(tmp, 0o600)
        os.replace(tmp, directory / f"{session_id}.json")

    def remove_session(self, session_id: str) -> None:
        if _SESSION_ID.match(session_id):
            (self.root / "sessions" / f"{session_id}.json").unlink(missing_ok=True)
