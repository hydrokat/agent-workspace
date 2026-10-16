import re
from dataclasses import dataclass, field
from pathlib import Path

from tandem.sanitize import clean

_EXAMPLE_IDS = ("abc-1234", "def-5678")
_HEADING = re.compile(r"^###\s+(.+?)\s+[—-]\s+(.+)$")
_FIELD = re.compile(r"^- \*\*([^*:]+):\*\*\s*(.*)$")


@dataclass(frozen=True)
class InProgressEntry:
    spec: str
    title: str
    fields: dict[str, str] = field(default_factory=dict)


def parse_in_progress(path: Path) -> list[InProgressEntry]:
    """Parse ``specs/in-progress.md`` entries; the template's example entries are skipped."""
    try:
        text = path.read_text(errors="replace")[:1_000_000]
    except OSError:
        return []
    entries: list[InProgressEntry] = []
    current: tuple[str, str, dict[str, str]] | None = None
    for line in text.splitlines():
        heading = _HEADING.match(line)
        if heading:
            if current:
                entries.append(InProgressEntry(*current))
            current = (clean(heading.group(1)), clean(heading.group(2)), {})
            continue
        if line.startswith("#"):
            if current:
                entries.append(InProgressEntry(*current))
            current = None
            continue
        match = _FIELD.match(line)
        if match and current:
            current[2][match.group(1).strip()] = clean(match.group(2)).strip().strip("`")
    if current:
        entries.append(InProgressEntry(*current))
    return [e for e in entries if not any(i in " ".join(e.fields.values()) for i in _EXAMPLE_IDS)]
