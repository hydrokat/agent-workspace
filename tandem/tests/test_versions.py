from pathlib import Path

import pytest

from tandem.versions import (
    InvalidMinVersionsError,
    Version,
    is_at_least,
    parse_min_versions,
    parse_version,
)


def test_parses_plain_semver() -> None:
    assert parse_version("1.3.6") == Version((1, 3, 6), "")


def test_parses_claude_banner() -> None:
    assert parse_version("2.1.301 (Claude Code)") == Version((2, 1, 301), "")


def test_parses_tmux_letter_suffix() -> None:
    assert parse_version("tmux 3.3a") == Version((3, 3), "a")


def test_parses_git_apple_banner() -> None:
    assert parse_version("git version 2.54.0 (Apple Git-157)") == Version((2, 54, 0), "")


def test_parses_python_banner() -> None:
    assert parse_version("Python 3.13.6") == Version((3, 13, 6), "")


def test_parses_next_style_tmux() -> None:
    assert parse_version("tmux next-3.5") == Version((3, 5), "")


@pytest.mark.parametrize("raw", ["", "no digits here", "v", "   "])
def test_unparseable_returns_none(raw: str) -> None:
    assert parse_version(raw) is None


def test_compare_numeric_not_lexical() -> None:
    assert is_at_least(Version((2, 10, 0), ""), Version((2, 9, 0), ""))
    assert not is_at_least(Version((2, 9, 0), ""), Version((2, 10, 0), ""))


def test_compare_pads_missing_components() -> None:
    assert is_at_least(Version((3, 3), ""), Version((3, 3, 0), ""))
    assert is_at_least(Version((3, 3, 0), ""), Version((3, 3), ""))


def test_letter_suffix_orders_after_bare() -> None:
    assert is_at_least(Version((3, 3), "a"), Version((3, 3), ""))
    assert not is_at_least(Version((3, 3), ""), Version((3, 3), "a"))


def test_parse_min_versions_reads_pairs_and_skips_comments(tmp_path: Path) -> None:
    f = tmp_path / "min.env"
    f.write_text("# header\n\nTANDEM_MIN_TMUX=3.3\nTANDEM_MIN_CLAUDE=2.1.301\n")
    assert parse_min_versions(f) == {"TANDEM_MIN_TMUX": "3.3", "TANDEM_MIN_CLAUDE": "2.1.301"}


@pytest.mark.parametrize(
    "line",
    [
        "X=$(rm -rf ~)",
        "X=`id`",
        "X=1.0; rm -rf ~",
        "X=${HOME}",
        "lower=1.0",
        "X = 1.0",
        "X=",
        "export X=1.0",
        "X=1.0 # trailing",
    ],
)
def test_parse_min_versions_rejects_unsafe_lines(tmp_path: Path, line: str) -> None:
    f = tmp_path / "min.env"
    f.write_text(f"TANDEM_MIN_TMUX=3.3\n{line}\n")
    with pytest.raises(InvalidMinVersionsError):
        parse_min_versions(f)


def test_parse_min_versions_never_executes_content(tmp_path: Path) -> None:
    marker = tmp_path / "pwned"
    f = tmp_path / "min.env"
    f.write_text(f"X=$(touch {marker})\n")
    with pytest.raises(InvalidMinVersionsError):
        parse_min_versions(f)
    assert not marker.exists()
