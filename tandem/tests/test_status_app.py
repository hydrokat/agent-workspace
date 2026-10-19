import asyncio
import json
import time
from collections.abc import Awaitable, Callable
from pathlib import Path

from textual.pilot import Pilot
from textual.widgets import OptionList, Static, TabbedContent

from tandem.coordination.store import CoordStore
from tandem.status.app import StatusApp


def write_session(root: Path, sid: str, **kw: object) -> None:
    (root / "sessions").mkdir(parents=True, exist_ok=True)
    rec = {"id": sid, "cli": "claude", "role": "implementer", "model": "sonnet", "pane": "%7", "cwd": "/w",
           "branch": "feat", "worktree": "/w", "task": "p-001 t-003", "heartbeat": time.time()}
    rec.update(kw)
    (root / "sessions" / f"{sid}.json").write_text(json.dumps(rec))


def workspace(tmp_path: Path) -> Path:
    ws = tmp_path / "ws"
    phase = ws / "specs" / "p-001-thing"
    phase.mkdir(parents=True)
    (phase / "impl.md").write_text("- [x] t-001: a\n- [ ] t-002: b\n")
    (phase / "t-001.md").write_text("## Status\n- [x] Done\n")
    return ws


def run_app(app: StatusApp, script: Callable[[Pilot[None]], Awaitable[None]], size: tuple[int, int] = (120, 40)) -> None:
    async def go() -> None:
        async with app.run_test(size=size) as pilot:
            await app.workers.wait_for_complete()
            await pilot.pause()
            await script(pilot)

    asyncio.run(go())


def text(app: StatusApp, selector: str) -> str:
    return str(app.query_one(selector, Static).render())


class FakeTmux:
    def __init__(self) -> None:
        self.calls: list[tuple[str, ...]] = []

    def run_checked(self, *args: str) -> str:
        self.calls.append(args)
        return ""


def test_shows_summary_phases_and_sessions(tmp_path: Path) -> None:
    ws = workspace(tmp_path)
    coord = tmp_path / "coord"
    write_session(coord, "implementer-1")
    app = StatusApp(ws, CoordStore(coord))

    async def script(p: Pilot[None]) -> None:
        assert "1/2 tasks" in text(app, "#summary") and "1 agent" in text(app, "#summary")
        assert "p-001-thing" in text(app, "#phases") and "t-002" in text(app, "#phases")
        assert app.query_one("#sessions", OptionList).option_count == 1

    run_app(app, script)


def test_empty_workspace_shows_helpful_states(tmp_path: Path) -> None:
    ws = tmp_path / "ws"
    ws.mkdir()
    app = StatusApp(ws, CoordStore(tmp_path / "none"))

    async def script(p: Pilot[None]) -> None:
        assert "No specs yet" in text(app, "#phases")
        assert "Coordination folder not found" in text(app, "#inprogress")
        assert "workspace · not a git repository" in text(app, "#repos")

    run_app(app, script)


def test_malformed_spec_does_not_break_the_rest(tmp_path: Path) -> None:
    ws = workspace(tmp_path)
    (ws / "specs" / "p-009-bad").mkdir()
    app = StatusApp(ws, CoordStore(tmp_path / "c"))

    async def script(p: Pilot[None]) -> None:
        assert "WARNING" in text(app, "#phases") and "p-001-thing" in text(app, "#phases")

    run_app(app, script)


def test_refresh_key_picks_up_new_sessions(tmp_path: Path) -> None:
    ws = workspace(tmp_path)
    coord = tmp_path / "coord"
    app = StatusApp(ws, CoordStore(coord))

    async def script(p: Pilot[None]) -> None:
        assert app.query_one("#sessions", OptionList).option_count == 0
        write_session(coord, "planner-1", role="planner")
        await p.press("r")
        await app.workers.wait_for_complete()
        await p.pause()
        assert app.query_one("#sessions", OptionList).option_count == 1

    run_app(app, script)


def test_goto_focuses_the_agents_pane(tmp_path: Path) -> None:
    ws = workspace(tmp_path)
    coord = tmp_path / "coord"
    write_session(coord, "implementer-1", pane="%7")
    tmux = FakeTmux()
    app = StatusApp(ws, CoordStore(coord), tmux=tmux)  # type: ignore[arg-type]

    async def script(p: Pilot[None]) -> None:
        app.query_one("#sessions", OptionList).focus()
        await p.press("down")
        await p.press("g")
        assert tmux.calls == [("select-pane", "-t", "%7")]

    run_app(app, script)


def test_goto_ignores_invalid_pane_ids(tmp_path: Path) -> None:
    ws = workspace(tmp_path)
    coord = tmp_path / "coord"
    write_session(coord, "implementer-1", pane="%7; kill-server")
    tmux = FakeTmux()
    app = StatusApp(ws, CoordStore(coord), tmux=tmux)  # type: ignore[arg-type]

    async def script(p: Pilot[None]) -> None:
        app.query_one("#sessions", OptionList).focus()
        await p.press("down")
        await p.press("g")
        assert tmux.calls == []

    run_app(app, script)


def test_compact_mode_shows_only_summary(tmp_path: Path) -> None:
    app = StatusApp(workspace(tmp_path), CoordStore(tmp_path / "c"))

    async def script(p: Pilot[None]) -> None:
        assert app.query_one("#body").display is False
        assert "tasks" in text(app, "#summary")

    run_app(app, script, size=(100, 6))


def test_escape_sequences_in_spec_titles_never_reach_the_screen(tmp_path: Path) -> None:
    ws = workspace(tmp_path)
    (ws / "specs" / "p-001-thing" / "impl.md").write_text("- [ ] t-001: \x1b[2Jevil [bold]x[/bold]\n")
    app = StatusApp(ws, CoordStore(tmp_path / "c"))

    async def script(p: Pilot[None]) -> None:
        assert "\x1b" not in text(app, "#phases")

    run_app(app, script)


def test_queries_tab_exists_and_f6_switches_to_it(tmp_path: Path) -> None:
    app = StatusApp(workspace(tmp_path), CoordStore(tmp_path / "c"))

    async def script(p: Pilot[None]) -> None:
        await p.press("f6")
        assert app.query_one(TabbedContent).active == "queries-pane"

    run_app(app, script)


def test_markup_in_untrusted_titles_is_shown_literally(tmp_path: Path) -> None:
    ws = workspace(tmp_path)
    (ws / "specs" / "p-001-thing" / "impl.md").write_text("- [ ] t-001: [bold]loud[/bold] [red]x\n")
    app = StatusApp(ws, CoordStore(tmp_path / "c"))

    async def script(p: Pilot[None]) -> None:
        assert "[bold]loud[/bold]" in text(app, "#phases")

    run_app(app, script)


def test_repositories_section_lists_codebase_branches_and_worktrees(tmp_path: Path) -> None:
    import subprocess

    git = ["git", "-c", "user.email=a@b.c", "-c", "user.name=t"]
    (tmp_path / "pkg").mkdir()
    for args in (["init", "-q", "-b", "trunk"], ["commit", "-q", "--allow-empty", "-m", "i"], ["branch", "feat"],
                 ["worktree", "add", "-q", str(tmp_path / "pkg-wt"), "feat"]):
        subprocess.run([*git, *args], cwd=tmp_path / "pkg", check=True)
    ws = tmp_path / "ws"
    (ws / "codebase-symlinks").mkdir(parents=True)
    (ws / "codebase-symlinks" / "pkg").symlink_to(tmp_path / "pkg")
    app = StatusApp(ws, CoordStore(tmp_path / "none"))

    async def script(p: Pilot[None]) -> None:
        await p.pause(0.5)
        body = text(app, "#repos")
        assert "pkg · 2 branches · 1 worktree" in body
        assert "* trunk" in body and "feat" in body and "→ ../pkg-wt" in body

    run_app(app, script)


def test_status_window_autosaves_the_session_periodically(tmp_path: Path) -> None:
    calls: list[float] = []
    app = StatusApp(workspace(tmp_path), CoordStore(tmp_path / "coord"), refresh_seconds=60,
                    autosave=lambda: calls.append(time.time()), autosave_seconds=0.05)

    async def script(pilot: Pilot[None]) -> None:
        await asyncio.sleep(0.3)
        await app.workers.wait_for_complete()

    run_app(app, script)
    assert len(calls) >= 2
