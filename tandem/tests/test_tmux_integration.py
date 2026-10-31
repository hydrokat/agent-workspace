import os
import shutil
import subprocess
import sys
import uuid
from collections.abc import Iterator
from pathlib import Path

import pytest

from tandem.coordination.store import CoordStore
from tandem.tmux import keys
from tandem.tmux.client import TmuxClient
from tandem.tmux.layout import SESSION, Launch, Layout, conf_text
from tandem.tmux.tabs import agents_in, visible_pane

pytestmark = pytest.mark.skipif(shutil.which("tmux") is None, reason="tmux not installed")


def sleeper(role: str, index: int, kind: str = "agent", label: str = "") -> Launch:
    return Launch(["sleep", "60"], role, index, label or f"fake·{role}{index}", kind=kind,
                  env={"TANDEM_SESSION_ID": f"{role}-{index}"}, session_id=f"{role}-{index}" if kind == "agent" else "")


@pytest.fixture
def layout(tmp_path: Path) -> Iterator[Layout]:
    socket = f"tandem-test-{os.getpid()}-{uuid.uuid4().hex[:8]}"
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
    assert "on" in layout.client.run_checked("show-options", "-g", "extended-keys")
    assert f"{keys.alt_name()}+h Help" in layout.client.run_checked("show-options", "-g", "status-left")


def test_tmux_root_bindings_single_and_multi_modifier(layout: Layout) -> None:
    built(layout)
    for cmd in keys.bind_commands("/fake/python"):
        layout.client.run_checked(*cmd)
    out = subprocess.run(
        ["tmux", "-L", str(layout.client.socket), "list-keys", "-T", "root"],
        capture_output=True, text=True, check=True
    ).stdout
    assert "M-h" in out
    assert "C-M-n" in out or "M-C-n" in out
    if sys.platform == "darwin":
        assert "˙" in out


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


def test_reset_restores_equal_columns_after_manual_resize(layout: Layout) -> None:
    ids = built(layout)

    def width(pane: str) -> int:
        return int(layout.client.run_checked("display-message", "-p", "-t", pane, "#{pane_width}"))

    layout.client.run_checked("resize-pane", "-t", ids["left"], "-x", "20")
    assert width(ids["left"]) < width(ids["right"]) - 20
    layout.reset()
    assert abs(width(ids["left"]) - width(ids["right"])) <= 2


def test_reflow_clamps_a_status_pane_that_is_too_tall(layout: Layout) -> None:
    ids = built(layout)
    layout.client.run_checked("resize-pane", "-t", ids["status"], "-y", "20")
    layout.reflow(200, 30)

    assert int(layout.client.run_checked("display-message", "-p", "-t", ids["status"], "#{pane_height}")) <= 12


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


def spec_launch(role: str, index: int, cli: str = "claude", model: str = "sonnet", cwd: str | None = None) -> Launch:
    import json

    launch = sleeper(role, index, label=f"{cli}·{model}")
    return Launch(launch.argv, role, index, launch.label, env=launch.env, session_id=launch.session_id,
                  spec=json.dumps({"cli": cli, "model": model, "skip_permissions": False, "cwd": cwd}))


def test_capture_state_records_lineup_active_tab_and_sizes(layout: Layout) -> None:
    from tandem import commands

    layout.build(160, 40, spec_launch("planner", 1), spec_launch("implementer", 1, "agy", "gemini-3"),
                 sleeper("status", 0, kind="own"))
    layout.add_agent(spec_launch("planner", 2, model="opus", cwd="/tmp"))
    layout.select("planner", 1)
    layout.client.run_checked("resize-pane", "-t", visible_pane(layout.panes(), "planner").id, "-x", "40")  # type: ignore[union-attr]
    snap = commands.capture_state(layout)
    assert [(a.cli, a.role, a.model, a.index, a.cwd) for a in snap.agents] == [
        ("claude", "planner", "sonnet", 1, None), ("claude", "planner", "opus", 2, "/tmp"),
        ("agy", "implementer", "gemini-3", 1, None)]
    assert snap.active == {"planner": 1, "implementer": 1}
    assert snap.columns_percent is not None and 20 <= snap.columns_percent <= 30
    assert snap.status_rows is not None and snap.status_rows >= 3


def test_apply_sizes_restores_proportions(layout: Layout) -> None:
    ids = built(layout)
    layout.apply_sizes(30, 8)
    percent, rows = layout.sizes()
    assert percent is not None and 28 <= percent <= 32 and rows == 8
    assert ids["left"]


def test_save_exit_writes_state_then_ends_session(layout: Layout, tmp_path: Path) -> None:
    from tandem import commands, state

    layout.build(160, 40, spec_launch("planner", 1), spec_launch("implementer", 1), sleeper("status", 0, kind="own"))
    commands.save_exit(layout, tmp_path)
    loaded = state.load(state.state_path(tmp_path))
    assert loaded is not None and len(loaded.agents) == 2
    assert not layout.exists()


def test_save_exit_keeps_session_when_there_is_nothing_to_save(layout: Layout, tmp_path: Path) -> None:
    from tandem import commands, state

    built(layout)
    commands.save_exit(layout, tmp_path)
    assert layout.exists() and not state.state_path(tmp_path).exists()


def test_respawn_own_restarts_only_tandem_panes(layout: Layout) -> None:
    ids = built(layout)
    before = layout.client.run_checked("display-message", "-p", "-t", ids["status"], "#{pane_pid}").strip()
    layout.respawn_own({"status": Launch(["sleep", "61"], "status", 0, "status", kind="own")})
    import time
    time.sleep(0.3)
    assert layout.client.run_checked("display-message", "-p", "-t", ids["status"], "#{pane_pid}").strip() != before
    assert layout.client.run_checked("display-message", "-p", "-t", ids["left"], "#{pane_current_command}").strip() == "sleep"


def test_second_status_row_runs_the_quota_line(layout: Layout) -> None:
    built(layout)
    assert layout.client.run_checked("show-options", "-g", "status").split()[-1] == "2"
    assert "quota-line" in layout.client.run_checked("show-options", "-g", "status-format[1]")


def test_two_workspaces_run_side_by_side_without_sharing_a_session(tmp_path: Path) -> None:
    from tandem import commands
    from tandem.config import Config
    from tandem.tmux.client import socket_for

    a, b = tmp_path / "a", tmp_path / "b"
    a.mkdir()
    b.mkdir()
    config = Config.from_env({"XDG_STATE_HOME": str(tmp_path / "state")})
    la, lb = commands.make_layout(a, config), commands.make_layout(b, config)
    try:
        assert not la.exists() and not lb.exists()
        la.build(160, 40, sleeper("planner", 1), sleeper("implementer", 1), sleeper("status", 0, kind="own"))
        assert la.exists() and not lb.exists()
        lb.build(160, 40, sleeper("planner", 1), sleeper("implementer", 1), sleeper("status", 0, kind="own"))
        assert la.exists() and lb.exists()
        lb.client.run_checked("kill-session", "-t", SESSION)
        assert la.exists() and not lb.exists()
    finally:
        for ws in (a, b):
            subprocess.run(["tmux", "-L", socket_for(ws), "kill-server"], capture_output=True)


def test_own_panes_are_pinned_to_their_workspace(tmp_path: Path) -> None:
    from tandem import commands

    assert commands.status_launch(workspace=tmp_path).env == {"TANDEM_WORKSPACE": str(tmp_path)}
    assert commands.placeholder_launch("planner", workspace=tmp_path).env["TANDEM_WORKSPACE"] == str(tmp_path)


def test_status_pane_border_has_a_title_that_is_not_the_hostname(layout: Layout) -> None:
    ids = built(layout)
    fmt = layout.client.run_checked("show-options", "-p", "-t", ids["status"], "pane-border-format")
    assert "STATUS" in fmt and "pane_title" not in fmt


def test_registered_session_records_the_model_not_the_label(layout: Layout, tmp_path: Path) -> None:
    import json

    launch = Launch(["sleep", "60"], "planner", 1, "claude·opus", session_id="planner-1",
                    spec=json.dumps({"cli": "claude", "model": "opus", "skip_permissions": False, "cwd": None}))
    layout.build(160, 40, launch, sleeper("implementer", 1), sleeper("status", 0, kind="own"))
    assert {s.id: s.model for s in layout.coord.sessions()}["planner-1"] == "opus"  # type: ignore[union-attr]


CONV = "46f2fc12-7f1f-4ca2-b714-06cbbfd669e9"
LIVE = "0b9a3c1e-1111-4222-8333-444455556666"


def conv_launch(role: str, index: int, cli: str, conversation: str | None) -> Launch:
    import json

    launch = sleeper(role, index, label=f"{cli}·m")
    return Launch(launch.argv, role, index, launch.label, env=launch.env, session_id=launch.session_id,
                  spec=json.dumps({"cli": cli, "model": "m", "skip_permissions": False, "cwd": None,
                                   "conversation": conversation}))


def test_capture_state_saves_each_agents_current_conversation(layout: Layout, tmp_path: Path) -> None:
    from tandem import commands, conversations

    coord = tmp_path / "coord"
    layout.build(160, 40, conv_launch("planner", 1, "claude", CONV), conv_launch("implementer", 1, "agy", None),
                 sleeper("status", 0, kind="own"))
    layout.add_agent(conv_launch("planner", 2, "claude", CONV))
    conversations.record(coord, "planner-2", LIVE)  # the user ran /clear: the statusline now reports a new chat
    log = conversations.agy_log_path(coord, "implementer-1")
    log.write_text(f"Created conversation {CONV}\n")
    snap = commands.capture_state(layout, clean=False)
    assert [(a.role, a.index, a.conversation) for a in snap.agents] == [
        ("planner", 1, CONV), ("planner", 2, LIVE), ("implementer", 1, CONV)]
    assert snap.clean is False


def test_autosave_writes_unclean_state_and_stops_once_closing(layout: Layout, tmp_path: Path) -> None:
    from tandem import commands, state

    layout.build(160, 40, conv_launch("planner", 1, "claude", CONV), sleeper("implementer", 0, kind="own"),
                 sleeper("status", 0, kind="own"))
    ws = tmp_path / "ws"
    assert commands.autosave(layout, ws)
    saved = state.load(state.state_path(ws))
    assert saved is not None and not saved.clean and saved.agents[0].conversation == CONV
    state.state_path(ws).unlink()
    layout.client.run_checked("set-option", "-t", SESSION, commands.CLOSING, "1")
    assert not commands.autosave(layout, ws) and not state.state_path(ws).exists()


def test_discard_exit_removes_saved_state_and_ends_session(layout: Layout, tmp_path: Path) -> None:
    from tandem import commands, state

    layout.build(160, 40, conv_launch("planner", 1, "claude", CONV), sleeper("implementer", 0, kind="own"),
                 sleeper("status", 0, kind="own"))
    ws = tmp_path / "ws"
    assert commands.autosave(layout, ws)
    commands.discard_exit(layout, ws)
    assert not state.state_path(ws).exists() and not layout.exists()


def test_save_exit_saves_clean_state(layout: Layout, tmp_path: Path) -> None:
    from tandem import commands, state

    layout.build(160, 40, conv_launch("planner", 1, "claude", CONV), sleeper("implementer", 0, kind="own"),
                 sleeper("status", 0, kind="own"))
    ws = tmp_path / "ws"
    commands.save_exit(layout, ws)
    saved = state.load(state.state_path(ws))
    assert saved is not None and saved.clean and not layout.exists()


def test_restart_with_launch_retags_the_pane(layout: Layout) -> None:
    import json
    import time

    built(layout)
    new = layout.add_agent(Launch(["sh", "-c", "exit 1"], "planner", 2, "boom", session_id="planner-2"))
    for _ in range(40):
        if any(p.dead for p in layout.panes()):
            break
        time.sleep(0.1)
    relaunch = conv_launch("planner", 2, "claude", CONV)
    layout.restart(new, coord_dir="/tmp/coord", launch=relaunch)
    pane = next(p for p in layout.panes() if p.id == new)
    assert not pane.dead and json.loads(pane.spec)["conversation"] == CONV


def test_conf_text_unsets_tmux_pane() -> None:
    text = conf_text(50000)
    assert "set-environment -g -u TMUX_PANE" in text


def test_mouse_bindings_applied_after_apply_chrome(layout: Layout) -> None:
    built(layout)
    out = subprocess.run(
        ["tmux", "-L", str(layout.client.socket), "list-keys", "-T", "root"],
        capture_output=True, text=True, check=True,
    ).stdout
    assert "MouseDown1Status" in out
    assert "MouseDown3Pane" in out


def test_unsafe_mouse_defaults_unbound_after_apply_chrome(layout: Layout) -> None:
    import re
    built(layout)
    out = subprocess.run(
        ["tmux", "-L", str(layout.client.socket), "list-keys", "-T", "root"],
        capture_output=True, text=True, check=True,
    ).stdout
    # Check exact key token: surrounded by whitespace so "MouseDown3Status" does not
    # false-match inside "M-MouseDown3Status" (a different, unrelated key).
    for unsafe in ("MouseDown3Status", "MouseDown3StatusLeft", "M-MouseDown3Pane"):
        pattern = rf"(?<!\S){re.escape(unsafe)}(?!\S)"
        matches = re.findall(pattern, out)
        assert not matches, f"{unsafe} should have been unbound; still present in list-keys"


def test_safe_tmux_mouse_defaults_still_present(layout: Layout) -> None:
    built(layout)
    out = subprocess.run(
        ["tmux", "-L", str(layout.client.socket), "list-keys", "-T", "root"],
        capture_output=True, text=True, check=True,
    ).stdout
    for safe in ("MouseDrag1Border", "MouseDown1Pane", "WheelUpPane"):
        assert safe in out, f"{safe} default should still be bound"


def test_border_click_binding_added_after_apply_chrome(layout: Layout) -> None:
    built(layout)
    out = subprocess.run(
        ["tmux", "-L", str(layout.client.socket), "list-keys", "-T", "root"],
        capture_output=True, text=True, check=True,
    ).stdout
    assert "MouseDown1Border" in out


def test_drag_resize_still_bound_after_border_click_binding(layout: Layout) -> None:
    built(layout)
    out = subprocess.run(
        ["tmux", "-L", str(layout.client.socket), "list-keys", "-T", "root"],
        capture_output=True, text=True, check=True,
    ).stdout
    assert "MouseDrag1Border" in out


def test_click_tab_select_switches_visible_agent(layout: Layout) -> None:
    from tandem import commands
    from tandem.config import Config
    from tandem.tmux.tabs import visible_pane

    ids = built(layout)
    layout.add_agent(sleeper("planner", 2))
    current = visible_pane(layout.panes(), "planner")
    assert current is not None and current.index == 2

    commands.click_tab(layout, Config.from_env(env={}), current.id, ids["left"])

    shown = visible_pane(layout.panes(), "planner")
    assert shown is not None and shown.index == 1


def test_bound_border_click_command_line_is_accepted_by_the_cli(layout: Layout, tmp_path: Path) -> None:
    import re
    import sys

    from tandem.tmux import keys

    built(layout)
    launcher = keys.launcher_for(sys.executable, str(tmp_path))
    bound = next(c for c in keys.mouse_commands(sys.executable, str(tmp_path))
                 if c[:3] == ["bind-key", "-n", "MouseDown1Border"])[3]
    template = re.search(r"click-tab (.*?)\"$", bound)
    assert template is not None
    pane = layout.panes()[0].id
    expanded = layout.client.run_checked("display-message", "-p", "-t", pane, template.group(1)).strip()

    result = subprocess.run(f"{launcher} click-tab {expanded}", shell=True, capture_output=True, text=True)

    assert result.returncode != 2 and "usage:" not in result.stderr, result.stderr


def test_restart_fallback_uses_active_pane_when_env_pane_empty(layout: Layout) -> None:
    import json
    import time
    from tandem.config import Config

    built(layout)
    new = layout.add_agent(Launch(["sh", "-c", "exit 1"], "planner", 2, "boom", session_id="planner-2"))
    for _ in range(40):
        if any(p.dead for p in layout.panes()):
            break
        time.sleep(0.1)
    layout.client.run_checked("select-pane", "-t", new)
    cfg = Config.from_env(env={})
    assert not cfg.tmux_pane
    pane_id = cfg.tmux_pane or layout.client.run_checked("display-message", "-p", "#{pane_id}").strip()
    assert pane_id == new



def test_click_tab_menu_renders_on_an_attached_client(layout: Layout) -> None:
    import fcntl
    import pty
    import select
    import struct
    import termios
    import time

    from tandem import commands
    from tandem.config import Config
    from tandem.tmux.tabs import visible_pane

    built(layout)
    layout.add_agent(sleeper("planner", 2))
    current = visible_pane(layout.panes(), "planner")
    assert current is not None

    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 160, 0, 0))
    proc = subprocess.Popen(
        ["tmux", "-L", str(layout.client.socket), "attach", "-t", SESSION],
        stdin=slave, stdout=slave, stderr=slave, env={**os.environ, "TERM": "xterm-256color"},
    )
    os.close(slave)
    try:
        deadline = time.time() + 5
        while time.time() < deadline:
            listed = subprocess.run(["tmux", "-L", str(layout.client.socket), "list-clients"],
                                    capture_output=True, text=True)
            if listed.stdout.strip():
                break
            time.sleep(0.1)
        assert listed.stdout.strip(), "tmux client did not attach"
        commands.click_tab(layout, Config.from_env(env={}), current.id)
        seen = b""
        end = time.time() + 1.5
        while time.time() < end:
            ready, _, _ = select.select([master], [], [], 0.2)
            if ready:
                seen += os.read(master, 65536)
    finally:
        proc.terminate()
        proc.wait(timeout=5)
        os.close(master)

    import re

    text = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b[()][A-Z0-9]|\x1b[=>]", "", seen.decode("utf-8", "replace"))
    menu_start = text.rfind("tandem")
    assert menu_start != -1, text[-800:]
    menu = text[menu_start:]
    assert "▶ 2" in menu and "1" in menu and "New agent" in menu, menu[-800:]


def test_click_tab_menu_item_commands_are_valid_tmux_commands(
    layout: Layout, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    import time

    from tandem import commands
    from tandem.config import Config
    from tandem.tmux.tabs import visible_pane

    ids = built(layout)
    layout.add_agent(sleeper("planner", 2))
    current = visible_pane(layout.panes(), "planner")
    assert current is not None

    record = tmp_path / "item args.txt"
    monkeypatch.setattr(keys, "launcher_for", lambda python, workspace: f"sh -c 'echo \"$@\" >> \"{record}\"' --")
    captured: list[tuple[str, ...]] = []
    real_run_checked = layout.client.run_checked

    def spy(*args: str) -> str:
        if args[0] != "display-menu":
            return real_run_checked(*args)
        captured.append(args)
        return ""

    monkeypatch.setattr(layout.client, "run_checked", spy)

    commands.click_tab(layout, Config.from_env(env={}), current.id)

    menu = next(a for a in captured if a[0] == "display-menu")
    item_commands = [c for c in menu[menu.index("#{pane_top}") + 1:][2::3] if c.startswith("run-shell")]
    assert len(item_commands) == 2
    for cmd in item_commands:
        done = subprocess.run(["tmux", "-L", str(layout.client.socket), "if-shell", "-F", "1", cmd],
                              capture_output=True, text=True)
        assert done.returncode == 0, done.stderr
    deadline = time.time() + 5
    while time.time() < deadline and (not record.exists() or len(record.read_text().splitlines()) < 2):
        time.sleep(0.1)
    assert sorted(record.read_text().splitlines()) == sorted(
        f"click-tab {pid} --select {pid}" for pid in (ids["left"], current.id))
