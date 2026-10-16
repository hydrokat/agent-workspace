import os
import re
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path


DEFAULT_UPDATE_SPEC = "hydrokat/agent-workspace"
_UPDATE_SPEC = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*/[A-Za-z0-9._-]+(#[A-Za-z0-9._/-]+)?$")


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
        spec = source.get("TANDEM_UPDATE_SPEC", "") or DEFAULT_UPDATE_SPEC
        if not _UPDATE_SPEC.match(spec):
            raise ValueError("TANDEM_UPDATE_SPEC must look like owner/repo or owner/repo#ref")
        return cls(coord_dir=coord_dir, color=not source.get("NO_COLOR", ""),
                   tmux_pane=source.get("TMUX_PANE", ""), workspace=workspace, state_dir=state_dir,
                   update_spec=spec, session_id=session)
