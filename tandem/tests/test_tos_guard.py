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


def test_tandem_never_imports_hashlib() -> None:
    """On Pythons built against a limited OpenSSL, importing hashlib prints a traceback into every pane and popup."""
    hits = [str(p.relative_to(SRC)) for p in SRC.rglob("*.py")
            if any(line.strip().startswith(("import hashlib", "from hashlib")) for line in p.read_text().splitlines())]
    assert hits == []


def test_respawn_pane_only_runs_through_the_layout_restart_paths() -> None:
    """Replacing a running agent is allowed only via Layout.restart / restart_live (and tandem's own placeholder)."""
    import ast

    allowed = {"restart", "_respawn", "close_agent", "respawn_own"}
    found: list[str] = []
    for path in SRC.rglob("*.py"):
        tree = ast.parse(path.read_text())
        for func in (n for n in ast.walk(tree) if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef))):
            source = ast.get_source_segment(path.read_text(), func) or ""
            own_lines = [line for line in source.splitlines() if "respawn-pane" in line]
            if own_lines and not (path.name == "layout.py" and func.name in allowed):
                found.append(f"{path.relative_to(SRC)}:{func.name}")
    assert found == []
