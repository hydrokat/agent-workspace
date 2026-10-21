import json
import shutil
import subprocess
import sys
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path

import tandem
from tandem.tmux.tabs import PaneInfo

Runner = Callable[[list[str], Path, float], tuple[int, str]]
UPDATE_TIMEOUT = 900.0
SYNC_TIMEOUT = 600.0


def run_streaming(argv: list[str], cwd: Path, timeout: float) -> tuple[int, str]:
    """Run an argv list showing its output live (the update popup); stdin is closed so nothing can prompt."""
    try:
        done = subprocess.run(argv, cwd=cwd, timeout=timeout, stdin=subprocess.DEVNULL)
    except subprocess.TimeoutExpired:
        return 124, "timed out"
    except OSError as error:
        return 127, str(error)
    return done.returncode, ""


def run_capturing(argv: list[str], cwd: Path, timeout: float) -> tuple[int, str]:
    try:
        done = subprocess.run(argv, cwd=cwd, timeout=timeout, capture_output=True, text=True,
                              stdin=subprocess.DEVNULL)
    except (OSError, subprocess.SubprocessError):
        return 1, ""
    return done.returncode, done.stdout.strip()


@dataclass
class UpdateResult:
    ok: bool
    message: str
    steps: list[str] = field(default_factory=list)
    reloaded: bool = False
    old_version: str = ""
    new_version: str = ""


def installed_dir(workspace: Path) -> Path:
    return workspace / ".agent-workspace" / "tandem"


def running_from_workspace(workspace: Path) -> bool:
    """True when this process runs the copy that ``agent-workspace update`` replaces."""
    here = Path(tandem.__file__).resolve()
    return installed_dir(workspace).resolve() in here.parents


def _content(path: Path) -> bytes | None:
    try:
        return path.read_bytes()
    except OSError:
        return None


def plan(workspace: Path, spec: str) -> list[str]:
    lines = [f"1. npx --yes {spec} update --yes --no-link   (refreshes skills, agents, run.sh and tandem, and updates the installed AI CLIs: claude update, agy update, codex through its package manager)"]
    if running_from_workspace(workspace):
        lines += ["2. uv sync   (refreshes tandem's Python environment)",
                  "3. restart tandem's own windows (status, placeholders) on the new code"]
    else:
        lines += ["2. tandem is running from a checkout, not the workspace copy: update that with git yourself"]
    lines.append("Your agents keep running. Nothing is typed into their panes.")
    return lines


def run_update(workspace: Path, spec: str, reload: Callable[[], bool], python: str = sys.executable,
               runner: Runner = run_streaming, quiet_runner: Runner = run_capturing,
               which: Callable[[str], str | None] = shutil.which) -> UpdateResult:
    """Update the workspace and tandem without touching running agents; ``reload`` restarts tandem's own panes."""
    result = UpdateResult(False, "", old_version=tandem.__version__)
    if which("npx") is None:
        result.message = "npx not found. Install Node.js (>= 18) to update, or run `npx " + spec + " update` yourself."
        return result
    from_workspace = running_from_workspace(workspace)
    min_file = installed_dir(workspace) / "min-versions.env"
    before = _content(min_file)
    code, note = runner(["npx", "--yes", spec, "update", "--yes", "--no-link"], workspace, UPDATE_TIMEOUT)
    if code != 0:
        result.message = f"agent-workspace update failed (exit {code}{': ' + note if note else ''}). Nothing was restarted."
        return result
    result.steps.append("workspace updated")
    if not from_workspace:
        result.ok = True
        result.message = "Workspace updated. tandem runs from a checkout, so update that copy with git."
        return result
    if which("uv") is None:
        result.message = "Workspace files were updated but uv was not found to refresh tandem. Run ./run.sh to finish."
        return result
    code, note = runner(["uv", "sync", "--project", str(installed_dir(workspace)), "--frozen"], workspace, SYNC_TIMEOUT)
    if code != 0:
        result.message = f"uv sync failed (exit {code}). tandem keeps running the old code; run ./run.sh to repair."
        return result
    result.steps.append("environment synced")
    _, version = quiet_runner([python, "-m", "tandem", "--version"], workspace, 30.0)
    result.new_version = version.removeprefix("tandem ").strip()
    result.reloaded = reload()
    result.steps.append("tandem windows restarted" if result.reloaded else "no running session to restart")
    changed = _content(min_file) != before
    result.ok = True
    result.message = ("Updated " + (f"tandem {result.old_version} → {result.new_version}" if result.new_version else "tandem") + ".")
    if changed:
        result.message += " Minimum dependency versions changed: quit and start with ./run.sh to re-check them."
    return result


@dataclass(frozen=True)
class StaleAgent:
    """A running agent whose CLI was updated after it started, so it still runs the old version."""

    pane_id: str
    role: str
    index: int
    cli: str
    model: str
    launched: str
    installed: str
    resumable: bool
    busy: bool
    claim: str


def _cli_and_model(pane: PaneInfo) -> tuple[str, str]:
    try:
        raw = json.loads(pane.spec)
        return str(raw["cli"]), str(raw["model"])
    except (ValueError, KeyError, TypeError):
        return "", ""


def stale_agents(panes: list[PaneInfo], installed: Callable[[str], str], resumable: Callable[[PaneInfo], bool],
                 busy: dict[str, str]) -> list[StaleAgent]:
    """Agents whose launched version differs from (or was never recorded against) the installed one.

    A CLI that does not answer ``--version`` cannot be judged and is skipped. ``busy`` maps a session id to the
    claim it holds with a fresh heartbeat.
    """
    versions: dict[str, str] = {}
    found: list[StaleAgent] = []
    for pane in panes:
        if pane.kind != "agent":
            continue
        cli, model = _cli_and_model(pane)
        if not cli:
            continue
        if cli not in versions:
            versions[cli] = installed(cli)
        if not versions[cli] or pane.cli_version == versions[cli]:
            continue
        found.append(StaleAgent(pane.id, pane.role, pane.index, cli, model, pane.cli_version, versions[cli],
                                resumable(pane), pane.session in busy, busy.get(pane.session, "")))
    return found


def default_selection(stale: list[StaleAgent]) -> set[str]:
    """Pre-selected: agents that reopen their conversation and are not in the middle of claimed work."""
    return {s.pane_id for s in stale if s.resumable and not s.busy}


def describe_stale(agent: StaleAgent) -> str:
    note = (f"busy (claim {agent.claim})" if agent.busy
            else "resumes conversation" if agent.resumable else "would start a new chat")
    return (f"{agent.role} {agent.index}  {agent.cli}·{agent.model}  {agent.launched or 'unknown'} → "
            f"{agent.installed}  {note}")


def choose_restarts(stale: list[StaleAgent], ask: Callable[[str], str] = input,
                    out: Callable[[str], None] = print) -> list[StaleAgent]:
    """Let the user pick which stale agents to restart. Enter keeps the safe defaults; end of input restarts none."""
    selected = default_selection(stale)
    out("These agents still run an older CLI:")
    if any(s.cli == "agy" for s in stale):
        out("(agy: the update may replace its binary under a running agent, so a restart is advised.)")
    while True:
        for number, agent in enumerate(stale, 1):
            out(f"  [{'x' if agent.pane_id in selected else ' '}] {number} {describe_stale(agent)}")
        out("Restart: a=all  numbers=toggle  Enter=restart selected  n=none")
        try:
            answer = ask("> ").strip().lower()
        except EOFError:
            return []
        if answer == "":
            return [s for s in stale if s.pane_id in selected]
        if answer == "n":
            return []
        for token in answer.replace(",", " ").split():
            if token == "a":
                selected = {s.pane_id for s in stale}
            elif token.isdigit() and 1 <= int(token) <= len(stale):
                pane_id = stale[int(token) - 1].pane_id
                selected ^= {pane_id}
