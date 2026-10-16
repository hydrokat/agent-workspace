import shutil
import subprocess
import sys
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path

import tandem

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
    lines = [f"1. npx --yes {spec} update --yes --no-link   (refreshes skills, agents, run.sh and tandem)"]
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
