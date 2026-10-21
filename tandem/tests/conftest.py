import pytest

from tandem import commands


@pytest.fixture(autouse=True)
def no_cli_version_probe(monkeypatch: pytest.MonkeyPatch) -> None:
    """Launching an agent records its CLI version; tests must not run the real `claude --version`."""
    monkeypatch.setattr(commands, "cli_version", lambda cli: "")
