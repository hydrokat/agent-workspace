import shutil
from collections.abc import Callable

from tandem.adapters import adapter_for
from tandem.adapters.base import AgentSpec, Model
from tandem.wizard.app import WizardApp
from tandem.wizard.flow import CLIS


def installed_clis() -> dict[str, bool]:
    return {cli: shutil.which(cli) is not None for cli in CLIS}


def load_models(cli: str) -> list[Model]:
    return adapter_for(cli).list_models()


def run_launch_wizard(
    installed: dict[str, bool] | None = None, loader: Callable[[str], list[Model]] = load_models
) -> list[AgentSpec] | None:
    """Full-screen startup wizard; returns None when cancelled."""
    return WizardApp(installed or installed_clis(), loader).run()


def run_add_agent(
    installed: dict[str, bool] | None = None, loader: Callable[[str], list[Model]] = load_models
) -> AgentSpec | None:
    """Single-agent variant used inside a tmux popup (F2)."""
    result = WizardApp(installed or installed_clis(), loader, single=True).run()
    return result[0] if result else None
