import asyncio
from collections.abc import Callable, Mapping

from textual import on
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.content import Content
from textual.widgets import Input, OptionList, Static
from textual.widgets.option_list import Option

from tandem.adapters.base import AgentSpec, Model, ModelListError
from tandem.wizard.flow import CLIS, ROLES, Step, WizardFlow

ModelLoader = Callable[[str], list[Model]]

HINTS = {
    Step.CLI: "↑↓ move  Enter pick  Esc cancel  ? help",
    Step.ROLE: "↑↓ move  Enter pick  Esc back",
    Step.MODEL: "↑↓ move  Enter pick  / filter or type an ID  Esc back",
    Step.PERMS: "↑↓ move  Enter pick  Esc back",
    Step.CWD: "Type a directory (blank = workspace root)  Enter next  Esc back",
    Step.SUMMARY: "Enter on a row: edit  a add another  d delete  L launch all  Esc back",
}
ROLE_TEXT = {
    "planner": "Planner      — left column; plans and breaks down specs",
    "implementer": "Implementer  — right column; claims tasks, codes in worktrees",
}


class WizardApp(App[list[AgentSpec] | None]):
    """Launch / add-agent wizard. ``single=True`` returns after one agent (used for F2)."""

    CSS = """
    #agents { height: auto; padding: 0 1; }
    #question { padding: 1 1 0 1; text-style: bold; }
    #choices { height: 1fr; }
    #message { height: 1; padding: 0 1; }
    #hints { height: 1; padding: 0 1; text-style: dim; }
    """
    BINDINGS = [
        Binding("escape", "go_back", "Back"),
        Binding("L", "launch", "Launch all"),
        Binding("a", "add", "Add"),
        Binding("d", "delete", "Delete"),
        Binding("slash", "focus_input", "Filter"),
    ]

    def __init__(self, installed: Mapping[str, bool], model_loader: ModelLoader, single: bool = False) -> None:
        super().__init__()
        self.flow = WizardFlow(installed)
        self._loader = model_loader
        self._single = single
        self._models: list[Model] = []
        self._loading = False

    def compose(self) -> ComposeResult:
        yield Static("", id="agents")
        yield Static("", id="question")
        yield OptionList(id="choices")
        yield Input(id="input")
        yield Static("", id="message")
        yield Static("", id="hints")

    def on_mount(self) -> None:
        self.title = "tandem · Launch agents"
        self._render_step()

    def _say(self, text: str) -> None:
        self.query_one("#message", Static).update(text)

    def _render_agents(self) -> None:
        lines = [f"  {i + 1}. {a.cli} · {a.role} · {a.model}" for i, a in enumerate(self.flow.agents)]
        text = "Agents to launch\n" + "\n".join(lines) if lines else "No agents yet"
        self.query_one("#agents", Static).update(text)

    def _render_step(self) -> None:
        step = self.flow.step
        self._render_agents()
        self._say("")
        choices = self.query_one("#choices", OptionList)
        field = self.query_one("#input", Input)
        choices.clear_options()
        field.display = step in (Step.MODEL, Step.CWD)
        field.value = ""
        question = self.query_one("#question", Static)
        self.query_one("#hints", Static).update(HINTS[step])
        if step is Step.CLI:
            question.update("Which AI CLI?")
            for cli in CLIS:
                ok = self.flow.installed.get(cli, False)
                label = cli if ok else f"{cli}  (not installed)"
                choices.add_option(Option(label, id=cli, disabled=not ok))
        elif step is Step.ROLE:
            question.update(f"Role for this {self.flow.draft_cli} agent?")
            for role in ROLES:
                choices.add_option(Option(ROLE_TEXT[role], id=role))
        elif step is Step.MODEL:
            question.update(f"Model for {self.flow.draft_cli} ({self.flow.draft_role})")
            self._start_model_load()
        elif step is Step.PERMS:
            question.update("Warning: run agy with --dangerously-skip-permissions? agy will run tools WITHOUT asking you.")
            choices.add_option(Option("No (default)", id="no"))
            choices.add_option(Option("Yes", id="yes"))
        elif step is Step.CWD:
            question.update("Working directory for this implementer (blank = workspace root)")
            field.placeholder = "/path/to/worktree"
            field.focus()
            return
        else:
            question.update("Ready")
            for i, a in enumerate(self.flow.agents):
                choices.add_option(Option(f"{i + 1}. {a.cli} · {a.role} · {a.model}", id=f"agent-{i}"))
            choices.add_option(Option("Add another agent", id="add"))
            if self.flow.can_launch:
                choices.add_option(Option(f"Launch all ({len(self.flow.agents)})", id="launch"))
        if choices.option_count:
            choices.highlighted = 0
            choices.focus()

    def _start_model_load(self) -> None:
        self._loading = True
        self._say("Loading models…")
        self.run_worker(self._load_models(self.flow.draft_cli), exclusive=True)

    async def _load_models(self, cli: str) -> None:
        try:
            self._models = await asyncio.to_thread(self._loader, cli)
            note = ""
        except (ModelListError, OSError) as exc:
            self._models = []
            note = f"Warning: could not list models ({exc}). Type a model ID and press Enter."
        self._loading = False
        if self.flow.step is not Step.MODEL:
            return
        self._fill_models("")
        self._say(note)

    def _fill_models(self, needle: str) -> None:
        choices = self.query_one("#choices", OptionList)
        choices.clear_options()
        for model in self._models:
            if needle.lower() in model.id.lower() or needle.lower() in model.label.lower():
                choices.add_option(Option(Content(f"{model.id}  {model.label}"), id=model.id))
        if choices.option_count:
            choices.highlighted = 0
            if not self.query_one("#input", Input).has_focus:
                choices.focus()
        else:
            self.query_one("#input", Input).focus()

    @on(Input.Changed, "#input")
    def _filter_changed(self, event: Input.Changed) -> None:
        if self.flow.step is Step.MODEL and not self._loading:
            self._fill_models(event.value)

    @on(Input.Submitted, "#input")
    def _input_submitted(self, event: Input.Submitted) -> None:
        text = event.value.strip()
        try:
            if self.flow.step is Step.MODEL:
                choices = self.query_one("#choices", OptionList)
                if not text and choices.highlighted is not None and choices.option_count:
                    text = str(choices.get_option_at_index(choices.highlighted).id)
                self.flow.choose_model(text)
            elif self.flow.step is Step.CWD:
                self.flow.choose_cwd(text)
            else:
                return
        except ValueError as exc:
            self._say(f"Warning: {exc}")
            return
        self._advance()

    @on(OptionList.OptionSelected, "#choices")
    def _option_selected(self, event: OptionList.OptionSelected) -> None:
        option_id = str(event.option.id)
        step = self.flow.step
        try:
            if step is Step.CLI:
                self.flow.choose_cli(option_id)
            elif step is Step.ROLE:
                self.flow.choose_role(option_id)
            elif step is Step.MODEL:
                self.flow.choose_model(option_id)
            elif step is Step.PERMS:
                self.flow.choose_skip_permissions(option_id == "yes")
            elif option_id == "add":
                self.flow.add_another()
            elif option_id == "launch":
                self.action_launch()
                return
            elif option_id.startswith("agent-"):
                self.flow.edit(int(option_id.split("-", 1)[1]))
        except ValueError as exc:
            self._say(f"Warning: {exc}")
            return
        self._advance()

    def _advance(self) -> None:
        if self._single and self.flow.step is Step.SUMMARY:
            self.exit(list(self.flow.agents))
            return
        self._render_step()

    def action_go_back(self) -> None:
        if self.flow.step is Step.CLI and not self.flow.agents:
            self.exit(None)
            return
        self.flow.back()
        self._render_step()

    def action_launch(self) -> None:
        if self.flow.step is Step.SUMMARY and self.flow.can_launch:
            self.exit(list(self.flow.agents))
        elif not self.flow.can_launch:
            self._say("Warning: add at least one agent before launching.")

    def action_add(self) -> None:
        if self.flow.step is Step.SUMMARY:
            self.flow.add_another()
            self._render_step()

    def action_delete(self) -> None:
        choices = self.query_one("#choices", OptionList)
        if self.flow.step is not Step.SUMMARY or choices.highlighted is None:
            return
        option_id = str(choices.get_option_at_index(choices.highlighted).id)
        if option_id.startswith("agent-"):
            self.flow.delete(int(option_id.split("-", 1)[1]))
            self._render_step()

    def action_focus_input(self) -> None:
        if self.flow.step is Step.MODEL:
            self.query_one("#input", Input).focus()
