import os
import re
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path


DEFAULT_UPDATE_SPEC = "hydrokat/agent-workspace"
_UPDATE_SPEC = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*/[A-Za-z0-9._-]+(#[A-Za-z0-9._/-]+)?$")
_DEFAULT_UPDATE_INTERVAL = 14400
_MIN_UPDATE_INTERVAL = 600


@dataclass(frozen=True)
class Config:
    """Typed view of the environment; the only place tandem reads env vars."""

    coord_dir: Path | None
    color: bool
    tmux_pane: str = ""
    workspace: Path | None = None
    state_dir: Path = Path.home() / ".local" / "state" / "tandem"
    update_spec: str = DEFAULT_UPDATE_SPEC
    session_id: str = ""
    claude_dir: Path = Path.home() / ".claude"
    """Claude Code's config folder (``CLAUDE_CONFIG_DIR``); tandem only checks whether a transcript exists there."""
    update_interval: int = _DEFAULT_UPDATE_INTERVAL
    """How often (seconds) to check for a newer release while tandem is running."""
    update_check: bool = True
    """False when TANDEM_UPDATE_CHECK=0 or the workspace has no manifest (dev checkout)."""

    @classmethod
    def from_env(cls, env: Mapping[str, str] | None = None) -> "Config":
        source = os.environ if env is None else env
        raw_dir = source.get("TANDEM_COORD_DIR", "")
        coord_dir = Path(raw_dir) if raw_dir else None
        if coord_dir is not None and not coord_dir.is_absolute():
            raise ValueError("TANDEM_COORD_DIR must be an absolute path")
        raw_ws = source.get("TANDEM_WORKSPACE", "")
        workspace = Path(raw_ws) if raw_ws and Path(raw_ws).is_absolute() else None
        xdg = source.get("XDG_STATE_HOME", "")
        state_dir = Path(xdg) / "tandem" if xdg and Path(xdg).is_absolute() else Path.home() / ".local" / "state" / "tandem"
        raw_session = source.get("TANDEM_SESSION_ID", "")
        session = raw_session if re.fullmatch(r"[a-z]+-\d{1,2}", raw_session) else ""
        raw_claude = source.get("CLAUDE_CONFIG_DIR", "")
        claude_dir = Path(raw_claude) if raw_claude and Path(raw_claude).is_absolute() else Path.home() / ".claude"
        spec = source.get("TANDEM_UPDATE_SPEC", "") or DEFAULT_UPDATE_SPEC
        if not _UPDATE_SPEC.match(spec):
            raise ValueError("TANDEM_UPDATE_SPEC must look like owner/repo or owner/repo#ref")
        raw_interval = source.get("TANDEM_UPDATE_INTERVAL", "")
        try:
            interval = max(_MIN_UPDATE_INTERVAL, int(raw_interval)) if raw_interval else _DEFAULT_UPDATE_INTERVAL
        except ValueError:
            interval = _DEFAULT_UPDATE_INTERVAL
        update_check = source.get("TANDEM_UPDATE_CHECK", "") != "0"
        if update_check:
            if workspace is None:
                update_check = False
            else:
                update_check = (workspace / ".agent-workspace" / "manifest.json").exists()
        return cls(coord_dir=coord_dir, color=not source.get("NO_COLOR", ""),
                   tmux_pane=source.get("TMUX_PANE", ""), workspace=workspace, state_dir=state_dir,
                   update_spec=spec, session_id=session, claude_dir=claude_dir,
                   update_interval=interval, update_check=update_check)
