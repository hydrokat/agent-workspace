import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

from tandem.coordination.store import AnswerConflictError, CoordStore

SCRIPT = Path(__file__).resolve().parents[2] / "templates/skills/agent-coordination/scripts/coord.py"


def coord(root: Path, *args: str, session: str = "impl-1") -> dict[str, object]:
    env = {**os.environ, "TANDEM_COORD_DIR": str(root), "TANDEM_SESSION_ID": session,
           "TANDEM_ROLE": "implementer", "TANDEM_CLI": "claude"}
    proc = subprocess.run([sys.executable, str(SCRIPT), *args], capture_output=True, text=True, env=env)
    assert proc.returncode == 0, proc.stdout + proc.stderr
    data: dict[str, object] = json.loads(proc.stdout)
    return data


@pytest.fixture
def root(tmp_path: Path) -> Path:
    return tmp_path / "coord"


def test_store_reads_what_coord_py_wrote(root: Path) -> None:
    coord(root, "register", "--model", "sonnet")
    coord(root, "claim", "p-001-x", "t-003", "--branch", "b", "--paths", "a.py", "b.py")
    coord(root, "ask", "--task", "p-001 t-003", "--blocking-scope", "ttl", "What TTL?", "--option", "5", "--option", "10")
    store = CoordStore(root)
    [session] = store.sessions()
    assert session.id == "impl-1" and session.model == "sonnet" and session.role == "implementer"
    [claim] = store.claims()
    assert claim.phase == "p-001-x" and claim.task == "t-003" and claim.paths == ("a.py", "b.py")
    [query] = store.queries()
    assert query.status == "open" and query.options == ("5", "10") and query.asked_by == "impl-1"


def test_tandem_answer_is_visible_to_coord_py(root: Path) -> None:
    qid = str(coord(root, "ask", "--task", "p-001 t-003", "--blocking-scope", "s", "Q?")["id"])
    CoordStore(root).answer(qid, "10 min", via="tandem")
    answers: list[dict[str, str]] = coord(root, "answers", "--mine")["answers"]  # type: ignore[assignment]
    [mine] = answers
    assert mine["answer"] == "10 min" and mine["answered_via"] == "tandem"
    coord(root, "ack", qid)
    assert CoordStore(root).queries()[0].status == "acknowledged"


def test_answer_after_cli_answer_conflicts(root: Path) -> None:
    qid = str(coord(root, "ask", "--task", "p-001 t-003", "--blocking-scope", "s", "Q?")["id"])
    coord(root, "answer", qid, "from cli", "--via", "cli", session="human")
    with pytest.raises(AnswerConflictError) as exc:
        CoordStore(root).answer(qid, "late", via="tandem")
    assert exc.value.status == "answered"
    assert CoordStore(root).queries()[0].answer == "from cli"


def test_store_tolerates_garbage_files(root: Path) -> None:
    coord(root, "register")
    (root / "sessions" / "bad.json").write_text("{not json")
    (root / "queries" / "q-zzzz.md").write_text("no frontmatter")
    (root / "claims" / "x.json").write_text("[]")
    store = CoordStore(root)
    assert len(store.sessions()) == 1 and store.queries() == [] and store.claims() == []


def test_store_missing_dir_is_empty(tmp_path: Path) -> None:
    store = CoordStore(tmp_path / "nope")
    assert store.sessions() == [] and store.claims() == [] and store.queries() == []
    assert not store.exists()


def test_withdraw_from_tandem_and_conflict(root: Path) -> None:
    qid = str(coord(root, "ask", "--task", "p-001 t-003", "--blocking-scope", "s", "Q?")["id"])
    CoordStore(root).withdraw(qid)
    assert CoordStore(root).queries()[0].status == "withdrawn"
    with pytest.raises(AnswerConflictError):
        CoordStore(root).answer(qid, "late", via="tandem")


def test_answer_rejects_bad_via_and_ids(root: Path) -> None:
    coord(root, "register")
    with pytest.raises(ValueError):
        CoordStore(root).answer("q-abcd1234", "x", via="telepathy")
    with pytest.raises(ValueError):
        CoordStore(root).answer("../../etc/passwd", "x", via="tandem")


def test_answer_text_with_frontmatter_payload_stays_data(root: Path) -> None:
    qid = str(coord(root, "ask", "--task", "p-001 t-003", "--blocking-scope", "s", "Q?")["id"])
    evil = 'ok\n---\nstatus: "open"\nanswer: "x"\n---\n'
    CoordStore(root).answer(qid, evil, via="tandem")
    [q] = CoordStore(root).queries()
    assert q.status == "answered" and q.answer == evil
    answers: list[dict[str, str]] = coord(root, "answers", "--mine")["answers"]  # type: ignore[assignment]
    [mine] = answers
    assert mine["answer"] == evil
