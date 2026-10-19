import json
import os
import time
from pathlib import Path
from typing import Any

import pytest

from tandem import quota

NOW = 1_800_000_000.0
PAYLOAD: dict[str, Any] = {"rate_limits": {"five_hour": {"used_percentage": 41.6, "resets_at": int(NOW) + 3600},
                           "seven_day": {"used_percentage": 12, "resets_at": int(NOW) + 5 * 86400}},
           "model": {"display_name": "x"}, "cost": {"total_cost_usd": 3}}


def test_parse_extracts_only_the_two_documented_windows() -> None:
    q = quota.parse_statusline(PAYLOAD, NOW)
    assert q is not None and q.five_hour == quota.Window(41.6, int(NOW) + 3600) and q.seven_day is not None


@pytest.mark.parametrize("data", [None, [], {}, {"rate_limits": 5}, {"rate_limits": {}},
                                  {"rate_limits": {"five_hour": {"used_percentage": "9", "resets_at": 1}}},
                                  {"rate_limits": {"five_hour": {"used_percentage": True, "resets_at": 1}}},
                                  {"rate_limits": {"five_hour": {"used_percentage": -1, "resets_at": 5}}}])
def test_parse_rejects_missing_or_malformed(data: object) -> None:
    assert quota.parse_statusline(data, NOW) is None


def test_one_window_is_enough() -> None:
    q = quota.parse_statusline({"rate_limits": {"seven_day": PAYLOAD["rate_limits"]["seven_day"]}}, NOW)
    assert q is not None and q.five_hour is None and q.seven_day is not None


def test_feed_file_holds_only_percentages_and_is_private(tmp_path: Path) -> None:
    q = quota.parse_statusline(PAYLOAD, NOW)
    assert q is not None
    quota.write_feed(tmp_path, "planner-1", q)
    path = tmp_path / "quota" / "claude-planner-1.json"
    assert os.stat(path).st_mode & 0o777 == 0o600
    text = path.read_text()
    assert "cost" not in text and "model" not in text and set(json.loads(text)) == {"updated", "five_hour", "seven_day"}
    assert quota.read_claude(tmp_path) == q


def test_bad_session_id_is_refused(tmp_path: Path) -> None:
    q = quota.parse_statusline(PAYLOAD, NOW)
    assert q is not None
    with pytest.raises(ValueError):
        quota.write_feed(tmp_path, "../evil", q)
    assert not (tmp_path / "quota").exists()


def test_read_picks_freshest_and_survives_garbage(tmp_path: Path) -> None:
    old = quota.parse_statusline(PAYLOAD, NOW - 100)
    new = quota.parse_statusline({"rate_limits": {"five_hour": {"used_percentage": 90, "resets_at": int(NOW) + 60}}}, NOW)
    assert old and new
    quota.write_feed(tmp_path, "planner-1", old)
    quota.write_feed(tmp_path, "planner-2", new)
    (tmp_path / "quota" / "claude-implementer-1.json").write_text("{nope")
    assert quota.read_claude(tmp_path) == new
    assert quota.read_claude(tmp_path / "missing") is None


def test_line_shows_percentages_and_reset_times() -> None:
    line = quota.format_line(["claude"], quota.parse_statusline(PAYLOAD, NOW), NOW)
    assert line.startswith("Quota  claude: 5h 42% (resets ") and "7d 12% (resets " in line


def test_line_says_quota_not_available_for_agy_always() -> None:
    assert quota.format_line(["agy"], None, NOW) == "Quota  agy: quota not available"
    both = quota.format_line(["claude", "agy"], None, NOW)
    assert both.count("quota not available") == 2


def test_expired_windows_are_dropped_and_empty_means_not_available() -> None:
    q = quota.ClaudeQuota(quota.Window(50, int(NOW) - 1), None, NOW)
    assert "claude: quota not available" in quota.format_line(["claude"], q, NOW)


def test_stale_feed_is_labelled() -> None:
    q = quota.parse_statusline(PAYLOAD, NOW - 3600)
    assert "[as of " in quota.format_line(["claude"], q, NOW)


def test_no_agents() -> None:
    assert "quota not available (no agents running)" in quota.format_line([], None, NOW)


def test_output_cannot_inject_tmux_formats() -> None:
    assert quota.tmux_safe("#[fg=red]#(rm -rf)\x1b[0m") == "##[fg=red]##(rm -rf)[0m"


def write_settings(path: Path, command: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps({"statusLine": {"type": "command", "command": command}}))


def test_user_statusline_precedence_local_project_user(tmp_path: Path) -> None:
    cwd, home = tmp_path / "p", tmp_path / "h"
    write_settings(home / ".claude" / "settings.json", "echo user")
    assert quota.user_statusline(cwd, home) == "echo user"
    write_settings(cwd / ".claude" / "settings.json", "echo project")
    assert quota.user_statusline(cwd, home) == "echo project"
    write_settings(cwd / ".claude" / "settings.local.json", "echo local")
    assert quota.user_statusline(cwd, home) == "echo local"


def test_user_statusline_never_chains_to_itself(tmp_path: Path) -> None:
    write_settings(tmp_path / ".claude" / "settings.json", "py -m tandem quota-feed")
    assert quota.user_statusline(tmp_path, tmp_path) is None


def test_feed_records_quota_then_runs_the_users_statusline(tmp_path: Path) -> None:
    home = tmp_path / "h"
    write_settings(home / ".claude" / "settings.json", "cat >/dev/null; echo MY-LINE")
    out = quota.run_feed(json.dumps(PAYLOAD), tmp_path / "coord", "planner-1", tmp_path, home, NOW)
    assert out.strip() == "MY-LINE" and quota.read_claude(tmp_path / "coord") is not None


def test_users_statusline_receives_the_same_input(tmp_path: Path) -> None:
    home = tmp_path / "h"
    write_settings(home / ".claude" / "settings.json", "cat")
    assert quota.run_feed('{"a": 1}', tmp_path / "c", "planner-1", tmp_path, home, NOW) == '{"a": 1}'


@pytest.mark.parametrize("text", ["", "not json", "[1]", "{" * 10])
def test_feed_never_raises_on_bad_input(tmp_path: Path, text: str) -> None:
    assert quota.run_feed(text, tmp_path / "c", "planner-1", tmp_path, tmp_path, NOW) == ""
    assert not (tmp_path / "c" / "quota").exists()


def test_feed_survives_unwritable_coord_dir(tmp_path: Path) -> None:
    blocker = tmp_path / "file"
    blocker.write_text("x")
    assert quota.run_feed(json.dumps(PAYLOAD), blocker / "sub", "planner-1", tmp_path, tmp_path, NOW) == ""


def test_hung_user_statusline_is_cut_off(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(quota, "CHAIN_TIMEOUT", 0.3)
    start = time.time()
    assert quota.chain("sleep 5", "{}") == ""
    assert time.time() - start < 3


def test_feed_records_the_panes_current_conversation(tmp_path: Path) -> None:
    from tandem import conversations

    conv = "46f2fc12-7f1f-4ca2-b714-06cbbfd669e9"
    quota.run_feed(json.dumps({"session_id": conv}), tmp_path / "coord", "planner-1", tmp_path, tmp_path, NOW)
    assert conversations.recorded(tmp_path / "coord", "planner-1") == conv
    quota.run_feed(json.dumps({"session_id": "nope"}), tmp_path / "coord", "planner-1", tmp_path, tmp_path, NOW)
    assert conversations.recorded(tmp_path / "coord", "planner-1") == conv
