from tandem.sanitize import clean
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
    parts = [f"{done}/{total} tasks", f"{len(s.sessions)} agents", f"{len(s.claims)} claims"]
    if s.stale_claims:
        parts.append(f"WARNING {len(s.stale_claims)} stale")
    if s.conflicts:
        parts.append(f"WARNING {len(s.conflicts)} conflicts")
    parts.append(f"{s.open_queries} open queries")
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


def codebases_text(s: StatusSnapshot) -> str:
    if not s.codebases:
        return "No linked codebases."
    lines = []
    for c in s.codebases:
        info = s.codebase_git.get(c.name)
        branch = f"  branch {info.current or '(detached)'}" if info else ""
        note = "" if c.exists else "  WARNING missing target"
        lines.append(f"{c.name}  {c.path}{branch}{note}")
    return "\n".join(lines)


def worktrees_text(s: StatusSnapshot) -> str:
    if s.workspace_git is None:
        return "Not a git repository, or git is unavailable."
    return "\n".join(f"{w.path}  {w.branch or '(detached)'}" for w in s.workspace_git.worktrees)


def branches_text(s: StatusSnapshot) -> str:
    if s.workspace_git is None:
        return "Not a git repository, or git is unavailable."
    current = s.workspace_git.current
    return "\n".join(f"{'* ' if b == current else '  '}{b}" for b in s.workspace_git.branches) or "No branches."


def conflicts_text(s: StatusSnapshot) -> str:
    if not s.conflicts:
        return "No overlapping Affected Paths between claimed tasks."
    return "\n".join(f"WARNING {c.first} and {c.second} both touch {clean(c.path)}" for c in s.conflicts)
