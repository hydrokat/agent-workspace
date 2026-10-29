import os
import re
import time
from dataclasses import dataclass
from pathlib import Path

from tandem.sanitize import clean
from tandem.versions import Version, is_at_least, parse_version

_NOTICE_LINE_RE = re.compile(r"^[A-Z_]+=[0-9A-Za-z._-]+$")
_KNOWN_KEYS = frozenset(
    {"TANDEM_UPDATE_FROM", "TANDEM_UPDATE_TO", "TANDEM_UPDATE_AT", "TANDEM_UPDATE_SEEN"}
)
_REQUIRED_KEYS = frozenset({"TANDEM_UPDATE_FROM", "TANDEM_UPDATE_TO", "TANDEM_UPDATE_AT"})

_SECTION_RE = re.compile(r"^## \[([^\]]+)\]")
_BULLET_RE = re.compile(r"^[\-\*] ")
_BOLD_RE = re.compile(r"^\*\*(.+?)\*\*")


@dataclass(frozen=True)
class Notice:
    from_version: str
    to_version: str
    at: int
    seen: bool = False


def notice_path(workspace: Path) -> Path:
    return workspace / ".agent-workspace" / "update-notice.env"


def read_notice(workspace: Path) -> Notice | None:
    """Parse the update notice file as data; return None on any error or invalid line."""
    path = notice_path(workspace)
    try:
        text = path.read_text()
    except OSError:
        return None
    data: dict[str, str] = {}
    for line in text.splitlines():
        if not line.strip():
            continue
        if not _NOTICE_LINE_RE.match(line):
            return None
        key, value = line.split("=", 1)
        if key not in _KNOWN_KEYS:
            return None
        data[key] = value
    if not _REQUIRED_KEYS.issubset(data):
        return None
    try:
        return Notice(
            from_version=data["TANDEM_UPDATE_FROM"],
            to_version=data["TANDEM_UPDATE_TO"],
            at=int(data["TANDEM_UPDATE_AT"]),
            seen=data.get("TANDEM_UPDATE_SEEN") == "1",
        )
    except (KeyError, ValueError):
        return None


def _write_atomic(path: Path, text: str) -> None:
    """Replace the notice in one step so a concurrent reader never sees a partial file."""
    tmp = path.with_suffix(".env.tmp")
    tmp.write_text(text)
    os.replace(tmp, path)


def write_notice(workspace: Path, from_version: str, to_version: str,
                 at: int | None = None) -> None:
    path = notice_path(workspace)
    path.parent.mkdir(parents=True, exist_ok=True)
    ts = at if at is not None else int(time.time())
    _write_atomic(
        path,
        f"TANDEM_UPDATE_FROM={from_version}\n"
        f"TANDEM_UPDATE_TO={to_version}\n"
        f"TANDEM_UPDATE_AT={ts}\n",
    )


def mark_seen(workspace: Path) -> None:
    """Persist the seen flag; a missing or already-seen notice is a no-op."""
    notice = read_notice(workspace)
    if notice is None or notice.seen:
        return
    _write_atomic(
        notice_path(workspace),
        f"TANDEM_UPDATE_FROM={notice.from_version}\n"
        f"TANDEM_UPDATE_TO={notice.to_version}\n"
        f"TANDEM_UPDATE_AT={notice.at}\n"
        f"TANDEM_UPDATE_SEEN=1\n",
    )


def _headline(bullet: str) -> str:
    """Extract the short headline from a changelog bullet line."""
    text = _BULLET_RE.sub("", bullet.strip())
    m = _BOLD_RE.match(text)
    if m:
        return clean(m.group(1).strip())
    period = text.find(".")
    if period >= 0:
        return clean(text[: period + 1])
    return clean(text)


def whats_new(
    changelog_text: str,
    from_v: str,
    to_v: str,
    limit: int = 8,
) -> list[str]:
    """Return up to ``limit`` bullet headlines from changelog sections in (from_v, to_v].

    Falls back to ``[Unreleased]`` when no versioned section matches.
    All text is sanitised with ``tandem.sanitize.clean``.
    """
    from_ver: Version | None = parse_version(from_v)
    to_ver: Version | None = parse_version(to_v)
    if from_ver is None or to_ver is None:
        return []

    sections: list[tuple[str, list[str]]] = []
    current_id: str | None = None
    current_bullets: list[str] = []

    for line in changelog_text.splitlines():
        m = _SECTION_RE.match(line)
        if m:
            if current_id is not None:
                sections.append((current_id, current_bullets))
            current_id = m.group(1)
            current_bullets = []
        elif current_id is not None and _BULLET_RE.match(line):
            current_bullets.append(line)

    if current_id is not None:
        sections.append((current_id, current_bullets))

    matching: list[str] = []
    unreleased: list[str] = []

    for sec_id, bullets in sections:
        if sec_id.lower() == "unreleased":
            unreleased = bullets
            continue
        sec_ver = parse_version(sec_id)
        if sec_ver is None:
            continue
        # section is in (from_v, to_v]: from_v < sec_ver <= to_v
        if not is_at_least(from_ver, sec_ver) and is_at_least(to_ver, sec_ver):
            matching.extend(bullets)

    source = matching if matching else unreleased
    headlines = [_headline(b) for b in source if _BULLET_RE.match(b)]
    return [h for h in headlines if h][:limit]
