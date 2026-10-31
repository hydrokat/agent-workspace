from tandem.tmux.tabs import (
    FORMAT, PaneInfo, agents_in, neighbor, parse_panes, render_title,
    tab_text, visible_pane,
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


def test_tab_text_shows_index_and_label() -> None:
    assert tab_text(pane("%1", "main", "planner", 2, "claude·opus")) == "2 claude·opus"


def test_tab_text_marks_exited_agents() -> None:
    dead = PaneInfo("%1", "main", "agent", "planner", 3, "agy", True, 7)
    assert tab_text(dead) == "⚠3 agy(exited 7)"
    unknown = PaneInfo("%1", "main", "agent", "planner", 3, "agy", True, None)
    assert tab_text(unknown) == "⚠3 agy(exited ?)"


def test_tab_text_replaces_unsafe_characters() -> None:
    assert "#" not in tab_text(pane("%1", "main", "planner", 1, "a#{x}b"))


def test_render_title_uses_tab_text_for_every_agent() -> None:
    panes = [pane("%1", "main", "planner", 1, "claude"), pane("%2", "stash", "planner", 2, "agy")]
    title = render_title(panes, "planner")
    assert tab_text(panes[1]) in title and f"[{tab_text(panes[0])}]" in title
