import re
import subprocess
from collections.abc import Callable

from tandem.adapters.base import AgentSpec, Model, ModelListError, validate_model_id
from tandem.conversations import valid_id

Runner = Callable[[list[str], float], tuple[int, str]]
_MODEL_LINE = re.compile(r"^([A-Za-z0-9][A-Za-z0-9._:\-]{0,99})\t(.+)$")
_CONTROL = re.compile(r"[\x00-\x08\x0b-\x1f\x7f-\x9f]")


def _subprocess_runner(argv: list[str], timeout: float) -> tuple[int, str]:
    done = subprocess.run(argv, capture_output=True, text=True, timeout=timeout, stdin=subprocess.DEVNULL)
    return done.returncode, done.stdout


def parse_models(output: str) -> list[Model]:
    """Parse ``agy models`` (``id<TAB>label`` per line); anything else is ignored."""
    models: list[Model] = []
    for line in output.splitlines():
        match = _MODEL_LINE.match(line)
        if match:
            models.append(Model(match.group(1), _CONTROL.sub("", match.group(2)).strip()))
    return models


class AgyAdapter:
    cli = "agy"

    def __init__(self, runner: Runner = _subprocess_runner) -> None:
        self._runner = runner

    def build_argv(self, spec: AgentSpec, resume: bool = False, log_file: str | None = None) -> list[str]:
        """``--log-file`` is where tandem later finds the conversation ID; ``--conversation`` reopens it."""
        argv = ["agy", "--model", validate_model_id(spec.model)]
        if spec.skip_permissions:
            argv.append("--dangerously-skip-permissions")
        if log_file is not None:
            if not log_file.startswith("/"):
                raise ValueError("log file must be an absolute path")
            argv += ["--log-file", log_file]
        if resume and spec.conversation is not None:
            conversation = valid_id(spec.conversation)
            if conversation is None:
                raise ValueError("conversation id must be a UUID")
            argv += ["--conversation", conversation]
        return argv

    def list_models(self) -> list[Model]:
        try:
            code, out = self._runner(["agy", "models"], 10.0)
        except (TimeoutError, subprocess.TimeoutExpired, OSError) as exc:
            raise ModelListError("could not list agy models") from exc
        if code != 0:
            raise ModelListError("agy models failed")
        return parse_models(out)
