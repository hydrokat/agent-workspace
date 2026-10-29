import json
import os
import re
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from tandem.versions import is_at_least, parse_version

Runner = Callable[[list[str], Path, float], tuple[int, str]]

CHECK_TIMEOUT = 60.0
DEFAULT_INTERVAL = 14400
MIN_INTERVAL = 600

_LINE_RE = re.compile(r"^[A-Z_]+=[-0-9A-Za-z._]*$")
_KNOWN_KEYS = frozenset({"TANDEM_CHECK_LAST", "TANDEM_CHECK_LATEST", "TANDEM_CHECK_SKIP"})


@dataclass(frozen=True)
class CheckState:
    last: float = 0.0
    latest: str = ""
    skip: str = ""


def state_path(workspace: Path) -> Path:
    return workspace / ".agent-workspace" / "update-check.env"


def read_state(workspace: Path) -> CheckState:
    """Parse update-check.env as data; return a fresh state on any error."""
    path = state_path(workspace)
    try:
        text = path.read_text()
    except OSError:
        return CheckState()
    data: dict[str, str] = {}
    for line in text.splitlines():
        if not line.strip():
            continue
        if not _LINE_RE.match(line):
            return CheckState()
        key, value = line.split("=", 1)
        if key not in _KNOWN_KEYS:
            return CheckState()
        data[key] = value
    try:
        return CheckState(
            last=float(data.get("TANDEM_CHECK_LAST", "0")),
            latest=data.get("TANDEM_CHECK_LATEST", ""),
            skip=data.get("TANDEM_CHECK_SKIP", ""),
        )
    except (ValueError, KeyError):
        return CheckState()


def write_state(workspace: Path, state: CheckState) -> None:
    """Atomically write the check state with os.replace."""
    path = state_path(workspace)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".env.tmp")
    tmp.write_text(
        f"TANDEM_CHECK_LAST={int(state.last)}\n"
        f"TANDEM_CHECK_LATEST={state.latest}\n"
        f"TANDEM_CHECK_SKIP={state.skip}\n"
    )
    os.replace(tmp, path)


def due(state: CheckState, now: float, interval: int) -> bool:
    """True when enough time has elapsed since the last successful check."""
    return now - state.last >= interval


def should_offer(installed: str | None, latest: str | None, skip: str) -> bool:
    """True when latest is strictly newer than installed and is not the skipped version."""
    if not installed or not latest:
        return False
    if latest == skip:
        return False
    iv = parse_version(installed)
    lv = parse_version(latest)
    if iv is None or lv is None:
        return False
    return not is_at_least(iv, lv)


def record(workspace: Path, state: CheckState, now: float, latest: str | None) -> CheckState:
    """Persist a completed check: stamp the time and carry through the latest version."""
    new_state = CheckState(
        last=now,
        latest=latest if latest is not None else state.latest,
        skip=state.skip,
    )
    write_state(workspace, new_state)
    return new_state


def latest_version(spec: str, workspace: Path, runner: Runner) -> str | None:
    """Run `npx --yes <spec> --version` and return the parsed version string, or None on any failure."""
    code, out = runner(["npx", "--yes", spec, "--version"], workspace, CHECK_TIMEOUT)
    if code != 0:
        return None
    parsed = parse_version(out)
    return parsed.text if parsed else None


def installed_version_from_manifest(workspace: Path) -> str | None:
    """Read the installed package version from .agent-workspace/manifest.json."""
    manifest = workspace / ".agent-workspace" / "manifest.json"
    try:
        data = json.loads(manifest.read_text())
        v = data.get("version", "")
        return str(v) if v else None
    except (OSError, ValueError, AttributeError, TypeError):
        return None
