from pathlib import Path

from tandem.tmux import keys

README = (Path(__file__).resolve().parents[1] / "README.md").read_text()


def test_readme_keymap_matches_keys_py() -> None:
    for b in keys.KEYMAP:
        assert f"| `{b.key}` | {b.description} |" in README


def test_readme_states_tos_rule() -> None:
    assert "never types into them" in README


def test_readme_documents_update_check_env() -> None:
    assert "TANDEM_UPDATE_CHECK" in README


def test_readme_documents_update_interval_env() -> None:
    assert "TANDEM_UPDATE_INTERVAL" in README


def test_readme_documents_no_update_flag() -> None:
    assert "--no-update" in README
