"""Architectural guard for the ToS guardrails in specs p-001: tandem never types into or reads AI panes."""
import re
from pathlib import Path

SRC = Path(__file__).resolve().parents[1] / "src" / "tandem"
FORBIDDEN = re.compile(r"send-keys|capture-pane|pipe-pane|send_keys(?!_own)|capture_pane|paste-buffer|load-buffer|copy-mode")
ALLOWED_FILES = {SRC / "tmux" / "client.py"}


def test_forbidden_tmux_operations_only_appear_in_the_guarded_client() -> None:
    offenders = [
        f"{path.relative_to(SRC)}:{n}: {line.strip()}"
        for path in SRC.rglob("*.py") if path not in ALLOWED_FILES
        for n, line in enumerate(path.read_text().splitlines(), 1)
        if FORBIDDEN.search(line)
    ]
    assert offenders == []


def test_own_pane_helpers_are_only_called_for_tandem_panes() -> None:
    callers = [
        f"{path.relative_to(SRC)}:{n}"
        for path in SRC.rglob("*.py") if path not in ALLOWED_FILES
        for n, line in enumerate(path.read_text().splitlines(), 1)
        if re.search(r"\.(send_keys_own|capture_own)\(", line)
    ]
    # Every call site must be reviewed; today only the status-pane tab switch is allowed.
    assert all(c.startswith("commands.py") for c in callers), callers


def test_no_shell_true_anywhere() -> None:
    hits = [str(p.relative_to(SRC)) for p in SRC.rglob("*.py") if "shell=True" in p.read_text()]
    assert hits == []


def test_no_network_or_token_handling_modules() -> None:
    banned = re.compile(r"^\s*(import|from)\s+(requests|httpx|urllib\.request|socket|http\.client|aiohttp)\b", re.M)
    hits = [str(p.relative_to(SRC)) for p in SRC.rglob("*.py") if banned.search(p.read_text())]
    assert hits == []
