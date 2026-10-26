from pathlib import Path

import pytest

from tandem.notice import (
    Notice,
    mark_seen,
    notice_path,
    read_notice,
    whats_new,
    write_notice,
)

CHANGELOG = """\
# Changelog

## [Unreleased]
- Not yet in a release

## [1.3.0] - 2024-01-01
### Added
- **Clickable buttons**: click the status bar
- Click an agent tab to switch

## [1.2.0] - 2023-12-01
- Old feature. Details here.
"""


# ---------------------------------------------------------------------------
# notice_path
# ---------------------------------------------------------------------------


def test_notice_path(tmp_path: Path) -> None:
    assert notice_path(tmp_path) == tmp_path / ".agent-workspace" / "update-notice.env"


# ---------------------------------------------------------------------------
# read_notice — valid inputs
# ---------------------------------------------------------------------------


def test_read_notice_valid(tmp_path: Path) -> None:
    path = notice_path(tmp_path)
    path.parent.mkdir(parents=True)
    path.write_text(
        "TANDEM_UPDATE_FROM=1.2.0\n"
        "TANDEM_UPDATE_TO=1.3.0\n"
        "TANDEM_UPDATE_AT=1700000000\n"
    )
    n = read_notice(tmp_path)
    assert n is not None
    assert n.from_version == "1.2.0"
    assert n.to_version == "1.3.0"
    assert n.at == 1700000000
    assert n.seen is False


def test_read_notice_with_seen_flag(tmp_path: Path) -> None:
    path = notice_path(tmp_path)
    path.parent.mkdir(parents=True)
    path.write_text(
        "TANDEM_UPDATE_FROM=1.2.0\n"
        "TANDEM_UPDATE_TO=1.3.0\n"
        "TANDEM_UPDATE_AT=1700000000\n"
        "TANDEM_UPDATE_SEEN=1\n"
    )
    n = read_notice(tmp_path)
    assert n is not None and n.seen is True


def test_read_notice_ignores_blank_lines(tmp_path: Path) -> None:
    path = notice_path(tmp_path)
    path.parent.mkdir(parents=True)
    path.write_text(
        "TANDEM_UPDATE_FROM=1.2.0\n"
        "\n"
        "TANDEM_UPDATE_TO=1.3.0\n"
        "TANDEM_UPDATE_AT=0\n"
    )
    assert read_notice(tmp_path) is not None


# ---------------------------------------------------------------------------
# read_notice — invalid inputs → None
# ---------------------------------------------------------------------------


def test_read_notice_missing_file(tmp_path: Path) -> None:
    assert read_notice(tmp_path) is None


def test_read_notice_invalid_line_format(tmp_path: Path) -> None:
    path = notice_path(tmp_path)
    path.parent.mkdir(parents=True)
    path.write_text(
        "TANDEM_UPDATE_FROM=1.2.0\n"
        "BAD LINE\n"
        "TANDEM_UPDATE_TO=1.3.0\n"
        "TANDEM_UPDATE_AT=0\n"
    )
    assert read_notice(tmp_path) is None


def test_read_notice_unknown_key_rejected(tmp_path: Path) -> None:
    path = notice_path(tmp_path)
    path.parent.mkdir(parents=True)
    path.write_text(
        "TANDEM_UPDATE_FROM=1.2.0\n"
        "TANDEM_UPDATE_TO=1.3.0\n"
        "TANDEM_UPDATE_AT=0\n"
        "UNKNOWN_KEY=value\n"
    )
    assert read_notice(tmp_path) is None


def test_read_notice_shell_injection_in_value_rejected(tmp_path: Path) -> None:
    path = notice_path(tmp_path)
    path.parent.mkdir(parents=True)
    path.write_text(
        "TANDEM_UPDATE_FROM=1.2.0\n"
        "TANDEM_UPDATE_TO=$(evil)\n"
        "TANDEM_UPDATE_AT=0\n"
    )
    assert read_notice(tmp_path) is None


def test_read_notice_space_in_value_rejected(tmp_path: Path) -> None:
    path = notice_path(tmp_path)
    path.parent.mkdir(parents=True)
    path.write_text(
        "TANDEM_UPDATE_FROM=1 2 0\n"
        "TANDEM_UPDATE_TO=1.3.0\n"
        "TANDEM_UPDATE_AT=0\n"
    )
    assert read_notice(tmp_path) is None


def test_read_notice_missing_required_key(tmp_path: Path) -> None:
    path = notice_path(tmp_path)
    path.parent.mkdir(parents=True)
    path.write_text(
        "TANDEM_UPDATE_FROM=1.2.0\n"
        "TANDEM_UPDATE_TO=1.3.0\n"  # missing AT
    )
    assert read_notice(tmp_path) is None


def test_read_notice_non_integer_at(tmp_path: Path) -> None:
    path = notice_path(tmp_path)
    path.parent.mkdir(parents=True)
    # letters-only value passes regex but int() fails
    path.write_text(
        "TANDEM_UPDATE_FROM=1.2.0\n"
        "TANDEM_UPDATE_TO=1.3.0\n"
        "TANDEM_UPDATE_AT=notanumber\n"
    )
    assert read_notice(tmp_path) is None


# ---------------------------------------------------------------------------
# write_notice
# ---------------------------------------------------------------------------


def test_write_notice_creates_parseable_file(tmp_path: Path) -> None:
    write_notice(tmp_path, "1.2.0", "1.3.0", at=1700000000)
    n = read_notice(tmp_path)
    assert n is not None
    assert n.from_version == "1.2.0" and n.to_version == "1.3.0" and n.at == 1700000000
    assert n.seen is False


def test_write_notice_creates_parent_dirs(tmp_path: Path) -> None:
    ws = tmp_path / "deep" / "workspace"
    write_notice(ws, "1.0.0", "1.1.0", at=0)
    assert notice_path(ws).exists()


def test_write_notice_defaults_at_to_now(tmp_path: Path) -> None:
    import time

    before = int(time.time())
    write_notice(tmp_path, "1.0.0", "1.1.0")
    after = int(time.time())
    n = read_notice(tmp_path)
    assert n is not None and before <= n.at <= after


# ---------------------------------------------------------------------------
# mark_seen
# ---------------------------------------------------------------------------


def test_mark_seen_sets_seen_flag(tmp_path: Path) -> None:
    write_notice(tmp_path, "1.2.0", "1.3.0", at=1700000000)
    assert read_notice(tmp_path) is not None
    assert read_notice(tmp_path).seen is False  # type: ignore[union-attr]
    mark_seen(tmp_path)
    n = read_notice(tmp_path)
    assert n is not None and n.seen is True


def test_mark_seen_preserves_existing_fields(tmp_path: Path) -> None:
    write_notice(tmp_path, "1.2.0", "1.3.0", at=1700000000)
    mark_seen(tmp_path)
    n = read_notice(tmp_path)
    assert n is not None
    assert n.from_version == "1.2.0" and n.to_version == "1.3.0" and n.at == 1700000000


def test_mark_seen_is_idempotent(tmp_path: Path) -> None:
    write_notice(tmp_path, "1.2.0", "1.3.0", at=1700000000)
    mark_seen(tmp_path)
    mark_seen(tmp_path)
    n = read_notice(tmp_path)
    assert n is not None and n.seen is True


def test_mark_seen_no_notice_is_noop(tmp_path: Path) -> None:
    mark_seen(tmp_path)  # must not raise


# ---------------------------------------------------------------------------
# whats_new
# ---------------------------------------------------------------------------


def test_whats_new_extracts_bullets_in_range() -> None:
    bullets = whats_new(CHANGELOG, "1.2.0", "1.3.0")
    assert len(bullets) == 2
    assert "Clickable buttons" in bullets[0]
    assert "Click an agent tab" in bullets[1]


def test_whats_new_bold_label_is_headline() -> None:
    log = "## [1.1.0]\n- **Bold label**: some long description goes here\n"
    bullets = whats_new(log, "1.0.0", "1.1.0")
    assert bullets == ["Bold label"]


def test_whats_new_first_sentence_without_bold() -> None:
    log = "## [1.1.0]\n- First sentence. More detail here.\n"
    bullets = whats_new(log, "1.0.0", "1.1.0")
    assert bullets == ["First sentence."]


def test_whats_new_full_line_when_no_period() -> None:
    log = "## [1.1.0]\n- No period in this line\n"
    bullets = whats_new(log, "1.0.0", "1.1.0")
    assert bullets == ["No period in this line"]


def test_whats_new_falls_back_to_unreleased() -> None:
    bullets = whats_new(CHANGELOG, "1.3.0", "2.0.0")
    assert len(bullets) == 1
    assert "Not yet in a release" in bullets[0]


def test_whats_new_no_match_and_no_unreleased_returns_empty() -> None:
    log = "## [1.3.0]\n- Something\n"
    assert whats_new(log, "1.3.0", "2.0.0") == []


def test_whats_new_respects_limit() -> None:
    log = "## [1.1.0]\n" + "- item\n" * 20
    bullets = whats_new(log, "1.0.0", "1.1.0", limit=5)
    assert len(bullets) == 5


def test_whats_new_sanitizes_escape_sequences() -> None:
    log = "## [1.1.0]\n- \x1b[31mevil\x1b[0m text\n"
    bullets = whats_new(log, "1.0.0", "1.1.0")
    assert "\x1b" not in bullets[0]
    assert "evil" in bullets[0]


def test_whats_new_from_version_not_included() -> None:
    """Sections at exactly from_v are not included (open lower bound)."""
    log = "## [1.2.0]\n- Old feature\n## [1.3.0]\n- New feature\n"
    bullets = whats_new(log, "1.2.0", "1.3.0")
    assert len(bullets) == 1 and "New feature" in bullets[0]


def test_whats_new_to_version_is_included() -> None:
    """Sections at exactly to_v are included (closed upper bound)."""
    log = "## [1.3.0]\n- Latest feature\n"
    bullets = whats_new(log, "1.2.0", "1.3.0")
    assert len(bullets) == 1 and "Latest feature" in bullets[0]


def test_whats_new_invalid_from_v_returns_empty() -> None:
    assert whats_new(CHANGELOG, "not-a-version", "1.3.0") == []


def test_whats_new_invalid_to_v_returns_empty() -> None:
    assert whats_new(CHANGELOG, "1.2.0", "not-a-version") == []


def test_whats_new_empty_changelog_returns_empty() -> None:
    assert whats_new("", "1.2.0", "1.3.0") == []
