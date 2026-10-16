from collections.abc import Mapping
from enum import Enum
from pathlib import Path

from tandem.adapters.base import AgentSpec, Role, validate_model_id

CLIS = ("claude", "agy")
ROLES: tuple[Role, ...] = ("planner", "implementer")


class Step(Enum):
    CLI = "cli"
    ROLE = "role"
    MODEL = "model"
    PERMS = "perms"
    CWD = "cwd"
    SUMMARY = "summary"


class WizardFlow:
    """Pure state machine behind the launch / add-agent wizard (no UI)."""

    def __init__(self, installed: Mapping[str, bool]) -> None:
        self.installed = dict(installed)
        self.agents: list[AgentSpec] = []
        self._step = Step.CLI
        self._cli = ""
        self._role: Role = "planner"
        self._model = ""
        self._skip = False

    @property
    def step(self) -> Step:
        return self._step

    @property
    def can_launch(self) -> bool:
        return bool(self.agents)

    @property
    def draft_cli(self) -> str:
        return self._cli

    @property
    def draft_role(self) -> Role:
        return self._role

    def choose_cli(self, cli: str) -> None:
        if cli not in CLIS or not self.installed.get(cli, False):
            raise ValueError(f"{cli} is not installed")
        self._cli = cli
        self._step = Step.ROLE

    def choose_role(self, role: str) -> None:
        if role not in ROLES:
            raise ValueError("role must be planner or implementer")
        self._role = "planner" if role == "planner" else "implementer"
        self._step = Step.MODEL

    def choose_model(self, model: str) -> None:
        self._model = validate_model_id(model)
        self._skip = False
        if self._cli == "agy":
            self._step = Step.PERMS
        else:
            self._after_perms()

    def choose_skip_permissions(self, skip: bool) -> None:
        self._skip = skip
        self._after_perms()

    def choose_cwd(self, text: str) -> None:
        cwd: str | None = None
        if text.strip():
            path = Path(text.strip()).expanduser()
            if not path.is_dir():
                raise ValueError("directory does not exist")
            cwd = str(path)
        self._commit(cwd)

    def _after_perms(self) -> None:
        if self._role == "implementer":
            self._step = Step.CWD
        else:
            self._commit(None)

    def _commit(self, cwd: str | None) -> None:
        index = sum(1 for a in self.agents if a.role == self._role) + 1
        self.agents.append(AgentSpec(self._cli, self._role, self._model, index, self._skip, cwd))
        self._step = Step.SUMMARY

    def add_another(self) -> None:
        self._step = Step.CLI

    def delete(self, position: int) -> None:
        del self.agents[position]
        self._renumber()

    def edit(self, position: int) -> None:
        self.delete(position)
        self._step = Step.CLI

    def _renumber(self) -> None:
        counts: dict[str, int] = {}
        renumbered: list[AgentSpec] = []
        for a in self.agents:
            counts[a.role] = counts.get(a.role, 0) + 1
            renumbered.append(AgentSpec(a.cli, a.role, a.model, counts[a.role], a.skip_permissions, a.cwd))
        self.agents = renumbered

    def back(self) -> None:
        if self._step is Step.CLI and self.agents:
            self._step = Step.SUMMARY
        elif self._step is Step.ROLE:
            self._step = Step.CLI
        elif self._step is Step.MODEL:
            self._step = Step.ROLE
        elif self._step is Step.PERMS:
            self._step = Step.MODEL
        elif self._step is Step.CWD:
            self._step = Step.PERMS if self._cli == "agy" else Step.MODEL
