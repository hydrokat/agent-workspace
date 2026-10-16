import pytest

from tandem.adapters import adapter_for, session_env, session_id, session_name
from tandem.adapters.agy import AgyAdapter, parse_models
from tandem.adapters.base import AgentSpec, InvalidModelError, ModelListError, validate_model_id
from tandem.adapters.claude import CLAUDE_ALIASES, ClaudeAdapter


def spec(cli: str = "claude", role: str = "planner", model: str = "opus", **kw: object) -> AgentSpec:
    return AgentSpec(cli=cli, role=role, model=model, index=kw.pop("index", 1), **kw)  # type: ignore[arg-type]


def test_claude_argv_snapshot() -> None:
    argv = ClaudeAdapter().build_argv(spec())
    assert argv[:7] == ["claude", "--permission-mode", "auto", "--model", "opus", "-n", "tandem-planner-1"]
    assert argv[7] == "--settings" and len(argv) == 9


def test_claude_settings_only_set_the_statusline_to_the_quota_feed() -> None:
    import json

    settings = json.loads(ClaudeAdapter().build_argv(spec())[8])
    assert list(settings) == ["statusLine"] and settings["statusLine"]["type"] == "command"
    assert settings["statusLine"]["command"].endswith("-m tandem quota-feed")


def test_claude_always_auto_permission_mode() -> None:
    argv = ClaudeAdapter().build_argv(spec(role="implementer", model="claude-sonnet-5-5", index=3))
    assert argv[1:3] == ["--permission-mode", "auto"] and argv[argv.index("-n") + 1] == "tandem-implementer-3"


def test_claude_rejects_skip_permissions() -> None:
    with pytest.raises(ValueError):
        ClaudeAdapter().build_argv(spec(skip_permissions=True))


def test_agy_argv_snapshot() -> None:
    assert AgyAdapter().build_argv(spec("agy", model="gemini-3.8-flash-high")) == [
        "agy", "--model", "gemini-3.8-flash-high"]


def test_agy_skip_permissions_flag_only_when_chosen() -> None:
    argv = AgyAdapter().build_argv(spec("agy", model="m1", skip_permissions=True))
    assert argv == ["agy", "--model", "m1", "--dangerously-skip-permissions"]


@pytest.mark.parametrize("bad", ["", "--evil-flag", "-x", "a;rm -rf /", "a b", "$(id)", "`id`", "a\nb",
                                 "a/b", "x" * 200, "‮evil", "a&b", "a|b"])
def test_model_id_injection_rejected(bad: str) -> None:
    with pytest.raises(InvalidModelError):
        validate_model_id(bad)
    for adapter, cli in ((ClaudeAdapter(), "claude"), (AgyAdapter(), "agy")):
        with pytest.raises(InvalidModelError):
            adapter.build_argv(spec(cli, model=bad))


@pytest.mark.parametrize("good", ["opus", "claude-sonnet-5-5", "gemini-3.8-flash-high", "a.b:c_d-1"])
def test_valid_model_ids(good: str) -> None:
    assert validate_model_id(good) == good


def test_argv_is_a_list_of_plain_strings() -> None:
    argv = ClaudeAdapter().build_argv(spec())
    assert isinstance(argv, list) and all(isinstance(a, str) for a in argv)


def test_session_naming_and_env() -> None:
    s = spec("agy", "implementer", "m1", index=2)
    assert session_id(s) == "implementer-2" and session_name(s) == "tandem-implementer-2"
    env = session_env(s, coord_dir="/w/.agent-workspace/coordination")
    assert env == {"TANDEM_SESSION_ID": "implementer-2", "TANDEM_ROLE": "implementer", "TANDEM_CLI": "agy",
                   "TANDEM_COORD_DIR": "/w/.agent-workspace/coordination"}


def test_env_never_carries_other_variables() -> None:
    env = session_env(spec(), coord_dir="/c")
    assert set(env) == {"TANDEM_SESSION_ID", "TANDEM_ROLE", "TANDEM_CLI", "TANDEM_COORD_DIR"}


def test_adapter_lookup() -> None:
    assert adapter_for("claude").cli == "claude" and adapter_for("agy").cli == "agy"
    with pytest.raises(KeyError):
        adapter_for("gemini")


def test_claude_models_are_aliases() -> None:
    assert [m.id for m in ClaudeAdapter().list_models()] == list(CLAUDE_ALIASES)


AGY_OUT = "Fetching available models...\ngemini-3.8-flash-high\tGemini 3.8 Flash (High)\nclaude-opus-5-5-low\tClaude Opus 5.5 (Low)\n"


def test_parse_models_skips_header() -> None:
    models = parse_models(AGY_OUT)
    assert [m.id for m in models] == ["gemini-3.8-flash-high", "claude-opus-5-5-low"]
    assert models[0].label == "Gemini 3.8 Flash (High)"


def test_parse_models_garbage_yields_nothing() -> None:
    assert parse_models("\x1b[31mBOOM\x1b[0m\n; rm -rf /\t--evil\n\n") == []


def test_list_models_uses_runner_with_timeout() -> None:
    seen: list[tuple[list[str], float]] = []

    def runner(argv: list[str], timeout: float) -> tuple[int, str]:
        seen.append((argv, timeout))
        return 0, AGY_OUT

    assert len(AgyAdapter(runner=runner).list_models()) == 2
    assert seen == [(["agy", "models"], 10.0)]


def test_list_models_failure_raises_model_list_error() -> None:
    def failing(argv: list[str], timeout: float) -> tuple[int, str]:
        return 1, "nope"

    def timing_out(argv: list[str], timeout: float) -> tuple[int, str]:
        raise TimeoutError

    for runner in (failing, timing_out):
        with pytest.raises(ModelListError):
            AgyAdapter(runner=runner).list_models()
