from tandem.adapters.base import AgentSpec, Model, session_name, validate_model_id

CLAUDE_ALIASES = ("opus", "sonnet", "fable")


class ClaudeAdapter:
    cli = "claude"

    def build_argv(self, spec: AgentSpec) -> list[str]:
        if spec.skip_permissions:
            raise ValueError("claude is always launched with --permission-mode auto")
        return ["claude", "--permission-mode", "auto", "--model", validate_model_id(spec.model),
                "-n", session_name(spec)]

    def list_models(self) -> list[Model]:
        return [Model(alias, f"{alias} (latest)") for alias in CLAUDE_ALIASES]
