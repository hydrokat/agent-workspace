import posixpath
from dataclasses import dataclass

from tandem.coordination.schema import Claim


@dataclass(frozen=True)
class Conflict:
    first: str
    second: str
    path: str


def _norm(path: str) -> str:
    return posixpath.normpath(path.replace("\\", "/")).strip("/")


def _overlap(a: str, b: str) -> str | None:
    if not a or not b or a == "." or b == ".":
        return None
    if a == b or b.startswith(a + "/"):
        return a
    if a.startswith(b + "/"):
        return b
    return None


def find_conflicts(claims: list[Claim]) -> list[Conflict]:
    """Pairs of claimed tasks whose Affected Paths equal or contain one another."""
    found: list[Conflict] = []
    for i, first in enumerate(claims):
        for second in claims[i + 1:]:
            for pa in first.paths:
                hit = next((o for pb in second.paths if (o := _overlap(_norm(pa), _norm(pb)))), None)
                if hit:
                    found.append(Conflict(first.task, second.task, hit))
                    break
    return found
