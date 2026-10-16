import json
import shlex
import sys

from tandem.adapters.base import AgentSpec, Model, session_name, validate_model_id

CLAUDE_ALIASES = ("opus", "sonnet", "fable")


def statusline_settings(python: str = sys.executable) -> str:
    """Inline settings that point Claude Code's documented statusline at tandem's quota feed.

    The feed chains to the user's own statusline command, so nothing they configured disappears.
    """
    command = f"{shlex.quote(python)} -m tandem quota-feed"
    return json.dumps({"statusLine": {"type": "command", "command": command}}, separators=(",", ":"))


class ClaudeAdapter:
    cli = "claude"

    def build_argv(self, spec: AgentSpec) -> list[str]:
        if spec.skip_permissions:
            raise ValueError("claude is always launched with --permission-mode auto")
        return ["claude", "--permission-mode", "auto", "--model", validate_model_id(spec.model),
                "-n", session_name(spec), "--settings", statusline_settings()]

    def list_models(self) -> list[Model]:
        return [Model(alias, f"{alias} (latest)") for alias in CLAUDE_ALIASES]
