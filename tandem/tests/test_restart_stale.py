import os
import shutil
import subprocess
import uuid
from collections.abc import Iterator
from pathlib import Path

import pytest

from tandem import commands, updater, versions
from tandem.adapters.base import AgentSpec
from tandem.config import Config
from tandem.coordination.store import CoordStore
from tandem.tmux.client import TmuxClient, TmuxError
from tandem.tmux.layout import Launch, Layout
from tandem.tmux.tabs import PaneInfo, parse_panes

CONV = "46f2fc12-7f1f-4ca2-b714-06cbbfd669e9"


def pane(pid: str, role: str, index: int, cli: str = "claude", launched: str = "2.1.305", kind: str = "agent",
         session: str = "") -> PaneInfo:
    spec = f'{{"cli": "{cli}", "model": "opus", "skip_permissions": false, "cwd": null}}'
    return PaneInfo(pid, "main", kind, role, index, f"{cli}·opus", False, None, session or f"{role}-{index}", spec,
                    launched)


def installed(table: dict[str, str]):  # type: ignore[no-untyped-def]
    return lambda cli: table.get(cli, "")


def test_parse_panes_exposes_the_launched_cli_version() -> None:
    line = "\t".join(["%1", "main", "agent", "planner", "1", "claude·opus", "0", "", "planner-1", "{}", "2.1.305"])
    assert parse_panes(line)[0].cli_version == "2.1.305"
    legacy = "\t".join(["%1", "main", "agent", "planner", "1", "claude·opus", "0", "", "planner-1", "{}"])
    assert parse_panes(legacy)[0].cli_version == ""


def test_installed_version_parses_the_banner_and_survives_missing_or_failing_clis() -> None:
    ran: list[list[str]] = []

    def runner(argv: list[str]) -> tuple[int, str]:
        ran.append(argv)
        return 0, "2.1.305 (Claude Code)"

    assert versions.installed_version("claude", lambda n: "/bin/claude", runner) == "2.1.305"
    assert ran == [["/bin/claude", "--version"]]
    assert versions.installed_version("claude", lambda n: None, runner) == ""
    assert versions.installed_version("claude", lambda n: "/bin/claude", lambda argv: (1, "boom 1.2")) == ""
    assert versions.installed_version("tmux", lambda n: "/bin/tmux", lambda argv: (0, "tmux 3.3a")) == "3.3a"


def test_agent_launch_records_the_installed_version(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(commands, "cli_version", lambda cli: "2.1.310")
    launch = commands.agent_launch(AgentSpec("claude", "planner", "opus", 1), str(tmp_path / "coord"), tmp_path)
    assert launch.cli_version == "2.1.310"


def test_stale_means_behind_or_unknown_and_skips_current_and_unprobeable_clis() -> None:
    panes = [pane("%1", "planner", 1, launched="2.1.305"), pane("%2", "planner", 2, launched="2.1.310"),
             pane("%3", "implementer", 1, cli="agy", launched=""), pane("%4", "implementer", 2, cli="agy", launched="1.4.0"),
             pane("%5", "status", 0, kind="own"), pane("%6", "implementer", 3, cli="codex", launched="0.1.0")]
    found = updater.stale_agents(panes, installed({"claude": "2.1.310", "agy": "1.4.0"}), lambda p: True, {})
    assert [(s.pane_id, s.launched, s.installed) for s in found] == [("%1", "2.1.305", "2.1.310"), ("%3", "", "1.4.0")]
    assert found[0].role == "planner" and found[0].cli == "claude" and found[0].model == "opus"


def test_flags_agents_that_cannot_resume_or_are_busy() -> None:
    panes = [pane("%1", "planner", 1), pane("%2", "planner", 2), pane("%3", "implementer", 1)]
    found = updater.stale_agents(panes, installed({"claude": "2.1.310"}), lambda p: p.id != "%2",
                                 {"implementer-1": "p-004 t-002"})
    flags = {s.pane_id: (s.resumable, s.busy, s.claim) for s in found}
    assert flags == {"%1": (True, False, ""), "%2": (False, False, ""), "%3": (True, True, "p-004 t-002")}
    assert updater.default_selection(found) == {"%1"}


def test_nothing_stale_when_all_are_current() -> None:
    assert updater.stale_agents([pane("%1", "planner", 1, launched="2.1.310")], installed({"claude": "2.1.310"}),
                                lambda p: True, {}) == []


def make_stale() -> list[updater.StaleAgent]:
    panes = [pane("%1", "planner", 1), pane("%2", "implementer", 1), pane("%3", "implementer", 2)]
    return updater.stale_agents(panes, installed({"claude": "2.1.310"}), lambda p: p.id != "%3",
                                {"implementer-1": "p-004 t-002"})


def choose(answers: list[str]) -> list[str]:
    feed = iter(answers)

    def ask(_: str) -> str:
        try:
            return next(feed)
        except StopIteration:
            raise EOFError from None

    lines: list[str] = []
    chosen = updater.choose_restarts(make_stale(), ask, lines.append)
    assert any("still run an older CLI" in line for line in lines)
    return [s.pane_id for s in chosen]


def test_enter_restarts_the_safe_defaults_only() -> None:
    assert choose([""]) == ["%1"]


def test_all_numbers_and_none() -> None:
    assert choose(["a", ""]) == ["%1", "%2", "%3"]
    assert choose(["2 3", ""]) == ["%1", "%2", "%3"]
    assert choose(["1", ""]) == []
    assert choose(["n"]) == []
    assert choose(["a", "n"]) == []


def test_ignores_junk_and_treats_end_of_input_as_none() -> None:
    assert choose(["9 x -1", ""]) == ["%1"]
    assert choose([]) == []


def test_the_update_plan_mentions_the_ai_cli_step(tmp_path: Path) -> None:
    text = "\n".join(updater.plan(tmp_path, "o/r"))
    assert "claude update" in text and "agents keep running" in text


@pytest.fixture
def layout(tmp_path: Path) -> Iterator[Layout]:
    socket = f"tandem-test-{os.getpid()}-{uuid.uuid4().hex[:8]}"
    lay = Layout(TmuxClient(socket=socket), coord=CoordStore(tmp_path / "coord"), workspace=str(tmp_path))
    conf = lay.write_conf(tmp_path / "tandem.conf")
    lay.client = TmuxClient(socket=socket, conf=conf)
    yield lay
    subprocess.run(["tmux", "-L", socket, "kill-server"], capture_output=True)


pytestmark_tmux = pytest.mark.skipif(shutil.which("tmux") is None, reason="tmux not installed")


def sleeper(role: str, index: int, kind: str = "agent", seconds: int = 60, version: str = "", spec: str = "") -> Launch:
    return Launch(["sleep", str(seconds)], role, index, f"fake·{role}{index}", kind=kind, cli_version=version, spec=spec,
                  env={"TANDEM_SESSION_ID": f"{role}-{index}"}, session_id=f"{role}-{index}" if kind == "agent" else "")


def build(layout: Layout, version: str = "2.1.305") -> dict[str, str]:
    return layout.build(160, 40, sleeper("planner", 1, version=version), sleeper("implementer", 1, version=version),
                        sleeper("status", 0, kind="own"))


@pytestmark_tmux
def test_launch_tags_the_pane_with_its_cli_version(layout: Layout) -> None:
    ids = build(layout)
    assert layout.client.run_checked("show-options", "-pqv", "-t", ids["left"], "@tandem_cli_version").strip() == "2.1.305"
    assert {p.id: p.cli_version for p in layout.panes()}[ids["left"]] == "2.1.305"


@pytestmark_tmux
def test_restart_live_replaces_a_running_agent_and_retags_it(layout: Layout) -> None:
    ids = build(layout)
    layout.restart_live(ids["left"], sleeper("planner", 1, seconds=61, version="2.1.310"))
    after = {p.id: p for p in layout.panes()}
    assert after[ids["left"]].cli_version == "2.1.310" and not after[ids["left"]].dead
    argv = layout.client.run_checked("show-options", "-pqv", "-t", ids["left"], "@tandem_argv")
    assert "61" in argv
    assert after[ids["right"]].cli_version == "2.1.305"


@pytestmark_tmux
def test_restart_live_refuses_panes_that_are_not_agents(layout: Layout) -> None:
    ids = build(layout)
    with pytest.raises(TmuxError):
        layout.restart_live(ids["status"], sleeper("planner", 1))
    with pytest.raises(TmuxError):
        layout.restart_live("%9999", sleeper("planner", 1))


@pytestmark_tmux
def test_restart_stale_reopens_conversations_and_isolates_failures(layout: Layout, tmp_path: Path) -> None:
    claude_dir = tmp_path / "claude"
    (claude_dir / "projects" / "p").mkdir(parents=True)
    (claude_dir / "projects" / "p" / f"{CONV}.jsonl").write_text("{}\n")
    config = Config.from_env({"CLAUDE_CONFIG_DIR": str(claude_dir), "TANDEM_COORD_DIR": str(tmp_path / "coord")})
    spec = (f'{{"cli": "claude", "model": "opus", "skip_permissions": false, "cwd": null, "conversation": "{CONV}"}}')
    layout.build(160, 40, sleeper("planner", 1, version="2.1.305", spec=spec),
                 sleeper("implementer", 1, version="2.1.305", spec=spec), sleeper("status", 0, kind="own"))
    panes = layout.panes()
    targets = [updater.StaleAgent(p.id, p.role, p.index, "claude", "opus", "2.1.305", "2.1.310", True, False, "")
               for p in panes if p.kind == "agent"]
    seen: list[list[str]] = []
    real = layout.restart_live

    def tracking(pane_id: str, launch: Launch) -> None:
        seen.append(launch.argv)
        if launch.role == "implementer":
            raise TmuxError("respawn failed")
        real(pane_id, Launch(["sleep", "62"], launch.role, launch.index, launch.label, cli_version="2.1.310",
                             env=launch.env, session_id=launch.session_id, spec=launch.spec))

    layout.restart_live = tracking  # type: ignore[method-assign]
    messages = commands.restart_stale(layout, tmp_path, config, targets)
    assert any(argv[-2:] == ["--resume", CONV] for argv in seen), seen
    assert len(seen) == 2 and any("respawn failed" in m for m in messages)
    assert {p.role: p.dead for p in layout.panes() if p.kind == "agent"} == {"planner": False, "implementer": False}


def test_offer_restarts_lists_but_does_not_restart_without_a_terminal(layout: Layout, tmp_path: Path,
                                                                     monkeypatch: pytest.MonkeyPatch) -> None:
    stale = [updater.StaleAgent("%1", "planner", 1, "claude", "opus", "2.1.305", "2.1.310", True, False, "")]
    monkeypatch.setattr(commands, "find_stale", lambda lay, ws, cfg: stale)
    restarted: list[list[updater.StaleAgent]] = []

    def record(lay: Layout, ws: Path, cfg: Config, chosen: list[updater.StaleAgent]) -> list[str]:
        restarted.append(chosen)
        return []

    monkeypatch.setattr(commands, "restart_stale", record)
    lines: list[str] = []
    commands.offer_restarts(layout, tmp_path, Config.from_env({}), interactive=False, out=lines.append)
    assert restarted == [] and any("planner 1" in line for line in lines)
    commands.offer_restarts(layout, tmp_path, Config.from_env({}), interactive=True, ask=lambda _: "", out=lines.append)
    assert [s.pane_id for s in restarted[0]] == ["%1"]


def test_offer_restarts_is_silent_when_nothing_is_stale(layout: Layout, tmp_path: Path,
                                                        monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(commands, "find_stale", lambda lay, ws, cfg: [])
    lines: list[str] = []
    commands.offer_restarts(layout, tmp_path, Config.from_env({}), interactive=True, out=lines.append)
    assert lines == []
