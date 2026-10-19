"""Each agent's own conversation ID, so a resumed tandem session reopens the same chat.

Sources are documented CLI interfaces only: the ``--session-id`` tandem gives Claude Code and the
``session_id`` Claude Code passes to its statusline command, and the log Antigravity writes to the
``--log-file`` tandem passes it. tandem never reads a pane or the contents of a conversation; for Claude it
only checks that a transcript file with that ID exists before asking for ``--resume``.
"""
import json
import os
import re
import tempfile
import time
import uuid
from pathlib import Path

SESSION_ID = re.compile(r"^[a-z]+-\d{1,2}$")
CONVERSATION_ID = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
_AGY_LOG_ID = re.compile(r"(?:Created|Streaming) conversation ([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})")
LOG_TAIL = 1_000_000


def valid_id(value: object) -> str | None:
    """The ID lowercased when it is a plain UUID (both CLIs use UUIDs), else None."""
    if not isinstance(value, str):
        return None
    lowered = value.lower()
    return lowered if CONVERSATION_ID.match(lowered) else None


def new_id() -> str:
    return str(uuid.uuid4())


def folder(coord_dir: Path) -> Path:
    return coord_dir / "conversations"


def _checked(session_id: str) -> str:
    if not SESSION_ID.match(session_id):
        raise ValueError("invalid session id")
    return session_id


def agy_log_path(coord_dir: Path, session_id: str) -> Path:
    return folder(coord_dir) / f"agy-{_checked(session_id)}.log"


def prepare(coord_dir: Path) -> None:
    folder(coord_dir).mkdir(parents=True, exist_ok=True, mode=0o700)


def record(coord_dir: Path, session_id: str, conversation: str) -> None:
    """Atomic, owner-only; the file holds the ID and a timestamp only."""
    checked = valid_id(conversation)
    if checked is None:
        raise ValueError("invalid conversation id")
    target = folder(coord_dir) / f"{_checked(session_id)}.json"
    prepare(coord_dir)
    fd, tmp = tempfile.mkstemp(dir=target.parent, prefix=".c-", suffix=".tmp")
    try:
        with os.fdopen(fd, "w") as handle:
            json.dump({"id": checked, "updated": time.time()}, handle)
        os.chmod(tmp, 0o600)
        os.replace(tmp, target)
    except BaseException:
        Path(tmp).unlink(missing_ok=True)
        raise


def recorded(coord_dir: Path, session_id: str) -> str | None:
    try:
        raw = json.loads((folder(coord_dir) / f"{_checked(session_id)}.json").read_text())
    except (OSError, ValueError):
        return None
    return valid_id(raw.get("id")) if isinstance(raw, dict) else None


def from_agy_log(path: Path) -> str | None:
    """The conversation the agy pane last created or switched to, from the tail of its log."""
    try:
        with path.open("rb") as handle:
            size = handle.seek(0, os.SEEK_END)
            handle.seek(max(0, size - LOG_TAIL))
            text = handle.read().decode("utf-8", "replace")
    except OSError:
        return None
    found = _AGY_LOG_ID.findall(text)
    return found[-1] if found else None


def current(coord_dir: Path, session_id: str, cli: str) -> str | None:
    """The conversation the agent in ``session_id`` is on now, when its CLI has told us."""
    if not SESSION_ID.match(session_id):
        return None
    if cli == "claude":
        return recorded(coord_dir, session_id)
    if cli == "agy":
        return from_agy_log(agy_log_path(coord_dir, session_id))
    return None


def forget(coord_dir: Path, session_id: str) -> None:
    """Drop what a previous agent in this slot left, so a new chat is never mistaken for an old one."""
    if not SESSION_ID.match(session_id):
        return
    for path in (folder(coord_dir) / f"{session_id}.json", agy_log_path(coord_dir, session_id)):
        path.unlink(missing_ok=True)


def claude_transcript_exists(claude_dir: Path, conversation: str) -> bool:
    """Claude Code only resumes a session it has saved; a session with no messages has no transcript yet."""
    checked = valid_id(conversation)
    return checked is not None and any((claude_dir / "projects").glob(f"*/{checked}.jsonl"))


def can_resume(cli: str, conversation: str | None, claude_dir: Path) -> bool:
    if conversation is None:
        return False
    if cli == "claude":
        return claude_transcript_exists(claude_dir, conversation)
    return cli == "agy" and valid_id(conversation) is not None
