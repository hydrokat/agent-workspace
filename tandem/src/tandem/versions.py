import re
from dataclasses import dataclass
from pathlib import Path

_VERSION_RE = re.compile(r"(\d+(?:\.\d+)*)([a-z]?)")
_MIN_LINE_RE = re.compile(r"^[A-Z_]+=[0-9A-Za-z.\-]+$")


class InvalidMinVersionsError(ValueError):
    """Raised when min-versions.env contains a line outside the strict grammar."""


@dataclass(frozen=True)
class Version:
    parts: tuple[int, ...]
    suffix: str


def parse_version(raw: str) -> Version | None:
    """Extract the first version-like token (e.g. ``3.3a``, ``2.1.301``) from tool output."""
    match = _VERSION_RE.search(raw)
    if match is None:
        return None
    parts = tuple(int(p) for p in match.group(1).split("."))
    return Version(parts, match.group(2))


def _padded(parts: tuple[int, ...], length: int) -> tuple[int, ...]:
    return parts + (0,) * (length - len(parts))


def is_at_least(actual: Version, minimum: Version) -> bool:
    """Return True when ``actual`` >= ``minimum``; missing components count as zero."""
    length = max(len(actual.parts), len(minimum.parts))
    return (_padded(actual.parts, length), actual.suffix) >= (
        _padded(minimum.parts, length),
        minimum.suffix,
    )


def parse_min_versions(path: Path) -> dict[str, str]:
    """Parse ``min-versions.env`` as data only; never evaluates content."""
    result: dict[str, str] = {}
    for number, line in enumerate(path.read_text().splitlines(), start=1):
        if not line.strip() or line.startswith("#"):
            continue
        if not _MIN_LINE_RE.match(line):
            raise InvalidMinVersionsError(f"{path.name}:{number}: invalid line")
        key, value = line.split("=", 1)
        result[key] = value
    return result
