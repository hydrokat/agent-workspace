from pathlib import Path

import pytest

from tandem.wizard.flow import Step, WizardFlow


def current(flow: WizardFlow) -> Step:
    return flow.step


def make() -> WizardFlow:
    return WizardFlow(installed={"claude": True, "agy": True})


def test_claude_planner_skips_perms_and_cwd() -> None:
    f = make()
    assert current(f) is Step.CLI
    f.choose_cli("claude")
    f.choose_role("planner")
    assert current(f) is Step.MODEL
    f.choose_model("opus")
    assert current(f) is Step.SUMMARY
    assert [(a.cli, a.role, a.model, a.index) for a in f.agents] == [("claude", "planner", "opus", 1)]


def test_agy_asks_permissions_default_no() -> None:
    f = make()
    f.choose_cli("agy")
    f.choose_role("planner")
    f.choose_model("gemini-3.8-flash-high")
    assert current(f) is Step.PERMS
    f.choose_skip_permissions(False)
    assert f.agents[0].skip_permissions is False


def test_implementer_asks_for_cwd(tmp_path: Path) -> None:
    f = make()
    f.choose_cli("claude")
    f.choose_role("implementer")
    f.choose_model("sonnet")
    assert current(f) is Step.CWD
    f.choose_cwd(str(tmp_path))
    assert f.agents[0].cwd == str(tmp_path) and current(f) is Step.SUMMARY


def test_blank_cwd_means_workspace_root() -> None:
    f = make()
    f.choose_cli("claude")
    f.choose_role("implementer")
    f.choose_model("sonnet")
    f.choose_cwd("")
    assert f.agents[0].cwd is None


def test_missing_cwd_rejected(tmp_path: Path) -> None:
    f = make()
    f.choose_cli("claude")
    f.choose_role("implementer")
    f.choose_model("sonnet")
    with pytest.raises(ValueError):
        f.choose_cwd(str(tmp_path / "nope"))
    assert current(f) is Step.CWD


def test_uninstalled_cli_cannot_be_chosen() -> None:
    f = WizardFlow(installed={"claude": True, "agy": False})
    with pytest.raises(ValueError):
        f.choose_cli("agy")


def test_bad_model_rejected_and_step_kept() -> None:
    f = make()
    f.choose_cli("claude")
    f.choose_role("planner")
    with pytest.raises(ValueError):
        f.choose_model("--evil")
    assert current(f) is Step.MODEL


def test_indexes_count_per_role_and_duplicates_allowed() -> None:
    f = make()
    for _ in range(2):
        f.add_another()
        f.choose_cli("claude")
        f.choose_role("planner")
        f.choose_model("opus")
    assert [a.index for a in f.agents] == [1, 2]


def test_delete_renumbers() -> None:
    f = make()
    for _ in range(2):
        f.add_another()
        f.choose_cli("claude")
        f.choose_role("planner")
        f.choose_model("opus")
    f.delete(0)
    assert [a.index for a in f.agents] == [1]


def test_edit_removes_and_restarts_flow() -> None:
    f = make()
    f.choose_cli("claude")
    f.choose_role("planner")
    f.choose_model("opus")
    f.edit(0)
    assert f.agents == [] and current(f) is Step.CLI


def test_back_goes_one_step() -> None:
    f = make()
    f.choose_cli("claude")
    f.choose_role("planner")
    f.back()
    assert current(f) is Step.ROLE
    f.back()
    assert current(f) is Step.CLI


def test_launch_requires_an_agent() -> None:
    f = make()
    assert f.can_launch is False
    f.choose_cli("claude")
    f.choose_role("planner")
    f.choose_model("opus")
    assert f.can_launch is True
