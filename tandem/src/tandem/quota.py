"""Subscription quota for the bottom status line.

The only source is the data Claude Code itself passes to a configured statusline command
(``rate_limits.five_hour`` / ``seven_day``: documented, supplied by the CLI). tandem never reads a pane,
an OAuth token, or any usage endpoint. Antigravity documents no permitted source, so it is always
"quota not available".
"""
import json
import os
import re
import subprocess
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

SESSION_ID = re.compile(r"^[a-z]+-\d{1,2}$")
MAX_INPUT = 1_000_000
CHAIN_TIMEOUT = 5.0
STALE_AFTER = 600.0
NOT_AVAILABLE = "quota not available"
FEED_COMMAND = "quota-feed"


@dataclass(frozen=True)
class Window:
    used_percentage: float
    resets_at: int


@dataclass(frozen=True)
class ClaudeQuota:
    five_hour: Window | None
    seven_day: Window | None
    updated: float


def quota_dir(coord_dir: Path) -> Path:
    return coord_dir / "quota"


def _window(raw: object) -> Window | None:
    if not isinstance(raw, dict):
        return None
    used, resets = raw.get("used_percentage"), raw.get("resets_at")
    if isinstance(used, bool) or not isinstance(used, (int, float)) or not 0 <= used <= 1000:
        return None
    if isinstance(resets, bool) or not isinstance(resets, (int, float)) or not 0 < resets < 4_102_444_800:
        return None
    return Window(float(used), int(resets))


def parse_statusline(data: object, now: float) -> ClaudeQuota | None:
    """The two documented windows from a statusline payload; None when the payload has neither."""
    limits = data.get("rate_limits") if isinstance(data, dict) else None
    if not isinstance(limits, dict):
        return None
    five, seven = _window(limits.get("five_hour")), _window(limits.get("seven_day"))
    return ClaudeQuota(five, seven, now) if five or seven else None


def _to_json(q: ClaudeQuota) -> dict[str, Any]:
    return {"updated": q.updated,
            **{name: {"used_percentage": w.used_percentage, "resets_at": w.resets_at}
               for name, w in (("five_hour", q.five_hour), ("seven_day", q.seven_day)) if w}}


def write_feed(coord_dir: Path, session_id: str, quota: ClaudeQuota) -> None:
    """Atomic, owner-only; the file holds percentages and reset times only."""
    if not SESSION_ID.match(session_id):
        raise ValueError("invalid session id")
    folder = quota_dir(coord_dir)
    folder.mkdir(parents=True, exist_ok=True, mode=0o700)
    fd, tmp = tempfile.mkstemp(dir=folder, prefix=".q-", suffix=".tmp")
    try:
        with os.fdopen(fd, "w") as handle:
            json.dump(_to_json(quota), handle, allow_nan=False)
        os.chmod(tmp, 0o600)
        os.replace(tmp, folder / f"claude-{session_id}.json")
    except BaseException:
        Path(tmp).unlink(missing_ok=True)
        raise


def read_claude(coord_dir: Path) -> ClaudeQuota | None:
    """Freshest feed across Claude sessions (they share one account)."""
    best: ClaudeQuota | None = None
    folder = quota_dir(coord_dir)
    if not folder.is_dir():
        return None
    for path in folder.glob("claude-*.json"):
        try:
            raw = json.loads(path.read_text())
            updated = raw.get("updated")
            found = parse_statusline({"rate_limits": raw}, float(updated) if isinstance(updated, (int, float)) else 0.0)
        except (OSError, ValueError, AttributeError):
            continue
        if found is not None and (best is None or found.updated > best.updated):
            best = found
    return best


def _when(epoch: int, now: float) -> str:
    local = time.localtime(epoch)
    return time.strftime("%H:%M", local) if epoch - now < 86400 else time.strftime("%a %H:%M", local)


def _claude_text(q: ClaudeQuota | None, now: float) -> str:
    windows = [(label, w) for label, w in (("5h", q.five_hour if q else None), ("7d", q.seven_day if q else None))
               if w is not None and w.resets_at > now]
    if q is None or not windows:
        return f"claude: {NOT_AVAILABLE}"
    text = " · ".join(f"{label} {w.used_percentage:.0f}% (resets {_when(w.resets_at, now)})" for label, w in windows)
    if now - q.updated > STALE_AFTER:
        text += f" [as of {time.strftime('%H:%M', time.localtime(q.updated))}]"
    return f"claude: {text}"


def format_line(clis: list[str], claude: ClaudeQuota | None, now: float) -> str:
    """One status line for the CLIs that currently have agents; every other CLI is 'quota not available'."""
    parts = []
    if "claude" in clis:
        parts.append(_claude_text(claude, now))
    if "agy" in clis:
        parts.append(f"agy: {NOT_AVAILABLE}")
    line = "Quota  " + "  │  ".join(parts) if parts else f"Quota: {NOT_AVAILABLE} (no agents running)"
    return tmux_safe(line)


def tmux_safe(text: str) -> str:
    """tmux expands ``#`` sequences in #() output; neutralize them and drop control characters."""
    return re.sub(r"[\x00-\x1f\x7f]", "", text).replace("#", "##")


def user_statusline(cwd: Path, home: Path) -> str | None:
    """The statusline command the user already has (local, project, then user settings), if any."""
    for path in (cwd / ".claude" / "settings.local.json", cwd / ".claude" / "settings.json",
                 home / ".claude" / "settings.json"):
        try:
            line = json.loads(path.read_text()).get("statusLine")
        except (OSError, ValueError, AttributeError):
            continue
        if isinstance(line, dict) and line.get("type") == "command" and isinstance(line.get("command"), str):
            command: str = line["command"]
            return None if FEED_COMMAND in command else command
    return None


def chain(command: str | None, stdin_text: str) -> str:
    """Run the user's own statusline command (as Claude Code would) on the same input so tandem never hides it."""
    if not command:
        return ""
    try:
        done = subprocess.run(["/bin/sh", "-c", command], input=stdin_text, capture_output=True, text=True,
                              timeout=CHAIN_TIMEOUT)
    except (OSError, subprocess.SubprocessError):
        return ""
    return done.stdout


def run_feed(stdin_text: str, coord_dir: Path, session_id: str, cwd: Path, home: Path, now: float) -> str:
    """Statusline entry point: record the quota fields, then return what the user's own statusline prints."""
    if len(stdin_text) <= MAX_INPUT:
        try:
            quota = parse_statusline(json.loads(stdin_text), now)
            if quota is not None:
                write_feed(coord_dir, session_id, quota)
        except (ValueError, OSError):
            pass
    return chain(user_statusline(cwd, home), stdin_text)
