import json
from dataclasses import dataclass
from typing import Any

STALE_SECONDS = 600


def _str(data: dict[str, Any], key: str) -> str:
    value = data.get(key, "")
    return value if isinstance(value, str) else ""


def _num(data: dict[str, Any], key: str) -> float:
    value = data.get(key, 0)
    return float(value) if isinstance(value, (int, float)) and not isinstance(value, bool) else 0.0


def _strs(data: dict[str, Any], key: str) -> tuple[str, ...]:
    value = data.get(key, [])
    return tuple(v for v in value if isinstance(v, str)) if isinstance(value, list) else ()


@dataclass(frozen=True)
class Session:
    id: str
    cli: str
    role: str
    model: str
    pane: str
    cwd: str
    branch: str
    worktree: str
    task: str
    heartbeat: float

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> "Session | None":
        sid = _str(d, "id")
        if not sid:
            return None
        return cls(sid, _str(d, "cli"), _str(d, "role"), _str(d, "model"), _str(d, "pane"),
                   _str(d, "cwd"), _str(d, "branch"), _str(d, "worktree"), _str(d, "task"),
                   _num(d, "heartbeat"))


@dataclass(frozen=True)
class Claim:
    phase: str
    task: str
    session: str
    branch: str
    worktree: str
    paths: tuple[str, ...]
    heartbeat: float

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> "Claim | None":
        phase, task = _str(d, "phase"), _str(d, "task")
        if not phase or not task:
            return None
        return cls(phase, task, _str(d, "session"), _str(d, "branch"), _str(d, "worktree"),
                   _strs(d, "paths"), _num(d, "heartbeat"))


@dataclass(frozen=True)
class Query:
    id: str
    status: str
    asked_by: str
    task: str
    blocking_scope: str
    created: float
    question: str
    options: tuple[str, ...]
    answer: str
    answered_via: str

    @classmethod
    def from_dict(cls, d: dict[str, Any]) -> "Query | None":
        qid = _str(d, "id")
        status = _str(d, "status")
        if not qid or status not in ("open", "answered", "acknowledged", "withdrawn"):
            return None
        return cls(qid, status, _str(d, "asked_by"), _str(d, "task"), _str(d, "blocking_scope"),
                   _num(d, "created"), _str(d, "question"), _strs(d, "options"),
                   _str(d, "answer"), _str(d, "answered_via"))


def decode_query_fields(text: str) -> dict[str, Any] | None:
    """Parse the JSON-valued frontmatter written by coord.py; never evaluates content."""
    lines = text.split("\n")
    if not lines or lines[0] != "---":
        return None
    fields: dict[str, Any] = {}
    for line in lines[1:]:
        if line == "---":
            return fields
        key, sep, raw = line.partition(": ")
        if not sep:
            return None
        try:
            fields[key] = json.loads(raw)
        except ValueError:
            return None
    return None


def encode_query_fields(fields: dict[str, Any]) -> str:
    return "---\n" + "\n".join(f"{k}: {json.dumps(v)}" for k, v in fields.items()) + "\n---\n"
