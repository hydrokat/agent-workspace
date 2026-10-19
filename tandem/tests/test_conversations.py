import json
import os
import stat
from pathlib import Path

import pytest

from tandem import conversations as conv

ID = "46f2fc12-7f1f-4ca2-b714-06cbbfd669e9"
OTHER = "0b9a3c1e-1111-4222-8333-444455556666"


def test_valid_id_accepts_uuids_only() -> None:
    assert conv.valid_id(ID.upper()) == ID
    for bad in ("", "abc", f"{ID}x", f"../{ID}", None, 3):
        assert conv.valid_id(bad) is None
    assert conv.valid_id(conv.new_id()) is not None


def test_record_round_trip_is_owner_only(tmp_path: Path) -> None:
    conv.record(tmp_path, "planner-1", ID)
    path = tmp_path / "conversations" / "planner-1.json"
    assert conv.recorded(tmp_path, "planner-1") == ID
    assert stat.S_IMODE(os.stat(path).st_mode) == 0o600
    assert set(json.loads(path.read_text())) == {"id", "updated"}


@pytest.mark.parametrize("session", ["../x", "planner", "Planner-1", "planner-100"])
def test_session_ids_are_checked(tmp_path: Path, session: str) -> None:
    with pytest.raises(ValueError):
        conv.record(tmp_path, session, ID)
    assert conv.current(tmp_path, session, "claude") is None


def test_invalid_conversation_is_never_recorded(tmp_path: Path) -> None:
    with pytest.raises(ValueError):
        conv.record(tmp_path, "planner-1", "not-a-uuid")


def test_agy_log_gives_the_latest_conversation(tmp_path: Path) -> None:
    log = conv.agy_log_path(tmp_path, "implementer-2")
    log.parent.mkdir(parents=True)
    log.write_text(f"I1017 server.go:1199] Created conversation {ID} (backend=X)\n"
                   f"I1017 conversation_manager.go:973] Streaming conversation {OTHER}\n"
                   "I1017 something else\n")
    assert conv.current(tmp_path, "implementer-2", "agy") == OTHER


def test_agy_log_only_reads_the_tail(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(conv, "LOG_TAIL", 200)
    log = conv.agy_log_path(tmp_path, "planner-1")
    log.parent.mkdir(parents=True)
    log.write_text(f"Created conversation {ID}\n" + "x" * 400 + "\n")
    assert conv.from_agy_log(log) is None


def test_missing_sources_mean_unknown(tmp_path: Path) -> None:
    assert conv.current(tmp_path, "planner-1", "claude") is None
    assert conv.current(tmp_path, "planner-1", "agy") is None
    assert conv.current(tmp_path, "planner-1", "other") is None


def test_forget_removes_record_and_log(tmp_path: Path) -> None:
    conv.record(tmp_path, "planner-1", ID)
    conv.agy_log_path(tmp_path, "planner-1").write_text(f"Created conversation {ID}\n")
    conv.forget(tmp_path, "planner-1")
    assert list((tmp_path / "conversations").iterdir()) == []
    conv.forget(tmp_path / "missing", "planner-1")


def test_claude_resume_needs_a_saved_transcript(tmp_path: Path) -> None:
    assert not conv.can_resume("claude", ID, tmp_path)
    (tmp_path / "projects" / "-Users-me-ws").mkdir(parents=True)
    (tmp_path / "projects" / "-Users-me-ws" / f"{ID}.jsonl").write_text("{}\n")
    assert conv.can_resume("claude", ID, tmp_path)
    assert not conv.can_resume("claude", OTHER, tmp_path)
    assert not conv.can_resume("claude", None, tmp_path)


def test_agy_resumes_any_known_conversation(tmp_path: Path) -> None:
    assert conv.can_resume("agy", ID, tmp_path)
    assert not conv.can_resume("agy", None, tmp_path)
    assert not conv.can_resume("agy", "bad", tmp_path)
