import asyncio
import json
import time
from collections.abc import Awaitable, Callable
from pathlib import Path

from textual.pilot import Pilot
from textual.widgets import Footer, OptionList, Static, TabbedContent

from tandem.config import Config
from tandem.coordination.store import CoordStore
from tandem.status.app import StatusApp
from tandem.update_check import Runner


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
        await p.press("alt+r")
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
        await p.press("alt+g")
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
        await p.press("alt+g")
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


def test_queries_tab_exists_and_alt_q_switches_to_it(tmp_path: Path) -> None:
    app = StatusApp(workspace(tmp_path), CoordStore(tmp_path / "c"))

    async def script(p: Pilot[None]) -> None:
        await p.press("alt+q")
        assert app.query_one(TabbedContent).active == "queries-pane"
        await p.pause()
        assert app.focused == app.query_one("#q-list")

        await p.press("alt+s")
        assert app.query_one(TabbedContent).active == "status-pane"
        await p.pause()
        assert app.focused == app.query_one("#sessions")

        import sys
        if sys.platform == "darwin":
            await p.press("œ")
            assert app.query_one(TabbedContent).active == "queries-pane"
            await p.pause()
            assert app.focused == app.query_one("#q-list")

            await p.press("ß")
            assert app.query_one(TabbedContent).active == "status-pane"
            await p.pause()
            assert app.focused == app.query_one("#sessions")

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


# Update banner


def test_update_banner_shown_for_unseen_notice(tmp_path: Path) -> None:
    from tandem.notice import write_notice
    from tandem.status.app import UpdateBanner

    ws = workspace(tmp_path)
    write_notice(ws, "1.2.0", "1.3.0", at=1700000000)
    app = StatusApp(ws, CoordStore(tmp_path / "coord"))

    async def script(p: Pilot[None]) -> None:
        banners = app.query(UpdateBanner)
        assert len(banners) == 1
        assert "1.3.0" in str(banners.first().query_one("#banner-text", Static).render())

    run_app(app, script)


def test_update_banner_hidden_when_no_notice(tmp_path: Path) -> None:
    from tandem.status.app import UpdateBanner

    app = StatusApp(workspace(tmp_path), CoordStore(tmp_path / "coord"))

    async def script(p: Pilot[None]) -> None:
        assert len(app.query(UpdateBanner)) == 0

    run_app(app, script)


def test_update_banner_hidden_when_notice_already_seen(tmp_path: Path) -> None:
    from tandem.notice import mark_seen, write_notice
    from tandem.status.app import UpdateBanner

    ws = workspace(tmp_path)
    write_notice(ws, "1.2.0", "1.3.0", at=1700000000)
    mark_seen(ws)
    app = StatusApp(ws, CoordStore(tmp_path / "coord"))

    async def script(p: Pilot[None]) -> None:
        assert len(app.query(UpdateBanner)) == 0

    run_app(app, script)


def test_update_banner_hidden_in_compact_mode(tmp_path: Path) -> None:
    from tandem.notice import write_notice
    from tandem.status.app import UpdateBanner

    ws = workspace(tmp_path)
    write_notice(ws, "1.2.0", "1.3.0", at=1700000000)
    app = StatusApp(ws, CoordStore(tmp_path / "coord"))

    async def script(p: Pilot[None]) -> None:
        banners = app.query(UpdateBanner)
        assert len(banners) == 1
        assert banners.first().display is False

    run_app(app, script, size=(100, 6))


def test_update_banner_dismiss_marks_seen_and_removes_banner(tmp_path: Path) -> None:
    from tandem.notice import read_notice, write_notice
    from tandem.status.app import UpdateBanner

    ws = workspace(tmp_path)
    write_notice(ws, "1.2.0", "1.3.0", at=1700000000)
    app = StatusApp(ws, CoordStore(tmp_path / "coord"))

    async def script(p: Pilot[None]) -> None:
        assert len(app.query(UpdateBanner)) == 1
        await p.click("#banner-dismiss")
        await p.pause()
        assert len(app.query(UpdateBanner)) == 0
        n = read_notice(ws)
        assert n is not None and n.seen is True

    run_app(app, script)


def test_update_banner_click_text_triggers_tmux_popup(tmp_path: Path) -> None:
    from tandem.notice import write_notice

    ws = workspace(tmp_path)
    write_notice(ws, "1.2.0", "1.3.0", at=1700000000)
    tmux = FakeTmux()
    app = StatusApp(ws, CoordStore(tmp_path / "coord"), tmux=tmux)  # type: ignore[arg-type]

    async def script(p: Pilot[None]) -> None:
        await p.click("#banner-text")
        await p.pause()
        assert any("display-popup" in call[0] for call in tmux.calls)

    run_app(app, script)


# Clickable Footer


def test_footer_replaces_hints_static(tmp_path: Path) -> None:
    app = StatusApp(workspace(tmp_path), CoordStore(tmp_path / "c"))

    async def script(p: Pilot[None]) -> None:
        assert len(app.query(Footer)) == 1

    run_app(app, script)


def test_footer_present_in_compact_mode(tmp_path: Path) -> None:
    app = StatusApp(workspace(tmp_path), CoordStore(tmp_path / "c"))

    async def script(p: Pilot[None]) -> None:
        footers = app.query(Footer)
        assert len(footers) == 1
        assert footers.first().display is True

    run_app(app, script, size=(100, 6))


def test_footer_key_display_is_os_aware(tmp_path: Path) -> None:
    import sys
    from textual.binding import Binding as TextualBinding
    from tandem.tmux.keys import alt_name

    app = StatusApp(workspace(tmp_path), CoordStore(tmp_path / "c"))

    async def script(p: Pilot[None]) -> None:
        expected_prefix = alt_name(sys.platform)
        tb_bindings = [b for b in app.BINDINGS if isinstance(b, TextualBinding)]
        displays = [b.key_display for b in tb_bindings if b.key_display]
        assert any(expected_prefix in d for d in displays)

    run_app(app, script)


def test_session_row_enter_goes_to_agent(tmp_path: Path) -> None:
    ws = workspace(tmp_path)
    coord = tmp_path / "coord"
    write_session(coord, "implementer-1", pane="%7")
    tmux = FakeTmux()
    app = StatusApp(ws, CoordStore(coord), tmux=tmux)  # type: ignore[arg-type]

    async def script(p: Pilot[None]) -> None:
        sessions = app.query_one("#sessions", OptionList)
        sessions.focus()
        await p.press("down")
        await p.press("enter")
        await p.pause()
        assert tmux.calls == [("select-pane", "-t", "%7")]

    run_app(app, script)


def test_session_row_click_goes_to_agent(tmp_path: Path) -> None:
    ws = workspace(tmp_path)
    coord = tmp_path / "coord"
    write_session(coord, "implementer-1", pane="%7")
    tmux = FakeTmux()
    app = StatusApp(ws, CoordStore(coord), tmux=tmux)  # type: ignore[arg-type]

    async def script(p: Pilot[None]) -> None:
        await p.click("#sessions", offset=(4, 1))
        await p.pause()
        assert tmux.calls == [("select-pane", "-t", "%7")]

    run_app(app, script)


# In-session update offer banner (t-007)


def _make_config(tmp_path: Path, ws: Path, interval: int = 600) -> Config:
    (ws / ".agent-workspace").mkdir(parents=True, exist_ok=True)
    (ws / ".agent-workspace" / "manifest.json").write_text('{"version": "1.3.0"}')
    return Config.from_env({"TANDEM_WORKSPACE": str(ws), "TANDEM_UPDATE_INTERVAL": str(interval)})


def _fake_runner(version: str | None, calls: list[list[str]] | None = None) -> Runner:
    def run(argv: list[str], cwd: Path, timeout: float) -> tuple[int, str]:
        if calls is not None:
            calls.append(argv)
        if version is None:
            return 1, ""
        return 0, version

    return run


def test_offer_banner_shown_when_newer_version_available(tmp_path: Path) -> None:
    from tandem.status.app import OfferBanner

    ws = workspace(tmp_path)
    cfg = _make_config(tmp_path, ws)
    app = StatusApp(ws, CoordStore(tmp_path / "coord"), config=cfg,
                    update_runner=_fake_runner("1.4.0"), now=lambda: 1_700_000_000.0)

    async def script(p: Pilot[None]) -> None:
        await app.workers.wait_for_complete()
        await p.pause()
        banners = app.query(OfferBanner)
        assert len(banners) == 1
        text = str(banners.first().query_one("#offer-text", Static).render())
        assert "1.3.0" in text and "1.4.0" in text

    run_app(app, script)


def test_offer_banner_not_shown_when_up_to_date(tmp_path: Path) -> None:
    from tandem.status.app import OfferBanner

    ws = workspace(tmp_path)
    cfg = _make_config(tmp_path, ws)
    app = StatusApp(ws, CoordStore(tmp_path / "coord"), config=cfg,
                    update_runner=_fake_runner("1.3.0"), now=lambda: 1_700_000_000.0)

    async def script(p: Pilot[None]) -> None:
        await app.workers.wait_for_complete()
        await p.pause()
        assert len(app.query(OfferBanner)) == 0

    run_app(app, script)


def test_offer_banner_not_shown_when_check_fails(tmp_path: Path) -> None:
    from tandem.status.app import OfferBanner

    ws = workspace(tmp_path)
    cfg = _make_config(tmp_path, ws)
    app = StatusApp(ws, CoordStore(tmp_path / "coord"), config=cfg,
                    update_runner=_fake_runner(None), now=lambda: 1_700_000_000.0)

    async def script(p: Pilot[None]) -> None:
        await app.workers.wait_for_complete()
        await p.pause()
        assert len(app.query(OfferBanner)) == 0

    run_app(app, script)


def test_offer_banner_later_hides_it(tmp_path: Path) -> None:
    from tandem.status.app import OfferBanner

    ws = workspace(tmp_path)
    cfg = _make_config(tmp_path, ws)
    app = StatusApp(ws, CoordStore(tmp_path / "coord"), config=cfg,
                    update_runner=_fake_runner("1.4.0"), now=lambda: 1_700_000_000.0)

    async def script(p: Pilot[None]) -> None:
        await app.workers.wait_for_complete()
        await p.pause()
        assert len(app.query(OfferBanner)) == 1
        await p.click("#offer-later")
        await p.pause()
        assert len(app.query(OfferBanner)) == 0

    run_app(app, script)


def test_offer_banner_skip_writes_skip_version(tmp_path: Path) -> None:
    from tandem.status.app import OfferBanner
    from tandem.update_check import read_state

    ws = workspace(tmp_path)
    cfg = _make_config(tmp_path, ws)
    app = StatusApp(ws, CoordStore(tmp_path / "coord"), config=cfg,
                    update_runner=_fake_runner("1.4.0"), now=lambda: 1_700_000_000.0)

    async def script(p: Pilot[None]) -> None:
        await app.workers.wait_for_complete()
        await p.pause()
        assert len(app.query(OfferBanner)) == 1
        await p.click("#offer-skip")
        await p.pause()
        assert len(app.query(OfferBanner)) == 0
        state = read_state(ws)
        assert state.skip == "1.4.0"

    run_app(app, script)


def test_offer_banner_update_now_triggers_popup(tmp_path: Path) -> None:
    from tandem.status.app import OfferBanner

    ws = workspace(tmp_path)
    cfg = _make_config(tmp_path, ws)
    tmux = FakeTmux()
    app = StatusApp(ws, CoordStore(tmp_path / "coord"), tmux=tmux,  # type: ignore[arg-type]
                    config=cfg, update_runner=_fake_runner("1.4.0"), now=lambda: 1_700_000_000.0)

    async def script(p: Pilot[None]) -> None:
        await app.workers.wait_for_complete()
        await p.pause()
        assert len(app.query(OfferBanner)) == 1
        await p.click("#offer-update")
        await p.pause()
        assert any("display-popup" in c[0] and "--yes" in c[-1] for c in tmux.calls)

    run_app(app, script)


def test_offer_banner_update_now_key_triggers_popup(tmp_path: Path) -> None:
    from tandem.status.app import OfferBanner

    ws = workspace(tmp_path)
    cfg = _make_config(tmp_path, ws)
    tmux = FakeTmux()
    app = StatusApp(ws, CoordStore(tmp_path / "coord"), tmux=tmux,  # type: ignore[arg-type]
                    config=cfg, update_runner=_fake_runner("1.4.0"), now=lambda: 1_700_000_000.0)

    async def script(p: Pilot[None]) -> None:
        await app.workers.wait_for_complete()
        await p.pause()
        assert len(app.query(OfferBanner)) == 1
        await p.press("alt+u")
        await p.pause()
        assert any("display-popup" in c[0] and "--yes" in c[-1] for c in tmux.calls)

    run_app(app, script)


def test_offer_banner_hidden_in_compact_mode(tmp_path: Path) -> None:
    from tandem.status.app import OfferBanner

    ws = workspace(tmp_path)
    cfg = _make_config(tmp_path, ws)
    app = StatusApp(ws, CoordStore(tmp_path / "coord"), config=cfg,
                    update_runner=_fake_runner("1.4.0"), now=lambda: 1_700_000_000.0)

    async def script(p: Pilot[None]) -> None:
        await app.workers.wait_for_complete()
        await p.pause()
        offers = app.query(OfferBanner)
        assert len(offers) == 1
        assert offers.first().display is False

    run_app(app, script, size=(100, 6))


def test_offer_banner_not_shown_when_update_check_disabled(tmp_path: Path) -> None:
    from tandem.status.app import OfferBanner

    ws = workspace(tmp_path)
    (ws / ".agent-workspace").mkdir(parents=True, exist_ok=True)
    (ws / ".agent-workspace" / "manifest.json").write_text('{"version": "1.3.0"}')
    cfg = Config.from_env({"TANDEM_WORKSPACE": str(ws), "TANDEM_UPDATE_CHECK": "0"})
    app = StatusApp(ws, CoordStore(tmp_path / "coord"), config=cfg,
                    update_runner=_fake_runner("1.4.0"), now=lambda: 1_700_000_000.0)

    async def script(p: Pilot[None]) -> None:
        await app.workers.wait_for_complete()
        await p.pause()
        assert len(app.query(OfferBanner)) == 0

    run_app(app, script)


def test_offer_banner_not_shown_for_skipped_version(tmp_path: Path) -> None:
    from tandem.status.app import OfferBanner
    from tandem.update_check import CheckState, write_state

    ws = workspace(tmp_path)
    cfg = _make_config(tmp_path, ws)
    write_state(ws, CheckState(last=0.0, latest="1.4.0", skip="1.4.0"))
    app = StatusApp(ws, CoordStore(tmp_path / "coord"), config=cfg,
                    update_runner=_fake_runner("1.4.0"), now=lambda: 1_700_000_000.0)

    async def script(p: Pilot[None]) -> None:
        await app.workers.wait_for_complete()
        await p.pause()
        assert len(app.query(OfferBanner)) == 0

    run_app(app, script)


def test_offer_banner_not_due_when_recently_checked(tmp_path: Path) -> None:
    from tandem.status.app import OfferBanner
    from tandem.update_check import CheckState, write_state

    ws = workspace(tmp_path)
    cfg = _make_config(tmp_path, ws, interval=3600)
    now_ts = 1_700_000_000.0
    write_state(ws, CheckState(last=now_ts - 100, latest="", skip=""))
    app = StatusApp(ws, CoordStore(tmp_path / "coord"), config=cfg,
                    update_runner=_fake_runner("1.4.0"), now=lambda: now_ts)

    async def script(p: Pilot[None]) -> None:
        await app.workers.wait_for_complete()
        await p.pause()
        assert len(app.query(OfferBanner)) == 0

    run_app(app, script)


def test_offer_banner_tmux_message_sent_once(tmp_path: Path) -> None:
    from tandem.status.app import OfferBanner

    ws = workspace(tmp_path)
    cfg = _make_config(tmp_path, ws)
    tmux = FakeTmux()
    app = StatusApp(ws, CoordStore(tmp_path / "coord"), tmux=tmux,  # type: ignore[arg-type]
                    config=cfg, update_runner=_fake_runner("1.4.0"), now=lambda: 1_700_000_000.0)

    async def script(p: Pilot[None]) -> None:
        await app.workers.wait_for_complete()
        await p.pause()
        msg_calls = [c for c in tmux.calls if c[0] == "display-message"]
        assert len(msg_calls) == 1
        assert "1.4.0" in msg_calls[0][-1]

    run_app(app, script)


def test_offer_update_now_key_no_op_without_offer(tmp_path: Path) -> None:
    """Alt+u does nothing when no offer is showing."""
    ws = workspace(tmp_path)
    cfg = _make_config(tmp_path, ws)
    tmux = FakeTmux()
    app = StatusApp(ws, CoordStore(tmp_path / "coord"), tmux=tmux,  # type: ignore[arg-type]
                    config=cfg, update_runner=_fake_runner("1.3.0"), now=lambda: 1_700_000_000.0)

    async def script(p: Pilot[None]) -> None:
        await app.workers.wait_for_complete()
        await p.pause()
        popup_calls_before = [c for c in tmux.calls if c[0] == "display-popup"]
        await p.press("alt+u")
        await p.pause()
        popup_calls_after = [c for c in tmux.calls if c[0] == "display-popup"]
        assert len(popup_calls_after) == len(popup_calls_before)

    run_app(app, script)


def test_update_now_footer_binding_visible_when_offer_showing(tmp_path: Path) -> None:
    from tandem.status.app import OfferBanner

    ws = workspace(tmp_path)
    cfg = _make_config(tmp_path, ws)
    app = StatusApp(ws, CoordStore(tmp_path / "coord"), config=cfg,
                    update_runner=_fake_runner("1.4.0"), now=lambda: 1_700_000_000.0)

    async def script(p: Pilot[None]) -> None:
        await app.workers.wait_for_complete()
        await p.pause()
        assert len(app.query(OfferBanner)) == 1
        assert app.check_action("update_now", ()) is True

    run_app(app, script)


def test_update_now_footer_binding_hidden_when_no_offer(tmp_path: Path) -> None:
    from tandem.status.app import OfferBanner
    from tandem.update_check import CheckState, write_state

    ws = workspace(tmp_path)
    cfg = _make_config(tmp_path, ws)
    now_ts = 1_700_000_000.0
    write_state(ws, CheckState(last=now_ts - 10, latest="1.3.0", skip=""))
    app = StatusApp(ws, CoordStore(tmp_path / "coord"), config=cfg,
                    update_runner=_fake_runner("1.3.0"), now=lambda: now_ts)

    async def script(p: Pilot[None]) -> None:
        await app.workers.wait_for_complete()
        await p.pause()
        assert len(app.query(OfferBanner)) == 0
        assert app.check_action("update_now", ()) is False

    run_app(app, script)


def test_offer_display_popup_argv_uses_update_yes(tmp_path: Path) -> None:
    """display-popup must end with 'update --yes' so the update runs non-interactively."""
    from tandem.status.app import OfferBanner

    ws = workspace(tmp_path)
    cfg = _make_config(tmp_path, ws)
    tmux = FakeTmux()
    app = StatusApp(ws, CoordStore(tmp_path / "coord"), tmux=tmux,  # type: ignore[arg-type]
                    config=cfg, update_runner=_fake_runner("1.4.0"), now=lambda: 1_700_000_000.0)

    async def script(p: Pilot[None]) -> None:
        await app.workers.wait_for_complete()
        await p.pause()
        await p.click("#offer-update")
        await p.pause()
        popup_calls = [c for c in tmux.calls if c[0] == "display-popup"]
        assert len(popup_calls) == 1
        cmd = popup_calls[0][-1]
        assert "update" in cmd and "--yes" in cmd

    run_app(app, script)
