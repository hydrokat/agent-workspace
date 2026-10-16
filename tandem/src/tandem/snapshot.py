import time
from dataclasses import dataclass, field
from pathlib import Path

from tandem.coordination.schema import STALE_SECONDS, Claim, Query, Session
from tandem.coordination.store import CoordStore
from tandem.sources.codebases import Codebase, list_codebases
from tandem.sources.conflicts import Conflict, find_conflicts
from tandem.sources.gitinfo import GitInfo, git_info
from tandem.sources.inprogress import InProgressEntry, parse_in_progress
from tandem.sources.specs import Phase, load_phases


@dataclass(frozen=True)
class StatusSnapshot:
    phases: list[Phase] = field(default_factory=list)
    entries: list[InProgressEntry] = field(default_factory=list)
    sessions: list[Session] = field(default_factory=list)
    claims: list[Claim] = field(default_factory=list)
    queries: list[Query] = field(default_factory=list)
    conflicts: list[Conflict] = field(default_factory=list)
    codebases: list[Codebase] = field(default_factory=list)
    workspace_git: GitInfo | None = None
    codebase_git: dict[str, GitInfo | None] = field(default_factory=dict)
    coord_exists: bool = False
    now: float = 0.0

    @property
    def open_queries(self) -> int:
        return sum(1 for q in self.queries if q.status == "open")

    def stale(self, heartbeat: float) -> bool:
        return self.now - heartbeat > STALE_SECONDS

    @property
    def stale_claims(self) -> list[Claim]:
        return [c for c in self.claims if self.stale(c.heartbeat)]


def collect(workspace: Path, store: CoordStore, git_timeout: float = 3.0) -> StatusSnapshot:
    """Read-only gather of everything the Status tab shows; every source tolerates missing data."""
    codebases = list_codebases(workspace)
    claims = store.claims()
    return StatusSnapshot(
        phases=load_phases(workspace),
        entries=parse_in_progress(workspace / "specs" / "in-progress.md"),
        sessions=store.sessions(), claims=claims, queries=store.queries(),
        conflicts=find_conflicts(claims), codebases=codebases,
        workspace_git=git_info(workspace, git_timeout),
        codebase_git={c.name: git_info(c.path, git_timeout) for c in codebases if c.is_git},
        coord_exists=store.exists(), now=time.time(),
    )
