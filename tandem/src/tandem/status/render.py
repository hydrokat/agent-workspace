import os

from tandem.sanitize import clean
from tandem.sources.gitinfo import GitInfo
from tandem.snapshot import StatusSnapshot


def age(seconds: float) -> str:
    seconds = max(0, int(seconds))
    if seconds < 90:
        return f"{seconds}s"
    if seconds < 5400:
        return f"{seconds // 60}m"
    return f"{seconds // 3600}h"


def bar(done: int, total: int, width: int = 12) -> str:
    filled = round(width * done / total) if total else 0
    return "█" * filled + "░" * (width - filled)


def summary_line(s: StatusSnapshot) -> str:
    done = sum(p.done for p in s.phases)
    total = sum(p.total for p in s.phases)
    parts = [f"{done}/{total} tasks", _count(len(s.sessions), "agent", "agents"), _count(len(s.claims), "claim", "claims")]
    if s.stale_claims:
        parts.append(f"WARNING {len(s.stale_claims)} stale")
    if s.conflicts:
        parts.append(f"WARNING {len(s.conflicts)} conflicts")
    parts.append(f"{s.open_queries} open " + ("query" if s.open_queries == 1 else "queries"))
    return " · ".join(parts)


def session_row(s: StatusSnapshot, index: int) -> str:
    sess = s.sessions[index]
    state = "stale" if s.stale(sess.heartbeat) else "heartbeat"
    where = f"  branch {clean(sess.branch)}" if sess.branch else ""
    task = clean(sess.task) or "idle"
    return (f"{clean(sess.id)}  {clean(sess.cli)}·{clean(sess.model) or '?'}  {task}{where}  "
            f"{state} {age(s.now - sess.heartbeat)} ago")


def in_progress_text(s: StatusSnapshot) -> str:
    lines: list[str] = []
    session_ids = {x.id for x in s.sessions}
    for claim in s.claims:
        flag = ""
        if s.stale(claim.heartbeat):
            gone = "" if claim.session in session_ids else f" (session {clean(claim.session)} is gone)"
            flag = f"  WARNING stale: last heartbeat {age(s.now - claim.heartbeat)} ago{gone}"
        lines.append(f"claim {clean(claim.phase)} {clean(claim.task)} by {clean(claim.session)}{flag}")
    for e in s.entries:
        lines.append(f"in-progress.md: {e.spec} — {e.title}")
        for key in ("Ongoing", "Next step"):
            if key in e.fields:
                lines.append(f"    {key}: {e.fields[key]}")
    if not lines:
        lines.append("Nothing in progress.")
    if not s.coord_exists:
        lines.append("Coordination folder not found: sessions unknown. Agents register via the agent-coordination skill.")
    return "\n".join(lines)


def phases_text(s: StatusSnapshot) -> str:
    if not s.phases:
        return "No specs yet. A planner can create one."
    lines: list[str] = []
    for p in s.phases:
        if p.warning:
            lines.append(f"WARNING {p.warning}")
            continue
        lines.append(f"{p.name}  {bar(p.done, p.total)}  {p.done}/{p.total}")
        for t in p.tasks:
            lines.append(f"    {t.id}  {t.status:<11} {t.title}")
    return "\n".join(lines)


MAX_IDLE_BRANCHES = 5
MIN_WIDTH = 24


def _count(n: int, one: str, many: str) -> str:
    return f"{n} {one if n == 1 else many}"


def fit(text: str, width: int) -> str:
    """Shorten ``text`` to ``width`` columns, keeping the end for paths and the start for names."""
    if width <= 1 or len(text) <= width:
        return text
    return text[: width - 1] + "…"


def _worktree_label(path: str, main_path: str) -> str:
    rel = os.path.relpath(path, main_path) if main_path else path
    return clean(rel if len(rel) < len(path) else path)


def repo_lines(name: str, info: GitInfo | None, width: int = 80) -> list[str]:
    """``name`` header, then one row per branch: ``* main checkout``, ``branch → worktree``, idle branches."""
    width = max(width, MIN_WIDTH)
    if info is None:
        return [fit(f"{clean(name)} · not a git repository, or git is unavailable", width)]
    main = next((w for w in info.worktrees if w.main), None)
    main_path = main.path if main else ""
    linked = [w for w in info.worktrees if not w.main]
    checked_out = {w.branch for w in info.worktrees if w.branch}
    idle = [b for b in info.branches if b not in checked_out]
    lines = [fit(f"{clean(name)} · {_count(len(info.branches), 'branch', 'branches')} · "
                f"{_count(len(linked), 'worktree', 'worktrees')}", width)]
    rows: list[tuple[str, str, str]] = []
    if main is not None:
        rows.append(("*", clean(main.branch) or "(detached)", ""))
    rows += [(" ", clean(w.branch) or "(detached)", _worktree_label(w.path, main_path)) for w in linked]
    rows += [(" ", clean(b), "") for b in idle[:MAX_IDLE_BRANCHES]]
    col = min(max((len(b) for _, b, _ in rows), default=0), max(width // 2, 10))
    for marker, branch, tree in rows:
        left = f"  {marker} {fit(branch, col):<{col}}"
        if tree:
            room = max(width - len(left) - 4, 6)
            lines.append(f"{left}  → {fit(tree, room)}")
        else:
            lines.append(left.rstrip())
    if len(idle) > MAX_IDLE_BRANCHES:
        lines.append(f"    +{len(idle) - MAX_IDLE_BRANCHES} more branches")
    return lines


def repos_text(s: StatusSnapshot, width: int = 80) -> str:
    """Workspace and every linked codebase: branch and the worktree it is checked out in."""
    blocks = [repo_lines("workspace", s.workspace_git, width)]
    for c in s.codebases:
        if not c.exists:
            blocks.append([fit(f"{c.name} · WARNING missing target {c.path}", width)])
        elif not c.is_git:
            blocks.append([fit(f"{c.name} · not a git repository", width)])
        else:
            blocks.append(repo_lines(c.name, s.codebase_git.get(c.name), width))
    if not s.codebases:
        blocks.append(["No linked codebases. `init` can link sibling repos into codebase-symlinks/."])
    return "\n\n".join("\n".join(b) for b in blocks)


def conflicts_text(s: StatusSnapshot) -> str:
    if not s.conflicts:
        return "No overlapping Affected Paths between claimed tasks."
    return "\n".join(f"WARNING {c.first} and {c.second} both touch {clean(c.path)}" for c in s.conflicts)
