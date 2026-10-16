import os
import shutil
import subprocess
from collections.abc import Iterator
from pathlib import Path

import pytest

from tandem.coordination.store import CoordStore
from tandem.tmux.client import TmuxClient
from tandem.tmux.layout import SESSION, Launch, Layout
from tandem.tmux.tabs import agents_in, visible_pane

pytestmark = pytest.mark.skipif(shutil.which("tmux") is None, reason="tmux not installed")


def sleeper(role: str, index: int, kind: str = "agent", label: str = "") -> Launch:
    return Launch(["sleep", "60"], role, index, label or f"fake·{role}{index}", kind=kind,
                  env={"TANDEM_SESSION_ID": f"{role}-{index}"}, session_id=f"{role}-{index}" if kind == "agent" else "")


@pytest.fixture
def layout(tmp_path: Path) -> Iterator[Layout]:
    socket = f"tandem-test-{os.getpid()}"
    client = TmuxClient(socket=socket)
    lay = Layout(client, coord=CoordStore(tmp_path / "coord"))
    conf = lay.write_conf(tmp_path / "tandem.conf")
    lay.client = TmuxClient(socket=socket, conf=conf)
    yield lay
    subprocess.run(["tmux", "-L", socket, "kill-server"], capture_output=True)


def built(lay: Layout) -> dict[str, str]:
    return lay.build(160, 40, sleeper("planner", 1), sleeper("implementer", 1), sleeper("status", 0, kind="own"))


def test_build_creates_columns_status_and_tags(layout: Layout) -> None:
    ids = built(layout)
    panes = layout.panes()
    assert {p.id for p in panes} >= set(ids.values())
    left = visible_pane(panes, "planner")
    right = visible_pane(panes, "implementer")
    assert left and right and left.id == ids["left"] and right.id == ids["right"]
    out = layout.client.run_checked("list-panes", "-t", f"{SESSION}:main", "-F", "#{pane_id} #{pane_left} #{pane_top}")
    pos = {line.split()[0]: (int(line.split()[1]), int(line.split()[2])) for line in out.splitlines()}
    assert pos[ids["left"]][0] < pos[ids["right"]][0]
    assert pos[ids["status"]][1] > pos[ids["left"]][1]


def test_border_title_lists_tabs(layout: Layout) -> None:
    ids = built(layout)
    fmt = layout.client.run_checked("show-options", "-p", "-t", ids["left"], "pane-border-format")
    assert "PLANNERS" in fmt and "[1 fake·planner1]" in fmt


def test_options_mouse_and_hint_bar(layout: Layout) -> None:
    built(layout)
    assert "on" in layout.client.run_checked("show-options", "-g", "mouse")
    assert "F1 Help" in layout.client.run_checked("show-options", "-g", "status-left")


def test_add_agent_swaps_in_and_tab_navigation(layout: Layout) -> None:
    built(layout)
    new = layout.add_agent(sleeper("planner", 2))
    panes = layout.panes()
    assert visible_pane(panes, "planner").id == new  # type: ignore[union-attr]
    assert len(agents_in(panes, "planner")) == 2
    layout.step("planner", -1)
    assert visible_pane(layout.panes(), "planner").index == 1  # type: ignore[union-attr]
    layout.select("planner", 2)
    assert visible_pane(layout.panes(), "planner").index == 2  # type: ignore[union-attr]
    fmt = layout.client.run_checked("show-options", "-p", "-t", new, "pane-border-format")
    assert "[2 fake·planner2]" in fmt and "1 fake·planner1" in fmt


def test_add_agent_records_session_with_pane(layout: Layout) -> None:
    built(layout)
    new = layout.add_agent(sleeper("implementer", 2))
    assert layout.coord is not None
    sessions = {s.id: s for s in layout.coord.sessions()}
    assert sessions["implementer-2"].pane == new


def test_close_visible_agent_shows_the_other_tab(layout: Layout) -> None:
    built(layout)
    layout.add_agent(sleeper("planner", 2))
    first = next(p for p in agents_in(layout.panes(), "planner") if p.index == 2)
    layout.close_agent(first.id, sleeper("planner", 0, kind="own", label="placeholder"))
    panes = layout.panes()
    assert [p.index for p in agents_in(panes, "planner")] == [1]
    assert visible_pane(panes, "planner").index == 1  # type: ignore[union-attr]
    assert layout.coord is not None and "planner-2" not in {s.id for s in layout.coord.sessions()}


def test_closing_last_agent_leaves_placeholder(layout: Layout) -> None:
    ids = built(layout)
    layout.close_agent(ids["left"], sleeper("planner", 0, kind="own", label="placeholder"))
    shown = visible_pane(layout.panes(), "planner")
    assert shown is not None and shown.kind == "own"
    assert "No planners yet" in layout.client.run_checked("show-options", "-p", "-t", shown.id, "pane-border-format")


def test_adding_to_empty_column_replaces_placeholder(layout: Layout) -> None:
    ids = built(layout)
    layout.close_agent(ids["left"], sleeper("planner", 0, kind="own", label="placeholder"))
    new = layout.add_agent(sleeper("planner", 1))
    panes = layout.panes()
    assert visible_pane(panes, "planner").id == new  # type: ignore[union-attr]
    assert not [p for p in panes if p.kind == "own" and p.role == "planner"]


def test_exited_agent_stays_and_shows_exit_code(layout: Layout) -> None:
    built(layout)
    layout.add_agent(Launch(["sh", "-c", "exit 3"], "planner", 2, "boom", session_id="planner-2"))
    import time
    for _ in range(40):
        dead = [p for p in layout.panes() if p.dead]
        if dead:
            break
        time.sleep(0.1)
    layout.refresh()
    assert dead and dead[0].dead_status == 3
    fmt = layout.client.run_checked("show-options", "-p", "-t", visible_pane(layout.panes(), "planner").id,  # type: ignore[union-attr]
                                    "pane-border-format")
    assert "exited 3" in fmt


def test_restart_respawns_dead_pane(layout: Layout) -> None:
    built(layout)
    new = layout.add_agent(Launch(["sh", "-c", "exit 1"], "planner", 2, "boom", session_id="planner-2"))
    import time
    for _ in range(40):
        if any(p.dead for p in layout.panes()):
            break
        time.sleep(0.1)
    layout.restart(new, coord_dir="/tmp/coord")
    time.sleep(0.2)
    assert new in {p.id for p in layout.panes()}


def test_reflow_stacks_and_unstacks(layout: Layout) -> None:
    ids = built(layout)

    def lefts() -> dict[str, str]:
        out = layout.client.run_checked("list-panes", "-t", f"{SESSION}:main", "-F", "#{pane_id} #{pane_left}")
        return dict(line.split(" ", 1) for line in out.splitlines())

    layout.reflow(80)
    assert lefts()[ids["left"]] == lefts()[ids["right"]]
    layout.reflow(160)
    assert lefts()[ids["left"]] != lefts()[ids["right"]]


def test_focus_selects_column_pane(layout: Layout) -> None:
    ids = built(layout)
    layout.focus("implementer")
    active = layout.client.run_checked("display-message", "-p", "-t", SESSION, "#{pane_id}").strip()
    assert active == ids["right"]


def test_user_tmux_conf_is_not_loaded(layout: Layout, tmp_path: Path) -> None:
    built(layout)
    assert "tandem.conf" in str(layout.client.conf)


def test_first_run_hook_registered(layout: Layout) -> None:
    built(layout)
    hooks = layout.client.run_checked("show-options", "-g", "client-attached")
    assert "first-run" in hooks
