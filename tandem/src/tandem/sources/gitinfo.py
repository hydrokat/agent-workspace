import subprocess
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Worktree:
    path: str
    branch: str


@dataclass(frozen=True)
class GitInfo:
    current: str
    branches: list[str]
    worktrees: list[Worktree]


def _git(directory: Path, timeout: float, *args: str) -> str | None:
    try:
        done = subprocess.run(["git", *args], cwd=directory, capture_output=True, text=True,
                              timeout=timeout, stdin=subprocess.DEVNULL)
    except (OSError, subprocess.SubprocessError):
        return None
    return done.stdout if done.returncode == 0 else None


def git_info(directory: Path, timeout: float = 5.0) -> GitInfo | None:
    """Branches and worktrees via argv git calls with a timeout; None when not a usable repo."""
    if not directory.is_dir():
        return None
    current = _git(directory, timeout, "branch", "--show-current")
    branches = _git(directory, timeout, "for-each-ref", "--format=%(refname:short)", "refs/heads")
    trees = _git(directory, timeout, "worktree", "list", "--porcelain")
    if current is None or branches is None or trees is None:
        return None
    worktrees: list[Worktree] = []
    path = ""
    for line in trees.splitlines():
        if line.startswith("worktree "):
            path = line[len("worktree "):]
            worktrees.append(Worktree(path, ""))
        elif line.startswith("branch ") and worktrees:
            worktrees[-1] = Worktree(path, line.rsplit("/", 1)[-1])
    return GitInfo(current.strip(), [b for b in branches.splitlines() if b], worktrees)
