import re
from dataclasses import dataclass
from typing import Literal, Protocol

Role = Literal["planner", "implementer"]

_MODEL_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:\-]{0,99}$")


class InvalidModelError(ValueError):
    """Model id contains characters that could be misread as a flag or shell syntax."""


class ModelListError(RuntimeError):
    """The CLI could not list its models (timeout, failure, or unusable output)."""


@dataclass(frozen=True)
class Model:
    id: str
    label: str


@dataclass(frozen=True)
class AgentSpec:
    cli: str
    role: Role
    model: str
    index: int = 1
    skip_permissions: bool = False
    cwd: str | None = None
    conversation: str | None = None
    """The CLI's own conversation ID (a UUID), so a resumed session reopens the same chat."""


class Adapter(Protocol):
    cli: str

    def build_argv(self, spec: AgentSpec, resume: bool = False, log_file: str | None = None) -> list[str]: ...

    def list_models(self) -> list[Model]: ...


def validate_model_id(model: str) -> str:
    """Return ``model`` unchanged when it is a plain identifier; never starts with ``-``."""
    if not _MODEL_ID.match(model):
        raise InvalidModelError("model id must be letters, digits, '.', '_', ':' or '-' and start with a letter or digit")
    return model


def session_id(spec: AgentSpec) -> str:
    return f"{spec.role}-{spec.index}"


def session_name(spec: AgentSpec) -> str:
    return f"tandem-{session_id(spec)}"


def session_env(spec: AgentSpec, coord_dir: str) -> dict[str, str]:
    """The only variables tandem adds to an AI pane's environment."""
    return {"TANDEM_SESSION_ID": session_id(spec), "TANDEM_ROLE": spec.role, "TANDEM_CLI": spec.cli,
            "TANDEM_COORD_DIR": coord_dir}
