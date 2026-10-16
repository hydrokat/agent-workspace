import json
import os
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from tandem.adapters.base import AgentSpec, InvalidModelError, Role, validate_model_id

STATE_VERSION = 1
CLIS = ("claude", "agy")
ROLES: tuple[Role, ...] = ("planner", "implementer")
MAX_AGENTS = 40


@dataclass(frozen=True)
class SavedState:
    """What a resumed tandem session rebuilds: the agent lineup, visible tabs, and pane proportions.

    Conversations are not stored: each CLI keeps its own history and offers its own resume.
    """

    agents: tuple[AgentSpec, ...]
    active: dict[str, int] = field(default_factory=dict)
    """Role -> index of the agent tab that was showing."""
    columns_percent: int | None = None
    """Width of the planner column, as a percent of the window (None when the columns were stacked)."""
    status_rows: int | None = None
    saved_at: float = 0.0


def state_path(workspace: Path) -> Path:
    return workspace / ".agent-workspace" / "tandem-session.json"


def to_json(state: SavedState) -> dict[str, Any]:
    return {
        "version": STATE_VERSION,
        "saved_at": state.saved_at,
        "agents": [{"cli": a.cli, "role": a.role, "model": a.model, "index": a.index,
                    "skip_permissions": a.skip_permissions, "cwd": a.cwd} for a in state.agents],
        "active": state.active,
        "layout": {"columns_percent": state.columns_percent, "status_rows": state.status_rows},
    }


def save(path: Path, state: SavedState) -> None:
    """Atomic write, owner-only; a half-written file can never replace a good one."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=".tandem-session-", suffix=".tmp")
    try:
        with os.fdopen(fd, "w") as handle:
            json.dump(to_json(state), handle, indent=2, allow_nan=False)
            handle.write("\n")
        os.chmod(tmp, 0o600)
        os.replace(tmp, path)
    except BaseException:
        Path(tmp).unlink(missing_ok=True)
        raise


def _agent(raw: object) -> AgentSpec | None:
    if not isinstance(raw, dict):
        return None
    cli, role, model, index = raw.get("cli"), raw.get("role"), raw.get("model"), raw.get("index")
    skip, cwd = raw.get("skip_permissions", False), raw.get("cwd")
    if cli not in CLIS or role not in ROLES or not isinstance(model, str):
        return None
    if not isinstance(index, int) or isinstance(index, bool) or not 1 <= index <= 99:
        return None
    if not isinstance(skip, bool) or (cli == "claude" and skip):
        return None
    if cwd is not None and (not isinstance(cwd, str) or not cwd.startswith("/")):
        return None
    try:
        validate_model_id(model)
    except InvalidModelError:
        return None
    return AgentSpec(cli, role, model, index, skip, cwd)


def _percent(raw: object, low: int, high: int) -> int | None:
    return raw if isinstance(raw, int) and not isinstance(raw, bool) and low <= raw <= high else None


def parse(data: object) -> SavedState | None:
    """Validate saved data; unusable entries are dropped, an unusable file yields None."""
    if not isinstance(data, dict) or data.get("version") != STATE_VERSION:
        return None
    raw_agents = data.get("agents")
    if not isinstance(raw_agents, list):
        return None
    seen: set[tuple[str, int]] = set()
    agents: list[AgentSpec] = []
    for raw in raw_agents[:MAX_AGENTS]:
        agent = _agent(raw)
        if agent is not None and (agent.role, agent.index) not in seen:
            seen.add((agent.role, agent.index))
            agents.append(agent)
    if not agents:
        return None
    raw_active = data.get("active")
    active = {r: i for r, i in raw_active.items() if r in ROLES and (r, i) in seen} if isinstance(raw_active, dict) else {}
    raw_layout = data.get("layout")
    layout: dict[str, Any] = raw_layout if isinstance(raw_layout, dict) else {}
    saved_at = data.get("saved_at")
    return SavedState(tuple(agents), active, _percent(layout.get("columns_percent"), 10, 90),
                      _percent(layout.get("status_rows"), 3, 60),
                      float(saved_at) if isinstance(saved_at, (int, float)) and not isinstance(saved_at, bool) else 0.0)


def load(path: Path) -> SavedState | None:
    try:
        return parse(json.loads(path.read_text()))
    except (OSError, ValueError):
        return None


def describe(state: SavedState, now: float | None = None) -> str:
    """One-screen summary shown before the resume/blank choice."""
    lines = []
    when = state.saved_at
    if when:
        minutes = max(0, int(((now if now is not None else time.time()) - when) // 60))
        lines.append(f"Saved session ({minutes} min ago)" if minutes < 120 else f"Saved session ({minutes // 60} h ago)")
    else:
        lines.append("Saved session")
    for role in ROLES:
        mine = [a for a in state.agents if a.role == role]
        if mine:
            lines.append(f"  {role}s: " + ", ".join(f"{a.cli}·{a.model}" for a in sorted(mine, key=lambda a: a.index)))
    return "\n".join(lines)
