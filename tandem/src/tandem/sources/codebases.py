from dataclasses import dataclass
from pathlib import Path

from tandem.sanitize import clean


@dataclass(frozen=True)
class Codebase:
    name: str
    path: Path
    exists: bool
    is_git: bool


def list_codebases(workspace: Path) -> list[Codebase]:
    """Entries of ``codebase-symlinks/``; symlinks are resolved for display only, never read from."""
    root = workspace / "codebase-symlinks"
    if not root.is_dir() or root.is_symlink():
        return []
    found: list[Codebase] = []
    for entry in sorted(root.iterdir()):
        target = entry.resolve()
        exists = target.is_dir()
        found.append(Codebase(clean(entry.name), target, exists, exists and (target / ".git").exists()))
    return found
