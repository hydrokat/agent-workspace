import pytest

from tandem import __version__
from tandem.cli import main


def test_version_flag(capsys: pytest.CaptureFixture[str]) -> None:
    with pytest.raises(SystemExit) as exc:
        main(["--version"])
    assert exc.value.code == 0
    assert __version__ in capsys.readouterr().out


def test_help_flag(capsys: pytest.CaptureFixture[str]) -> None:
    with pytest.raises(SystemExit) as exc:
        main(["--help"])
    assert exc.value.code == 0
    assert "tandem" in capsys.readouterr().out


def test_unknown_flag_exits_nonzero() -> None:
    with pytest.raises(SystemExit) as exc:
        main(["--nope"])
    assert exc.value.code != 0


def test_importing_tandem_commands_does_not_load_hashlib() -> None:
    import subprocess
    import sys

    code = "import sys, tandem.cli, tandem.commands, tandem.status.app; sys.exit('hashlib' in sys.modules)"
    assert subprocess.run([sys.executable, "-c", code]).returncode == 0


def test_click_subcommand_in_help() -> None:
    with pytest.raises(SystemExit) as exc:
        main(["--help"])
    assert exc.value.code == 0


def test_click_unknown_action_is_no_op(tmp_path: pytest.TempPathFactory) -> None:
    from tandem.commands import click
    from tandem.config import Config
    from tandem.tmux.client import TmuxClient, ToSViolation
    from tandem.tmux.layout import Layout

    calls: list[tuple[str, ...]] = []

    class FakeClient(TmuxClient):
        def run(self, *args: str) -> object:  # type: ignore[override]
            calls.append(args)
            from tandem.tmux.client import Result
            return Result(0, "", "")

    layout = Layout(FakeClient(), python="/py", workspace=None)
    config = Config.from_env({})
    click(layout, config, "")
    click(layout, config, "BADACTION")
    click(layout, config, "send-keys")
    assert calls == [], "unknown actions must produce no tmux calls"


def test_click_allow_list_matches_click_actions() -> None:
    from tandem.tmux.keys import CLICK_ACTIONS

    allowed = frozenset(CLICK_ACTIONS)
    assert "help" in allowed
    assert "add-agent" in allowed
    assert "quit" in allowed
    assert "send-keys" not in allowed
    assert "capture-pane" not in allowed
    assert "" not in allowed


class _TabClickHarness:
    def __init__(self, role: str = "planner", panes: "list[object] | None" = None) -> None:
        from tandem.config import Config
        from tandem.tmux.client import Result, TmuxClient
        from tandem.tmux.layout import Layout
        from tandem.tmux.tabs import PaneInfo

        self.checked: list[tuple[str, ...]] = []
        self.selected: list[tuple[str, int]] = []
        role_out = role
        harness = self

        class FakeClient(TmuxClient):
            def run(self, *args: str) -> Result:
                return Result(0, role_out + "\n", "")

            def run_checked(self, *args: str) -> str:
                harness.checked.append(args)
                return ""

        self.panes = panes if panes is not None else [
            PaneInfo("%1", "main", "agent", "planner", 1, "claude", False, None),
            PaneInfo("%2", "stash", "agent", "planner", 2, "agy", True, 3),
            PaneInfo("%9", "main", "agent", "implementer", 1, "codex", False, None),
        ]

        class FakeLayout(Layout):
            def panes(self) -> list[PaneInfo]:
                return harness.panes  # type: ignore[return-value]

            def select(self, role: str, index: int) -> None:
                harness.selected.append((role, index))

        self.layout = FakeLayout(FakeClient(), python="/py", workspace=None)
        self.config = Config.from_env({})


def test_click_tab_opens_a_menu_of_the_columns_agents() -> None:
    from tandem.commands import click_tab

    h = _TabClickHarness()
    click_tab(h.layout, h.config, "%1")

    assert len(h.checked) == 1
    args = h.checked[0]
    assert args[:2] == ("display-menu", "-T") and args[3:5] == ("-t", "%1")
    assert "#{pane_left}" in args and "#{pane_top}" in args
    items = args[args.index("#{pane_top}") + 1:]
    assert items[0] == "▶ 1 claude" and items[1] == "1"
    assert "click-tab %1 --select %1" in items[2]
    assert items[3] == "  ⚠2 agy(exited 3)" and items[4] == "2"
    assert "click-tab %2 --select %2" in items[5]
    assert "codex" not in " ".join(items), "other columns' agents are not listed"
    assert items[6:] == ("", "New agent", "n", items[-1]) and "add-agent" in items[-1]


def test_click_tab_empty_column_opens_add_agent() -> None:
    from tandem.commands import click_tab

    h = _TabClickHarness(panes=[])
    click_tab(h.layout, h.config, "%1")
    assert [a[0] for a in h.checked] == ["display-popup"] and "add-agent" in h.checked[0][-1]


def test_click_tab_select_switches_to_that_agent() -> None:
    from tandem.commands import click_tab

    h = _TabClickHarness()
    click_tab(h.layout, h.config, "%1", "%2")
    assert h.selected == [("planner", 2)] and h.checked == []


def test_click_tab_select_ignores_other_columns_and_unknown_panes() -> None:
    from tandem.commands import click_tab

    h = _TabClickHarness()
    for bad in ("%9", "%77", "2", "%2; kill-server", "$(id)"):
        click_tab(h.layout, h.config, "%1", bad)
    assert h.selected == [] and h.checked == []


def test_click_tab_ignores_invalid_pane_ids_and_non_column_panes() -> None:
    from tandem.commands import click_tab

    h = _TabClickHarness()
    for bad in ("", "1", "%x", "%1; kill-server", "\\%1"):
        click_tab(h.layout, h.config, bad)
    own = _TabClickHarness(role="status")
    click_tab(own.layout, own.config, "%3")
    assert h.checked == [] and own.checked == [] and own.selected == []


def test_click_tab_parser_takes_only_a_pane_and_optional_select() -> None:
    from tandem.cli import build_parser

    ns = build_parser().parse_args(["click-tab", "%0"])
    assert ns.pane == "%0" and ns.select == ""
    ns = build_parser().parse_args(["click-tab", "%0", "--select", "%2"])
    assert ns.select == "%2"
