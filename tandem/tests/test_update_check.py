import json
import os
from pathlib import Path

import pytest

from tandem.update_check import (
    CheckState,
    DEFAULT_INTERVAL,
    MIN_INTERVAL,
    Runner,
    due,
    installed_version_from_manifest,
    latest_version,
    read_state,
    record,
    should_offer,
    state_path,
    write_state,
)


# ---------------------------------------------------------------------------
# due()
# ---------------------------------------------------------------------------


def test_due_when_never_checked() -> None:
    state = CheckState()
    assert due(state, 1_000_000.0, DEFAULT_INTERVAL) is True


def test_due_when_interval_elapsed() -> None:
    state = CheckState(last=1_000_000.0)
    assert due(state, 1_000_000.0 + DEFAULT_INTERVAL, DEFAULT_INTERVAL) is True


def test_not_due_before_interval() -> None:
    state = CheckState(last=1_000_000.0)
    assert due(state, 1_000_000.0 + DEFAULT_INTERVAL - 1, DEFAULT_INTERVAL) is False


def test_due_at_exact_interval_boundary() -> None:
    state = CheckState(last=1_000_000.0)
    assert due(state, 1_000_000.0 + DEFAULT_INTERVAL, DEFAULT_INTERVAL) is True


def test_due_respects_custom_interval() -> None:
    state = CheckState(last=1_000_000.0)
    assert due(state, 1_000_000.0 + 700, 600) is True
    assert due(state, 1_000_000.0 + 500, 600) is False


# ---------------------------------------------------------------------------
# should_offer()
# ---------------------------------------------------------------------------


def test_offer_when_newer_version_available() -> None:
    assert should_offer("1.3.0", "1.4.0", "") is True


def test_no_offer_when_same_version() -> None:
    assert should_offer("1.3.0", "1.3.0", "") is False


def test_no_offer_when_older_version() -> None:
    assert should_offer("1.4.0", "1.3.0", "") is False


def test_no_offer_when_version_is_skipped() -> None:
    assert should_offer("1.3.0", "1.4.0", "1.4.0") is False


def test_offer_when_skip_matches_different_version() -> None:
    assert should_offer("1.3.0", "1.5.0", "1.4.0") is True


def test_no_offer_when_installed_is_none() -> None:
    assert should_offer(None, "1.4.0", "") is False


def test_no_offer_when_latest_is_none() -> None:
    assert should_offer("1.3.0", None, "") is False


def test_no_offer_when_both_none() -> None:
    assert should_offer(None, None, "") is False


def test_no_offer_when_installed_empty() -> None:
    assert should_offer("", "1.4.0", "") is False


def test_no_offer_when_latest_empty() -> None:
    assert should_offer("1.3.0", "", "") is False


def test_offer_with_patch_bump() -> None:
    assert should_offer("1.3.0", "1.3.1", "") is True


def test_offer_with_major_bump() -> None:
    assert should_offer("1.3.0", "2.0.0", "") is True


# ---------------------------------------------------------------------------
# read_state / write_state
# ---------------------------------------------------------------------------


def test_fresh_state_when_file_missing(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    ws.mkdir()
    state = read_state(ws)
    assert state.last == 0.0
    assert state.latest == ""
    assert state.skip == ""


def test_round_trip_write_read(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    ws.mkdir()
    orig = CheckState(last=1_700_000_000.0, latest="1.4.0", skip="1.3.5")
    write_state(ws, orig)
    back = read_state(ws)
    assert back.last == 1_700_000_000.0
    assert back.latest == "1.4.0"
    assert back.skip == "1.3.5"


def test_write_is_atomic(tmp_path: Path) -> None:
    """write_state must use os.replace so no partial file is ever visible."""
    ws = tmp_path / "ws"
    ws.mkdir()
    write_state(ws, CheckState(last=1.0, latest="1.0.0", skip=""))
    path = state_path(ws)
    tmp = path.with_suffix(".env.tmp")
    assert not tmp.exists()
    assert path.exists()


def test_invalid_line_returns_fresh_state(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    (ws / ".agent-workspace").mkdir(parents=True)
    state_path(ws).write_text("TANDEM_CHECK_LAST=bad line here\n")
    assert read_state(ws) == CheckState()


def test_unknown_key_returns_fresh_state(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    (ws / ".agent-workspace").mkdir(parents=True)
    state_path(ws).write_text("TANDEM_CHECK_LAST=1000\nSECRET=hi\n")
    assert read_state(ws) == CheckState()


def test_empty_file_returns_fresh_state(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    (ws / ".agent-workspace").mkdir(parents=True)
    state_path(ws).write_text("")
    assert read_state(ws) == CheckState()


def test_partial_file_returns_default_values(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    (ws / ".agent-workspace").mkdir(parents=True)
    state_path(ws).write_text("TANDEM_CHECK_LAST=1000\nTANDEM_CHECK_LATEST=1.4.0\n")
    back = read_state(ws)
    assert back.last == 1000.0
    assert back.latest == "1.4.0"
    assert back.skip == ""


# ---------------------------------------------------------------------------
# record()
# ---------------------------------------------------------------------------


def test_record_updates_last_and_latest(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    ws.mkdir()
    state = CheckState()
    new_state = record(ws, state, 1_700_000_000.0, "1.4.0")
    assert new_state.last == 1_700_000_000.0
    assert new_state.latest == "1.4.0"
    assert new_state.skip == ""


def test_record_keeps_skip(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    ws.mkdir()
    state = CheckState(last=0.0, latest="1.3.0", skip="1.3.5")
    new_state = record(ws, state, 1_700_000_000.0, "1.4.0")
    assert new_state.skip == "1.3.5"


def test_record_preserves_latest_when_check_fails(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    ws.mkdir()
    state = CheckState(last=0.0, latest="1.4.0", skip="")
    new_state = record(ws, state, 1_700_000_000.0, None)
    assert new_state.latest == "1.4.0"
    assert new_state.last == 1_700_000_000.0


def test_record_persists_to_disk(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    ws.mkdir()
    record(ws, CheckState(), 1_700_000_000.0, "1.4.0")
    back = read_state(ws)
    assert back.last == 1_700_000_000.0
    assert back.latest == "1.4.0"


# ---------------------------------------------------------------------------
# latest_version()
# ---------------------------------------------------------------------------


def _runner_ok(version: str):  # type: ignore[no-untyped-def]
    def run(argv: list[str], cwd: Path, timeout: float) -> tuple[int, str]:
        return 0, f"agent-workspace {version}\n"
    return run


def _runner_fail() -> Runner:
    def run(argv: list[str], cwd: Path, timeout: float) -> tuple[int, str]:
        return 1, ""
    return run


def test_latest_version_parses_output(tmp_path: Path) -> None:
    result = latest_version("hydrokat/agent-workspace", tmp_path, _runner_ok("1.4.0"))
    assert result == "1.4.0"


def test_latest_version_returns_none_on_failure(tmp_path: Path) -> None:
    result = latest_version("hydrokat/agent-workspace", tmp_path, _runner_fail())
    assert result is None


def test_latest_version_returns_none_on_unparseable_output(tmp_path: Path) -> None:
    def run(argv: list[str], cwd: Path, timeout: float) -> tuple[int, str]:
        return 0, "not a version\n"
    result = latest_version("hydrokat/agent-workspace", tmp_path, run)
    assert result is None


def test_latest_version_passes_spec_and_timeout(tmp_path: Path) -> None:
    calls: list[tuple[list[str], float]] = []

    def run(argv: list[str], cwd: Path, timeout: float) -> tuple[int, str]:
        calls.append((argv, timeout))
        return 0, "1.4.0"

    latest_version("owner/repo#main", tmp_path, run)
    assert calls[0][0] == ["npx", "--yes", "owner/repo#main", "--version"]
    assert calls[0][1] == 60.0


def test_latest_version_failed_npx_still_records_last(tmp_path: Path) -> None:
    """latest_version returning None must not prevent record() from updating TANDEM_CHECK_LAST."""
    ws = tmp_path / "ws"
    ws.mkdir()
    state = CheckState()
    new_state = record(ws, state, 9_000_000.0, None)
    assert new_state.last == 9_000_000.0
    assert new_state.latest == ""


# ---------------------------------------------------------------------------
# installed_version_from_manifest()
# ---------------------------------------------------------------------------


def test_reads_version_from_manifest(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    (ws / ".agent-workspace").mkdir(parents=True)
    (ws / ".agent-workspace" / "manifest.json").write_text(json.dumps({"version": "1.3.0"}))
    assert installed_version_from_manifest(ws) == "1.3.0"


def test_returns_none_when_manifest_missing(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    ws.mkdir()
    assert installed_version_from_manifest(ws) is None


def test_returns_none_when_version_key_absent(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    (ws / ".agent-workspace").mkdir(parents=True)
    (ws / ".agent-workspace" / "manifest.json").write_text(json.dumps({"name": "agent-workspace"}))
    assert installed_version_from_manifest(ws) is None


def test_returns_none_on_invalid_json(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    (ws / ".agent-workspace").mkdir(parents=True)
    (ws / ".agent-workspace" / "manifest.json").write_text("not json")
    assert installed_version_from_manifest(ws) is None
