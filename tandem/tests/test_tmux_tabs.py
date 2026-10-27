from tandem.tmux.tabs import (
    FORMAT, PaneInfo, TabSpan, agents_in, neighbor, parse_panes, render_title,
    tab_at, title_spans, visible_pane,
)


def pane(pid: str, window: str, role: str, index: int, label: str, kind: str = "agent",
         dead: bool = False, status: int | None = None) -> PaneInfo:
    return PaneInfo(pid, window, kind, role, index, label, dead, status)


PANES = [
    pane("%1", "main", "planner", 1, "claude·opus"),
    pane("%2", "main", "implementer", 1, "claude·sonnet"),
    pane("%3", "main", "status", 0, "", kind="own"),
    pane("%4", "w-planner-2", "planner", 2, "agy·argon"),
    pane("%5", "w-planner-3", "planner", 3, "agy·flash", dead=True, status=1),
]


def test_parse_panes_round_trip() -> None:
    text = "\n".join("\t".join([p.id, p.window, p.kind, p.role, str(p.index), p.label,
                               "1" if p.dead else "0", str(p.dead_status or ""), p.session]) for p in PANES)
    assert parse_panes(text) == PANES


def test_parse_panes_skips_garbage_and_untagged() -> None:
    assert parse_panes("garbage\n%9\tmain\t\t\t\t\t0\t\n\n") == []


def test_format_uses_user_options() -> None:
    assert "@tandem_role" in FORMAT and "pane_dead_status" in FORMAT


def test_visible_pane_is_the_one_in_main() -> None:
    assert visible_pane(PANES, "planner") == PANES[0]
    assert visible_pane(PANES, "implementer") == PANES[1]


def test_agents_in_sorted_and_excludes_placeholders() -> None:
    mixed = PANES + [pane("%6", "main", "x", 0, "", kind="own")]
    assert [p.id for p in agents_in(mixed, "planner")] == ["%1", "%4", "%5"]


def test_neighbor_wraps() -> None:
    assert neighbor(PANES, "planner", 1).id == "%4"  # type: ignore[union-attr]
    assert neighbor(PANES, "planner", -1).id == "%5"  # type: ignore[union-attr]
    moved = [p for p in PANES]
    assert neighbor(moved, "implementer", 1).id == "%2"  # type: ignore[union-attr]


def test_neighbor_none_when_column_empty() -> None:
    assert neighbor(PANES, "nobody", 1) is None


def test_title_marks_visible_and_exited() -> None:
    title = render_title(PANES, "planner")
    assert "PLANNERS" in title and "[1 claude·opus]" in title and "2 agy·argon" in title
    assert "⚠3 agy·flash(exited 1)" in title
    assert "#{?pane_active,▶ ,}" in title


def test_title_empty_column() -> None:
    assert "No planners yet" in render_title([], "planner")


def test_title_escapes_tmux_format_characters() -> None:
    evil = [pane("%1", "main", "planner", 1, "a#{x}b,c}")]
    title = render_title(evil, "planner")
    assert "#{x}" not in title.replace("#{?pane_active,▶ ,}", "")
    assert "a" in title


# --- title_spans and tab_at ---

def test_title_spans_empty_column_returns_empty_list() -> None:
    assert title_spans([], "planner", active=True) == []
    assert title_spans([], "planner", active=False) == []


def test_title_spans_active_column_single_agent() -> None:
    # Active column: head = "─ ▶ PLANNERS ─" = 14 chars, " " = 1, start = 15
    # tab "[1 opus]" = 8 chars → span (15, 23)
    p = pane("%1", "main", "planner", 1, "opus")
    spans = title_spans([p], "planner", active=True)
    assert spans == [TabSpan(15, 23, "%1")]


def test_title_spans_inactive_column_single_agent() -> None:
    # Inactive: head = "─ PLANNERS ─" = 12 chars, " " = 1, start = 13
    # tab "[1 opus]" = 8 chars → span (13, 21)
    p = pane("%1", "main", "planner", 1, "opus")
    spans = title_spans([p], "planner", active=False)
    assert spans == [TabSpan(13, 21, "%1")]


def test_title_spans_current_tab_has_brackets() -> None:
    # visible pane is %1 (in "main") → its tab text is "[1 claude·opus]"
    spans = title_spans(PANES, "planner", active=True)
    current_span = next(s for s in spans if s.pane_id == "%1")
    width = current_span.end - current_span.start
    # "[1 claude·opus]" = 15 chars
    assert width == 15


def test_title_spans_non_current_tab_has_no_brackets() -> None:
    spans = title_spans(PANES, "planner", active=True)
    tab2 = next(s for s in spans if s.pane_id == "%4")
    # "2 agy·argon" = 11 chars (no brackets)
    assert tab2.end - tab2.start == 11


def test_title_spans_exited_tab_includes_warning_and_exit_code() -> None:
    spans = title_spans(PANES, "planner", active=True)
    dead_span = next(s for s in spans if s.pane_id == "%5")
    # "⚠3 agy·flash(exited 1)" = 22 chars
    assert dead_span.end - dead_span.start == 22


def test_title_spans_many_tabs_positions_are_consecutive_with_two_space_gap() -> None:
    spans = title_spans(PANES, "planner", active=True)
    assert len(spans) == 3
    # Verify two-space gap between consecutive spans
    for a, b in zip(spans, spans[1:]):
        assert b.start == a.end + 2


def test_title_spans_active_marker_shifts_all_spans() -> None:
    # Active adds "▶ " (2 chars) to the head, shifting all spans by 2
    panes_one = [pane("%1", "main", "planner", 1, "opus")]
    inactive = title_spans(panes_one, "planner", active=False)
    active = title_spans(panes_one, "planner", active=True)
    assert active[0].start == inactive[0].start + 2
    assert active[0].end == inactive[0].end + 2


def test_title_spans_agree_with_render_title() -> None:
    """Each span [start, end) must extract the exact tab text from render_title."""
    for active in (True, False):
        spans = title_spans(PANES, "planner", active=active)
        raw = render_title(PANES, "planner")
        resolved = raw.replace("#{?pane_active,▶ ,}", "▶ " if active else "")
        agents = agents_in(PANES, "planner")
        assert len(spans) == len(agents)
        current = visible_pane(PANES, "planner")
        for span, agent in zip(spans, agents):
            extracted = resolved[span.start:span.end]
            assert str(agent.index) in extracted
            assert agent.label in extracted
            if agent.dead:
                assert "exited" in extracted
            if current and agent.id == current.id:
                assert extracted.startswith("[") and extracted.endswith("]")
            else:
                assert not extracted.startswith("[")


def test_tab_at_finds_correct_pane_within_span() -> None:
    spans = title_spans(PANES, "planner", active=True)
    assert len(spans) == 3
    s0, s1, s2 = spans
    assert tab_at(spans, s0.start) == s0.pane_id
    assert tab_at(spans, s0.end - 1) == s0.pane_id
    assert tab_at(spans, s1.start) == s1.pane_id
    assert tab_at(spans, s2.start) == s2.pane_id
    assert tab_at(spans, s2.end - 1) == s2.pane_id


def test_tab_at_returns_none_in_head_and_gap() -> None:
    spans = title_spans(PANES, "planner", active=True)
    s0, s1 = spans[0], spans[1]
    # Head area (before first span)
    assert tab_at(spans, 0) is None
    assert tab_at(spans, s0.start - 1) is None
    # Gap between spans (exactly at s0.end and s0.end+1)
    assert tab_at(spans, s0.end) is None
    assert tab_at(spans, s0.end + 1) is None
    assert tab_at(spans, s1.start - 1) is None
    # Past the last span
    assert tab_at(spans, spans[-1].end) is None


def test_tab_at_empty_spans_always_returns_none() -> None:
    assert tab_at([], 0) is None
    assert tab_at([], 100) is None
