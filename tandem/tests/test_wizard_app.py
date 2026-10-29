import asyncio
from collections.abc import Awaitable, Callable

from textual.pilot import Pilot
from textual.widgets import Footer, OptionList

from tandem.adapters.base import AgentSpec, Model, ModelListError
from tandem.tmux import keys
from tandem.wizard.app import WizardApp
from tandem.wizard.flow import Step


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


def test_footer_replaces_hint_bar() -> None:
    async def script(p: Pilot[list[AgentSpec] | None]) -> None:
        assert len(p.app.query(Footer)) == 1
        assert isinstance(p.app, WizardApp)
        assert p.app.check_action("go_back", ()) is True
        assert p.app.check_action("help", ()) is True
        await p.press("escape")

    drive(app(), script)


def test_model_step_alt_f_focuses_filter() -> None:
    async def script(p: Pilot[list[AgentSpec] | None]) -> None:
        await p.press("enter")  # cli: claude
        await p.press("enter")  # role: planner
        await p.app.workers.wait_for_complete()
        await p.pause()
        assert isinstance(p.app, WizardApp)
        assert p.app.flow.step is Step.MODEL
        assert p.app.check_action("focus_input", ()) is True
        assert p.app.check_action("add", ()) is False
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
        assert isinstance(p.app, WizardApp)
        assert p.app.flow.step is Step.SUMMARY
        assert p.app.check_action("add", ()) is True
        assert p.app.check_action("delete", ()) is True
        assert p.app.check_action("launch", ()) is True
        assert p.app.check_action("focus_input", ()) is False
        await p.press("alt+a")
        assert p.app.flow.step.name == "CLI"
        await p.press("escape")

    drive(app(), script)


def test_wizard_help_key_shows_notification() -> None:
    async def script(p: Pilot[list[AgentSpec] | None]) -> None:
        await p.press("question_mark")
        assert any(n.title == "Keys" for n in p.app._notifications)

    drive(app(), script)


def test_check_action_filter_only_at_model_step() -> None:
    async def script(p: Pilot[list[AgentSpec] | None]) -> None:
        assert isinstance(p.app, WizardApp)
        assert p.app.check_action("focus_input", ()) is False
        await p.press("enter")
        await p.press("enter")
        await p.app.workers.wait_for_complete()
        await p.pause()
        assert p.app.flow.step is Step.MODEL
        assert p.app.check_action("focus_input", ()) is True
        assert p.app.check_action("add", ()) is False
        await p.press("escape")

    drive(app(), script)


def test_check_action_add_delete_launch_only_at_summary() -> None:
    async def script(p: Pilot[list[AgentSpec] | None]) -> None:
        await p.press("enter", "enter")
        await p.app.workers.wait_for_complete()
        await p.pause()
        await p.press("enter")
        assert isinstance(p.app, WizardApp)
        assert p.app.flow.step is Step.SUMMARY
        assert p.app.check_action("add", ()) is True
        assert p.app.check_action("delete", ()) is True
        assert p.app.check_action("launch", ()) is True
        assert p.app.check_action("focus_input", ()) is False
        await p.press("escape")

    drive(app(), script)


def test_wizard_key_display_is_os_aware() -> None:
    import sys
    from textual.binding import Binding as TextualBinding
    from tandem.tmux.keys import display_shortcut, WIZARD_ADD, WIZARD_FILTER

    wizard = app()

    async def script(p: Pilot[list[AgentSpec] | None]) -> None:
        tb_bindings = [b for b in wizard.BINDINGS if isinstance(b, TextualBinding)]
        add_binding = next(b for b in tb_bindings if b.action == "add")
        filter_binding = next(b for b in tb_bindings if b.action == "focus_input")
        assert add_binding.key_display == display_shortcut(WIZARD_ADD, sys.platform)
        assert filter_binding.key_display == display_shortcut(WIZARD_FILTER, sys.platform)
        await p.press("escape")

    drive(wizard, script)

