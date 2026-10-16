from tandem.coordination.schema import Claim, Query, Session
from tandem.snapshot import StatusSnapshot
from tandem.sources.conflicts import Conflict
from tandem.sources.specs import Phase, Task
from tandem.status import render

NOW = 10_000.0


def snap(**kw: object) -> StatusSnapshot:
    return StatusSnapshot(now=NOW, coord_exists=True, **kw)  # type: ignore[arg-type]


def sess(**kw: str | float) -> Session:
    base = dict(id="implementer-1", cli="claude", role="implementer", model="sonnet", pane="%2", cwd="/w",
                branch="b", worktree="/w", task="p-001 t-003", heartbeat=NOW - 12)
    base.update(kw)
    return Session(**base)  # type: ignore[arg-type]


def test_summary_counts() -> None:
    s = snap(phases=[Phase("p", [Task("t-1", "a", "Done"), Task("t-2", "b", "Pending")])],
             sessions=[sess()], claims=[Claim("p", "t-2", "implementer-1", "", "", (), NOW - 5)])
    assert render.summary_line(s) == "1/2 tasks · 1 agents · 1 claims · 0 open queries"


def test_summary_warns_on_stale_and_conflicts() -> None:
    stale = Claim("p", "t-2", "gone", "", "", (), NOW - 5000)
    line = render.summary_line(snap(claims=[stale], conflicts=[Conflict("t-1", "t-2", "x")]))
    assert "WARNING 1 stale" in line and "WARNING 1 conflicts" in line


def test_open_queries_counted() -> None:
    q = Query("q-1", "open", "a", "t", "s", 0.0, "?", (), "", "")
    assert "1 open queries" in render.summary_line(snap(queries=[q]))


def test_session_row_has_words_not_just_symbols() -> None:
    row = render.session_row(snap(sessions=[sess()]), 0)
    assert "heartbeat 12s ago" in row and "claude·sonnet" in row and "p-001 t-003" in row


def test_session_row_stale() -> None:
    assert "stale" in render.session_row(snap(sessions=[sess(heartbeat=NOW - 5000)]), 0)


def test_session_text_is_sanitized() -> None:
    row = render.session_row(snap(sessions=[sess(task="\x1b[31mred\x1b[0m")]), 0)
    assert "\x1b" not in row


def test_stale_claim_names_gone_session() -> None:
    text = render.in_progress_text(snap(claims=[Claim("p-001-x", "t-002", "3f2a", "", "", (), NOW - 840)]))
    assert "WARNING stale" in text and "3f2a is gone" in text


def test_empty_states() -> None:
    assert "Nothing in progress" in render.in_progress_text(snap())
    assert "No specs yet" in render.phases_text(snap())
    assert "No linked codebases" in render.codebases_text(snap())
    assert "Not a git repository" in render.worktrees_text(snap())
    assert "No overlapping" in render.conflicts_text(snap())


def test_missing_coordination_dir_hint() -> None:
    text = render.in_progress_text(StatusSnapshot(now=NOW, coord_exists=False))
    assert "agent-coordination" in text


def test_phase_warning_row_does_not_hide_others() -> None:
    text = render.phases_text(snap(phases=[Phase("bad", warning="bad/impl.md missing"), Phase("ok", [Task("t-1", "x", "Done")])]))
    assert "WARNING bad/impl.md missing" in text and "ok  " in text


def test_bar_and_age() -> None:
    assert render.bar(1, 2, 4) == "██░░" and render.bar(0, 0, 4) == "░░░░"
    assert render.age(12) == "12s" and render.age(840) == "14m" and render.age(7200) == "2h"


def test_conflict_text() -> None:
    assert "t-1 and t-2 both touch src" in render.conflicts_text(snap(conflicts=[Conflict("t-1", "t-2", "src")]))

