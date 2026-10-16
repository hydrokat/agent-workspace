from tandem.wizard.runner import installed_clis, load_models


def test_installed_clis_reports_both_keys() -> None:
    assert set(installed_clis()) == {"claude", "agy"}


def test_load_models_for_claude_needs_no_subprocess() -> None:
    assert [m.id for m in load_models("claude")][0] == "opus"
