from tandem.adapters.agy import AgyAdapter
from tandem.adapters.base import Adapter, AgentSpec, session_env, session_id, session_name
from tandem.adapters.claude import ClaudeAdapter


def adapter_for(cli: str) -> Adapter:
    """Return the adapter for ``cli``; raises KeyError for unknown CLIs."""
    adapters: dict[str, Adapter] = {"claude": ClaudeAdapter(), "agy": AgyAdapter()}
    return adapters[cli]

__all__ = ["Adapter", "AgentSpec", "adapter_for", "session_env", "session_id", "session_name"]
