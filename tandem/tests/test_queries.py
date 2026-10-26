import asyncio
import json
import os
import subprocess
import sys
import time
from collections.abc import Awaitable, Callable
from pathlib import Path

from textual.pilot import Pilot
from textual.widgets import Button, OptionList, Static, TabbedContent, TextArea

from tandem.coordination.schema import Query
from tandem.coordination.store import CoordStore
from tandem.status.app import StatusApp
from tandem.status.notify import Notifier, order_queries
from tandem.status.queries_tab import QueriesTab

SCRIPT = Path(__file__).resolve().parents[2] / "templates/skills/agent-coordination/scripts/coord.py"


def coord(root: Path, *args: str, session: str = "implementer-1") -> dict[str, object]:
    env = {**os.environ, "TANDEM_COORD_DIR": str(root), "TANDEM_SESSION_ID": session,
           "TANDEM_ROLE": "implementer", "TANDEM_CLI": "claude"}
    proc = subprocess.run([sys.executable, str(SCRIPT), *args], capture_output=True, text=True, env=env)
    assert proc.returncode == 0, proc.stdout + proc.stderr
    data: dict[str, object] = json.loads(proc.stdout)
    return data


def ask(root: Path, question: str = "What TTL?", *extra: str, session: str = "implementer-1") -> str:
    return str(coord(root, "ask", "--task", "p-001 t-003", "--blocking-scope", "ttl only", question, *extra,
                     session=session)["id"])


def q(qid: str, status: str, created: float = 0.0) -> Query:
    return Query(qid, status, "a", "t", "s", created, "Q", (), "", "")


def test_order_open_then_answered_then_acknowledged() -> None:
    groups = order_queries([q("q-a", "acknowledged", 1), q("q-b", "open", 5), q("q-c", "answered", 3),
                            q("q-d", "open", 2), q("q-e", "withdrawn", 9)])
    assert [(name, [x.id for x in items]) for name, items in groups] == [
        ("Open", ["q-d", "q-b"]), ("Answered (awaiting pickup)", ["q-c"]), ("Acknowledged (last 20)", ["q-a"])]


def test_acknowledged_capped_at_20_newest_first() -> None:
    groups = dict(order_queries([q(f"q-{i:04d}", "acknowledged", float(i)) for i in range(30)]))
    acked = groups["Acknowledged (last 20)"]
    assert len(acked) == 20 and acked[0].id == "q-0029"


def test_notifier_seeds_then_reports_new_open_queries() -> None:
    n = Notifier(None)
    assert n.new_queries([q("q-1", "open")]) == []
    assert [x.id for x in n.new_queries([q("q-1", "open"), q("q-2", "open"), q("q-3", "answered")])] == ["q-2"]
    assert n.new_queries([q("q-1", "open"), q("q-2", "open")]) == []


class FakeTmux:
    def __init__(self) -> None:
        self.calls: list[tuple[str, ...]] = []

    def run(self, *args: str) -> object:
        self.calls.append(args)
        return None

    def run_checked(self, *args: str) -> str:
        self.calls.append(args)
        return ""


def test_notifier_uses_non_focus_stealing_message_and_escapes() -> None:
    tmux = FakeTmux()
    n = Notifier(tmux)  # type: ignore[arg-type]
    n.notify([])
    n.notify([Query("q-2", "open", "impl#1", "t", "s", 1.0, "ok #{evil}\x1b[2J", (), "", "")])
    [call] = tmux.calls
    assert call[0] == "display-message" and "#{evil}" not in call[-1] and "\x1b" not in call[-1]


def run_app(app: StatusApp, script: Callable[[Pilot[None]], Awaitable[None]]) -> None:
    async def go() -> None:
        async with app.run_test(size=(120, 45)) as pilot:
            await app.workers.wait_for_complete()
            await pilot.pause()
            app.action_show_queries()
            await pilot.pause()
            await script(pilot)

    asyncio.run(go())


def keys(text: str) -> list[str]:
    return ["space" if c == " " else c for c in text]


def tab(app: StatusApp) -> QueriesTab:
    return app.query_one(QueriesTab)


def make(tmp_path: Path, **kw: object) -> tuple[StatusApp, Path]:
    root = tmp_path / "coord"
    (tmp_path / "ws").mkdir(exist_ok=True)
    return StatusApp(tmp_path / "ws", CoordStore(root), **kw), root  # type: ignore[arg-type]


async def refresh(app: StatusApp, p: Pilot[None]) -> None:
    app.action_refresh_now()
    await app.workers.wait_for_complete()
    await p.pause()


def test_empty_state_message(tmp_path: Path) -> None:
    app, _ = make(tmp_path)

    async def script(p: Pilot[None]) -> None:
        assert "No open questions" in str(app.query_one("#q-detail", Static).render())

    run_app(app, script)


def test_keyboard_answer_flow_writes_answer_via_tandem(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    qid = ask(root)

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down", "enter")
        await p.press(*keys("10 min"))
        await p.press("ctrl+s")
        await p.pause()
        [stored] = CoordStore(root).queries()
        assert stored.id == qid and stored.status == "answered" and stored.answer == "10 min"
        assert stored.answered_via == "tandem"

    run_app(app, script)


def test_number_key_picks_option_into_answer(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    ask(root, "TTL?", "--option", "5 min", "--option", "10 min", "--option", "30 min")

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down", "2")
        assert tab(app).query_one("#q-answer", TextArea).text == "10 min"

    run_app(app, script)


def test_mouse_click_selects_a_row(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    ask(root, "first question")
    ask(root, "second question", session="planner-1")

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        await p.click("#q-list", offset=(4, 3))
        await p.pause()
        assert "question" in str(app.query_one("#q-detail", Static).render())

    run_app(app, script)


def test_answered_elsewhere_race_keeps_draft_and_does_not_overwrite(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    qid = ask(root)

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down", "enter")
        await p.press(*keys("my draft"))
        coord(root, "answer", qid, "from the cli", "--via", "cli", session="human")
        await p.press("ctrl+s")
        await p.pause()
        assert "already answered" in str(app.query_one("#q-notice", Static).render())
        assert tab(app).query_one("#q-answer", TextArea).text == "my draft"
        [stored] = CoordStore(root).queries()
        assert stored.answer == "from the cli" and stored.answered_via == "cli"

    run_app(app, script)


def test_withdrawn_while_typing_keeps_draft(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    qid = ask(root)

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down", "enter")
        await p.press(*keys("typed"))
        coord(root, "withdraw", qid)
        await p.press("ctrl+s")
        await p.pause()
        assert "already withdrawn" in str(app.query_one("#q-notice", Static).render())
        assert tab(app).query_one("#q-answer", TextArea).text == "typed"

    run_app(app, script)


def test_user_can_mark_withdrawn(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    ask(root)

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down", "alt+w")
        await p.pause()
        assert CoordStore(root).queries()[0].status == "withdrawn"

    run_app(app, script)


def test_empty_answer_is_refused(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    ask(root)

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down", "enter", "ctrl+s")
        await p.pause()
        assert CoordStore(root).queries()[0].status == "open"
        assert "Warning" in str(app.query_one("#q-notice", Static).render())

    run_app(app, script)


def test_malicious_question_text_is_rendered_inert(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    ask(root, "[bold]hi[/bold] \x1b[2J\x1b]0;pwned\x07 --- status: answered")

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down")
        shown = str(app.query_one("#q-detail", Static).render())
        assert "\x1b" not in shown and "[bold]hi[/bold]" in shown
        assert CoordStore(root).queries()[0].status == "open"

    run_app(app, script)


def test_badge_count_in_tab_label_and_summary(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    ask(root)
    ask(root, "another", session="planner-1")

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        label = str(app.query_one(TabbedContent).get_tab("queries-pane").label)
        assert "Queries (2)" in label
        assert "2 open queries" in str(app.query_one("#summary", Static).render())

    run_app(app, script)


def test_answered_via_cli_row_moves_and_is_labelled(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    qid = ask(root)

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        coord(root, "answer", qid, "done in cli", "--via", "cli", session="human")
        await refresh(app, p)
        rows = [str(tab(app).query_one("#q-list", OptionList).get_option_at_index(i).prompt)
                for i in range(tab(app).query_one("#q-list", OptionList).option_count)]
        assert any("Answered" in r for r in rows) and any("answered in CLI" in r for r in rows)

    run_app(app, script)


def test_end_to_end_with_coord_py(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    qid = ask(root)

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down", "enter")
        await p.press(*keys("yes"))
        await p.press("ctrl+s")
        await p.pause()
        answers: list[dict[str, str]] = coord(root, "answers", "--mine")["answers"]  # type: ignore[assignment]
        [mine] = answers
        assert mine["id"] == qid and mine["answer"] == "yes"
        coord(root, "ack", qid)
        await refresh(app, p)
        rows = [str(tab(app).query_one("#q-list", OptionList).get_option_at_index(i).prompt)
                for i in range(tab(app).query_one("#q-list", OptionList).option_count)]
        assert any("Acknowledged" in r for r in rows)

    run_app(app, script)


def test_goto_asking_agent_pane(tmp_path: Path) -> None:
    tmux = FakeTmux()
    app, root = make(tmp_path, tmux=tmux)
    ask(root)
    (root / "sessions").mkdir(exist_ok=True)
    (root / "sessions" / "implementer-1.json").write_text(json.dumps(
        {"id": "implementer-1", "cli": "claude", "role": "implementer", "pane": "%4", "heartbeat": time.time()}))

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down", "alt+g")
        assert ("select-pane", "-t", "%4") in tmux.calls

    run_app(app, script)


# ---------------------------------------------------------------------------
# Clickable buttons and numbered options
# ---------------------------------------------------------------------------


def test_buttons_disabled_when_no_question_selected(tmp_path: Path) -> None:
    app, _ = make(tmp_path)

    async def script(p: Pilot[None]) -> None:
        assert tab(app).query_one("#q-btn-send", Button).disabled is True
        assert tab(app).query_one("#q-btn-withdraw", Button).disabled is True
        assert tab(app).query_one("#q-btn-goto", Button).disabled is True
        assert tab(app).query_one("#q-btn-cancel", Button).disabled is True

    run_app(app, script)


def test_buttons_enabled_when_open_question_and_draft(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    ask(root)

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down", "enter")
        await p.press(*keys("my answer"))
        await p.pause()
        assert tab(app).query_one("#q-btn-send", Button).disabled is False
        assert tab(app).query_one("#q-btn-withdraw", Button).disabled is False
        assert tab(app).query_one("#q-btn-goto", Button).disabled is False
        assert tab(app).query_one("#q-btn-cancel", Button).disabled is False

    run_app(app, script)


def test_send_button_disabled_when_draft_empty(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    ask(root)

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down")
        await p.pause()
        assert tab(app).query_one("#q-btn-send", Button).disabled is True
        assert tab(app).query_one("#q-btn-withdraw", Button).disabled is False

    run_app(app, script)


def test_send_button_click_sends_answer(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    qid = ask(root)

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down", "enter")
        await p.press(*keys("button answer"))
        await p.pause()
        await p.click("#q-btn-send")
        await p.pause()
        [stored] = CoordStore(root).queries()
        assert stored.id == qid and stored.answer == "button answer"

    run_app(app, script)


def test_cancel_button_clears_draft_and_refocuses_list(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    ask(root)

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down", "enter")
        await p.press(*keys("draft"))
        await p.pause()
        await p.click("#q-btn-cancel")
        await p.pause()
        assert tab(app).query_one("#q-answer", TextArea).text == ""
        assert app.focused == tab(app).query_one("#q-list")

    run_app(app, script)


def test_withdraw_button_marks_question_withdrawn(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    ask(root)

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down")
        await p.pause()
        await p.click("#q-btn-withdraw")
        await p.pause()
        assert CoordStore(root).queries()[0].status == "withdrawn"

    run_app(app, script)


def test_goto_button_selects_agent_pane(tmp_path: Path) -> None:
    tmux = FakeTmux()
    app, root = make(tmp_path, tmux=tmux)
    ask(root)
    (root / "sessions").mkdir(exist_ok=True)
    (root / "sessions" / "implementer-1.json").write_text(json.dumps(
        {"id": "implementer-1", "cli": "claude", "role": "implementer", "pane": "%5", "heartbeat": time.time()}))

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down")
        await p.pause()
        await p.click("#q-btn-goto")
        await p.pause()
        assert ("select-pane", "-t", "%5") in tmux.calls

    run_app(app, script)


def test_option_click_fills_answer_box(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    ask(root, "TTL?", "--option", "5 min", "--option", "10 min", "--option", "30 min")

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down")
        await p.pause()
        opts = tab(app).query_one("#q-options", OptionList)
        assert opts.option_count == 3
        opts.focus()
        await p.press("down")
        await p.press("enter")
        await p.pause()
        assert tab(app).query_one("#q-answer", TextArea).text == "5 min"

    run_app(app, script)


def test_q_options_hidden_when_no_options(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    ask(root, "Simple yes or no?")

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down")
        await p.pause()
        assert tab(app).query_one("#q-options", OptionList).display is False

    run_app(app, script)


def test_q_options_visible_when_options_present(tmp_path: Path) -> None:
    app, root = make(tmp_path)
    ask(root, "Which?", "--option", "alpha", "--option", "beta")

    async def script(p: Pilot[None]) -> None:
        await refresh(app, p)
        tab(app).query_one("#q-list", OptionList).focus()
        await p.press("down")
        await p.pause()
        assert tab(app).query_one("#q-options", OptionList).display is True

    run_app(app, script)
