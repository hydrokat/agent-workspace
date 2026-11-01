import os
import shlex
import sys
from collections.abc import Sequence


def is_frozen() -> bool:
    return bool(getattr(sys, "frozen", False))


def self_argv(python: str | None = None) -> list[str]:
    if is_frozen():
        tandem_self = os.environ.get("TANDEM_SELF")
        if tandem_self and os.path.isfile(tandem_self) and os.access(tandem_self, os.X_OK):
            return [tandem_self]
        return [sys.executable]
    py = python if python is not None else sys.executable
    return [py, "-m", "tandem"]


def self_command(self_cmd: Sequence[str] | str | None = None) -> str:
    if self_cmd is None:
        return shlex.join(self_argv())
    if isinstance(self_cmd, str):
        return self_cmd
    return shlex.join(self_cmd)
