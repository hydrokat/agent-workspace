import asyncio
from collections.abc import Awaitable, Callable

from textual.pilot import Pilot
from textual.widgets import OptionList

from tandem.adapters.base import AgentSpec, Model, ModelListError
from tandem.tmux import keys
from tandem.wizard.app import WizardApp


def loader(cli: str) -> list[Model]:
    if cli == "agy":
        return [Model("gemini-3.8-flash-high", "Flash High"), Model("gemini-3.8-flash-low", "Flash Low")]
    return [Model("opus", "opus (latest)"), Model("sonnet", "sonnet (latest)")]


def failing_loader(cli: str) -> list[Model]:
    raise ModelListError("timeout")


def drive(app: WizardApp, script: Callable[[Pilot[list[AgentSpec] | None]], Awaitable[None]]) -> list[AgentSpec] | None:
    async def go() -> list[AgentSpec] | None:
        async with app.run_test(size=(100, 30)) as pilot:
            await script(pilot)
            await pilot.pause()
        return app.return_value

    return asyncio.run(go())


def app(single: bool = False, load: Callable[[str], list[Model]] = loader) -> WizardApp:
    return WizardApp(installed={"claude": True, "agy": True}, model_loader=load, single=single)


def test_keyboard_flow_launches_claude_planner() -> None:
    async def script(p: Pilot[list[AgentSpec] | None]) -> None:
        await p.press("enter")  # claude
        await p.press("enter")  # planner
        await p.app.workers.wait_for_complete()
        await p.pause()
        await p.press("enter")  # first model
        await p.press("L")

    result = drive(app(), script)
    assert result is not None and [(a.cli, a.role, a.model) for a in result] == [("claude", "planner", "opus")]


def test_mouse_click_selects_option() -> None:
    async def script(p: Pilot[list[AgentSpec] | None]) -> None:
        await p.click(OptionList, offset=(4, 2))  # second option: agy
        await p.press("down")  # role: implementer
        await p.press("enter")
        await p.app.workers.wait_for_complete()
        await p.pause()
        await p.press("enter")  # model
        await p.press("enter")  # perms: No (default)
        await p.press("enter")  # cwd blank
        await p.press("L")

    result = drive(app(), script)
    assert result is not None and (result[0].cli, result[0].role) == ("agy", "implementer")
    assert result[0].skip_permissions is False


def test_model_load_failure_allows_typing_an_id() -> None:
    async def script(p: Pilot[list[AgentSpec] | None]) -> None:
        await p.press("down", "enter")  # agy
        await p.press("enter")  # planner
        await p.app.workers.wait_for_complete()
        await p.pause()
        await p.press(*"my-model", "enter")
        await p.press("enter")  # perms No
        await p.press("L")

    result = drive(app(load=failing_loader), script)
    assert result is not None and result[0].model == "my-model"


def test_invalid_typed_model_is_rejected_with_message() -> None:
    async def script(p: Pilot[list[AgentSpec] | None]) -> None:
        await p.press("enter", "enter")
        await p.app.workers.wait_for_complete()
        await p.pause()
        await p.press(*"a;b", "enter")
        await p.pause()
        assert "Warning" in str(p.app.query_one("#message").render())
        await p.press("escape")
        await p.press("escape")
        await p.press("escape")

    assert drive(app(load=failing_loader), script) is None


def test_single_mode_returns_after_one_agent() -> None:
    async def script(p: Pilot[list[AgentSpec] | None]) -> None:
        await p.press("enter", "enter")
        await p.app.workers.wait_for_complete()
        await p.pause()
        await p.press("enter")

    result = drive(app(single=True), script)
    assert result is not None and len(result) == 1


def test_launch_disabled_without_agents() -> None:
    async def script(p: Pilot[list[AgentSpec] | None]) -> None:
        await p.press("L")
        await p.press("escape")

    assert drive(app(), script) is None


def test_hint_bar_lists_keys() -> None:
    async def script(p: Pilot[list[AgentSpec] | None]) -> None:
        text = str(p.app.query_one("#hints").render())
        assert "Enter" in text and "Esc" in text
        await p.press("escape")

    drive(app(), script)


def test_model_step_alt_f_focuses_filter() -> None:
    async def script(p: Pilot[list[AgentSpec] | None]) -> None:
        await p.press("enter")  # cli: claude
        await p.press("enter")  # role: planner
        await p.app.workers.wait_for_complete()
        await p.pause()
        hints = str(p.app.query_one("#hints").render())
        assert f"{keys.alt_name()}+f" in hints
        await p.press("alt+f")
        assert p.app.focused == p.app.query_one("#input")
        await p.press("escape")

    drive(app(), script)


def test_summary_step_actions_use_modifier_keys() -> None:
    async def script(p: Pilot[list[AgentSpec] | None]) -> None:
        await p.press("enter")  # claude
        await p.press("enter")  # planner
        await p.app.workers.wait_for_complete()
        await p.pause()
        await p.press("enter")  # model
        hints = str(p.app.query_one("#hints").render())
        alt = keys.alt_name()
        assert f"{alt}+a" in hints and f"{alt}+d" in hints and "Shift+L" in hints
        await p.press("alt+a")
        assert isinstance(p.app, WizardApp)
        assert p.app.flow.step.name == "CLI"
        await p.press("escape")

    drive(app(), script)


def test_wizard_help_key_shows_notification() -> None:
    async def script(p: Pilot[list[AgentSpec] | None]) -> None:
        await p.press("question_mark")
        assert any(n.title == "Keys" for n in p.app._notifications)

    drive(app(), script)

