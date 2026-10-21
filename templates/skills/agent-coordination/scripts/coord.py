#!/usr/bin/env python3
"""File-based coordination for agent sessions: sessions, claims, and queries.

Stdlib only. JSON on stdout. Exit codes: 0 ok, 1 conflict/not allowed, 2 invalid input.
"""
from __future__ import annotations

import argparse
import fcntl
import json
import os
import re
import secrets
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any, NoReturn

STALE_SECONDS = 600
MAX_TEXT = 8000
SESSION_RE = re.compile(r"^[a-z0-9-]{1,64}$")
PHASE_RE = re.compile(r"^p-\d{3}-[a-z0-9-]{1,48}$")
TASK_RE = re.compile(r"^t-\d{3}$")
QUERY_RE = re.compile(r"^q-[a-z0-9]{4,32}$")
ROLES = ("planner", "implementer")
VIA = ("tandem", "cli")
STATUSES = ("open", "answered", "acknowledged", "withdrawn")
CTRL_RE = re.compile(r"[\x00-\x08\x0b-\x1f\x7f-\x9f]")


def fail(message: str, code: int = 2, **extra: Any) -> NoReturn:
    print(json.dumps({"error": message, **extra}))
    sys.exit(code)


def emit(data: dict[str, Any]) -> None:
    print(json.dumps(data))


def clean_text(value: str, label: str) -> str:
    if len(value) > MAX_TEXT:
        fail(f"{label} exceeds {MAX_TEXT} characters")
    return CTRL_RE.sub("", value.replace("\r\n", "\n").replace("\r", "\n").replace("\x1b", ""))


def check(pattern: re.Pattern[str], value: str, label: str) -> str:
    if not pattern.match(value):
        fail(f"invalid {label}")
    return value


def resolve_root() -> Path:
    override = os.environ.get("TANDEM_COORD_DIR", "")
    if override:
        path = Path(override)
        if not path.is_absolute():
            fail("TANDEM_COORD_DIR must be absolute")
        return path
    base = Path.cwd()
    try:
        common = subprocess.run(
            ["git", "rev-parse", "--git-common-dir"],
            capture_output=True, text=True, timeout=5, check=True,
        ).stdout.strip()
        base = (Path.cwd() / common).resolve().parent
    except (OSError, subprocess.SubprocessError):
        pass
    for candidate in (base, *base.parents):
        if (candidate / ".agent-workspace" / "manifest.json").is_file():
            return candidate / ".agent-workspace" / "coordination"
    return base / ".agent-workspace" / "coordination"


def ensure_dir(path: Path) -> Path:
    if path.is_symlink():
        fail(f"refusing symlinked directory: {path.name}")
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    if path.is_symlink() or not path.is_dir():
        fail(f"refusing non-directory: {path.name}")
    path.chmod(0o700)
    return path


class Store:
    def __init__(self, root: Path) -> None:
        self.root = ensure_dir(root)
        gitignore = self.root / ".gitignore"
        if not gitignore.exists():
            gitignore.write_text("*\n")
        self.sessions = ensure_dir(self.root / "sessions")
        self.claims = ensure_dir(self.root / "claims")
        self.queries = ensure_dir(self.root / "queries")

    def write(self, path: Path, text: str) -> None:
        fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=".tmp-")
        with os.fdopen(fd, "w") as handle:
            handle.write(text)
        os.chmod(tmp, 0o600)
        os.replace(tmp, path)

    def read_json(self, path: Path) -> dict[str, Any] | None:
        try:
            data = json.loads(path.read_text())
        except (OSError, ValueError):
            return None
        return data if isinstance(data, dict) else None

    def lock(self) -> Any:
        handle = open(self.root / ".lock", "a")
        fcntl.flock(handle, fcntl.LOCK_EX)
        return handle


def encode_query(fields: dict[str, Any]) -> str:
    lines = [f"{key}: {json.dumps(value)}" for key, value in fields.items()]
    return "---\n" + "\n".join(lines) + "\n---\n"


def decode_query(text: str) -> dict[str, Any] | None:
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


def read_query(store: Store, qid: str) -> dict[str, Any] | None:
    try:
        return decode_query((store.queries / f"{qid}.md").read_text())
    except OSError:
        return None


def session_id(args: argparse.Namespace) -> str:
    sid = args.session or os.environ.get("TANDEM_SESSION_ID", "")
    if not sid:
        fail("no session: set TANDEM_SESSION_ID or run `whoami` first")
    return check(SESSION_RE, sid, "session id")


def identity(args: argparse.Namespace) -> dict[str, str]:
    role = os.environ.get("TANDEM_ROLE", "implementer")
    if role not in ROLES:
        fail("invalid role")
    cli = clean_text(os.environ.get("TANDEM_CLI", "unknown"), "cli")[:32]
    return {"session": session_id(args), "role": role, "cli": cli}


def git_value(*argv: str) -> str:
    try:
        return subprocess.run(["git", *argv], capture_output=True, text=True, timeout=5).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return ""


def with_staleness(record: dict[str, Any], ttl: int = STALE_SECONDS) -> dict[str, Any]:
    beat = record.get("heartbeat")
    age = time.time() - beat if isinstance(beat, (int, float)) else float("inf")
    return {**record, "age_seconds": None if age == float("inf") else int(age), "stale": age > ttl}


def list_json(directory: Path, store: Store) -> list[dict[str, Any]]:
    records = [store.read_json(p) for p in sorted(directory.glob("*.json"))]
    return [with_staleness(r) for r in records if r]


def cmd_whoami(args: argparse.Namespace, store: Store) -> None:
    if not (args.session or os.environ.get("TANDEM_SESSION_ID")):
        args.session = f"s-{secrets.token_hex(4)}"
        data = identity(args)
        register(args, store)
        emit({**data, "generated": True, "hint": f"export TANDEM_SESSION_ID={data['session']}"})
        return
    emit(identity(args))


def register(args: argparse.Namespace, store: Store) -> dict[str, Any]:
    ident = identity(args)
    record = {
        "id": ident["session"], "cli": ident["cli"], "role": ident["role"],
        "model": clean_text(getattr(args, "model", "") or "", "model")[:64],
        "pane": os.environ.get("TMUX_PANE", ""), "cwd": os.getcwd(),
        "branch": git_value("branch", "--show-current"),
        "worktree": git_value("rev-parse", "--show-toplevel"),
        "task": "", "started": time.time(), "heartbeat": time.time(),
    }
    store.write(store.sessions / f"{ident['session']}.json", json.dumps(record))
    return record


def cmd_register(args: argparse.Namespace, store: Store) -> None:
    emit(register(args, store))


def cmd_heartbeat(args: argparse.Namespace, store: Store) -> None:
    sid = session_id(args)
    path = store.sessions / f"{sid}.json"
    record = store.read_json(path)
    if record is None:
        record = register(args, store)
    record["heartbeat"] = time.time()
    if args.task is not None:
        record["task"] = clean_text(args.task, "task")[:120]
    store.write(path, json.dumps(record))
    for claim_path in store.claims.glob("*.json"):
        claim = store.read_json(claim_path)
        if claim and claim.get("session") == sid:
            claim["heartbeat"] = time.time()
            store.write(claim_path, json.dumps(claim))
    emit({"ok": True, "session": sid})


def cmd_unregister(args: argparse.Namespace, store: Store) -> None:
    sid = session_id(args)
    (store.sessions / f"{sid}.json").unlink(missing_ok=True)
    emit({"ok": True, "session": sid})


def claim_path(store: Store, phase: str, task: str) -> Path:
    check(PHASE_RE, phase, "phase")
    check(TASK_RE, task, "task")
    return store.claims / f"{phase}--{task}.json"


def cmd_claim(args: argparse.Namespace, store: Store) -> None:
    sid = session_id(args)
    path = claim_path(store, args.phase, args.task)
    record = {
        "phase": args.phase, "task": args.task, "session": sid,
        "branch": clean_text(args.branch or "", "branch")[:120],
        "worktree": clean_text(args.worktree or "", "worktree")[:300],
        "paths": [clean_text(p, "path")[:300] for p in (args.paths or [])][:50],
        "claimed": time.time(), "heartbeat": time.time(),
    }
    fd, tmp = tempfile.mkstemp(dir=store.claims, prefix=".tmp-")
    with os.fdopen(fd, "w") as handle:
        handle.write(json.dumps(record))
    os.chmod(tmp, 0o600)
    try:
        os.link(tmp, path)
    except FileExistsError:
        existing = store.read_json(path) or {}
        info = with_staleness(existing)
        fail("already claimed", 1, held_by=existing.get("session"), stale=info["stale"],
             note="Stale claims are never taken over automatically; ask the user via `ask`.")
    finally:
        os.unlink(tmp)
    emit({"ok": True, "claim": record})


def cmd_release(args: argparse.Namespace, store: Store) -> None:
    sid = session_id(args)
    path = claim_path(store, args.phase, args.task)
    existing = store.read_json(path)
    if existing is None:
        fail("no such claim", 1)
    if existing.get("session") != sid:
        fail("claim owned by another session", 1, held_by=existing.get("session"))
    path.unlink(missing_ok=True)
    emit({"ok": True})


def cmd_claims(args: argparse.Namespace, store: Store) -> None:
    claims = list_json(store.claims, store)
    if args.stale:
        claims = [c for c in claims if c["stale"]]
    emit({"claims": claims})


def cmd_ask(args: argparse.Namespace, store: Store) -> None:
    sid = session_id(args)
    qid = f"q-{secrets.token_hex(4)}"
    task = clean_text(args.task, "task")[:120]
    fields = {
        "id": qid, "status": "open", "asked_by": sid, "task": task,
        "blocking_scope": clean_text(args.blocking_scope, "blocking scope")[:300],
        "created": time.time(), "question": clean_text(args.question, "question"),
        "options": [clean_text(o, "option")[:200] for o in (args.option or [])][:9],
        "answer": "", "answered_via": "",
    }
    store.write(store.queries / f"{qid}.md", encode_query(fields))
    emit({"ok": True, "id": qid, "note": "Continue work outside blocking_scope; check `answers --mine` at checkpoints."})


def all_queries(store: Store) -> list[dict[str, Any]]:
    found = [read_query(store, p.stem) for p in sorted(store.queries.glob("q-*.md"))]
    return [q for q in found if q and q.get("status") in STATUSES]


def cmd_answers(args: argparse.Namespace, store: Store) -> None:
    sid = session_id(args) if args.mine else ""
    items = [q for q in all_queries(store) if q["status"] == "answered" and (not args.mine or q["asked_by"] == sid)]
    emit({"answers": items})


def transition(store: Store, qid: str, allowed_from: tuple[str, ...], updates: dict[str, Any]) -> dict[str, Any]:
    check(QUERY_RE, qid, "query id")
    with store.lock():
        query = read_query(store, qid)
        if query is None:
            fail("no such query", 1)
        if query["status"] not in allowed_from:
            fail(f"query is {query['status']}, not {' or '.join(allowed_from)}", 1, status=query["status"])
        query.update(updates)
        store.write(store.queries / f"{qid}.md", encode_query(query))
    return query


def cmd_answer(args: argparse.Namespace, store: Store) -> None:
    if args.via not in VIA:
        fail("--via must be tandem or cli")
    text = clean_text(args.text, "answer")
    emit({"ok": True, "query": transition(store, args.id, ("open",), {
        "status": "answered", "answer": text, "answered_via": args.via, "answered_at": time.time()})})


def cmd_ack(args: argparse.Namespace, store: Store) -> None:
    emit({"ok": True, "query": transition(store, args.id, ("answered",), {"status": "acknowledged"})})


def cmd_withdraw(args: argparse.Namespace, store: Store) -> None:
    emit({"ok": True, "query": transition(store, args.id, ("open",), {"status": "withdrawn"})})


def cmd_overview(args: argparse.Namespace, store: Store) -> None:
    queries = all_queries(store)
    emit({
        "sessions": list_json(store.sessions, store),
        "claims": list_json(store.claims, store),
        "open_queries": [q for q in queries if q["status"] == "open"],
    })


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="coord.py", description=__doc__)
    parser.add_argument("--session", help="override TANDEM_SESSION_ID")
    sub = parser.add_subparsers(dest="command", required=True)

    def add(name: str, func: Any) -> argparse.ArgumentParser:
        p = sub.add_parser(name)
        p.set_defaults(func=func)
        return p

    add("whoami", cmd_whoami)
    add("register", cmd_register).add_argument("--model")
    hb = add("heartbeat", cmd_heartbeat)
    hb.add_argument("--task")
    add("unregister", cmd_unregister)
    claim = add("claim", cmd_claim)
    claim.add_argument("phase")
    claim.add_argument("task")
    claim.add_argument("--branch")
    claim.add_argument("--worktree")
    claim.add_argument("--paths", nargs="*")
    rel = add("release", cmd_release)
    rel.add_argument("phase")
    rel.add_argument("task")
    add("claims", cmd_claims).add_argument("--stale", action="store_true")
    ask = add("ask", cmd_ask)
    ask.add_argument("question")
    ask.add_argument("--task", required=True)
    ask.add_argument("--blocking-scope", required=True)
    ask.add_argument("--option", action="append")
    add("answers", cmd_answers).add_argument("--mine", action="store_true")
    add("ack", cmd_ack).add_argument("id")
    ans = add("answer", cmd_answer)
    ans.add_argument("id")
    ans.add_argument("text")
    ans.add_argument("--via", required=True)
    add("withdraw", cmd_withdraw).add_argument("id")
    add("overview", cmd_overview)
    return parser


def main(argv: list[str] | None = None) -> None:
    args = build_parser().parse_args(argv)
    store = Store(resolve_root())
    args.func(args, store)


if __name__ == "__main__":
    main()
