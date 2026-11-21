import os
import stat
import subprocess
import sys
import time
from pathlib import Path

import pytest

SKILL = Path(__file__).resolve().parents[2] / "templates/skills/test-failures/scripts"
WATCH = SKILL / "watch.py"
RUN_TESTS = SKILL / "run_tests.py"
FIXTURES = Path(__file__).resolve().parent / "fixtures/test-failures"

STUB = """#!{python}
import os, sys, time
from pathlib import Path

fixture = Path(os.environ["STUB_FIXTURE"])
cwd = Path.cwd()


def snapshot():
    return sorted((p.name, p.stat().st_mtime_ns) for p in cwd.glob("*.src"))


if os.environ.get("STUB_SILENT") != "1":
    sys.stdout.write(fixture.read_text())
    sys.stdout.flush()
last = snapshot()
while True:
    time.sleep(0.05)
    now = snapshot()
    if now != last:
        last = now
        if os.environ.get("STUB_RERUN") == "0":
            continue
        time.sleep(0.3)
        nxt = cwd / ".next-output"
        sys.stdout.write((nxt if nxt.exists() else fixture).read_text())
        sys.stdout.flush()
"""

ONESHOT = """#!{python}
import os, sys
sys.stdout.write(open(os.environ["STUB_FIXTURE"]).read())
sys.exit(int(os.environ.get("STUB_EXIT", "0")))
"""


def make_bin(path: Path, body: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(body.format(python=sys.executable))
    path.chmod(path.stat().st_mode | stat.S_IEXEC)


@pytest.fixture()
def env(tmp_path: Path) -> dict[str, str]:
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    return {**os.environ, "PATH": str(bin_dir), "NO_COLOR": "1"}


@pytest.fixture()
def ws(tmp_path: Path) -> Path:
    root = tmp_path / "ws"
    (root / ".agent-workspace").mkdir(parents=True)
    return root


def watch(cwd: Path, env: dict[str, str], *args: str, timeout: int = 60) -> subprocess.CompletedProcess[str]:
    return subprocess.run([sys.executable, str(WATCH), *args], capture_output=True, text=True, cwd=cwd, env=env, timeout=timeout)


@pytest.fixture(autouse=True)
def cleanup(ws: Path, env: dict[str, str]):
    yield
    watch(ws, env, "stop", "--all")


def stub(env: dict[str, str], name: str, fixture: str) -> None:
    make_bin(Path(env["PATH"]) / name, STUB)
    env["STUB_FIXTURE"] = str(FIXTURES / fixture)


FAMILIES = [
    ("cargo", {"Cargo.toml": "[package]\nname='x'\n"}, "bacon", "cargo_fail.txt", ["tests::wrong_sum", "math is hard"], ["tests::adds"]),
    ("bun", {"package.json": '{"scripts": {"test": "bun test"}}'}, "bun", "bun_fail.txt", ["applies the discount", "Expected: 91"], ["adds items"]),
    ("node", {"package.json": '{"scripts": {"test": "node --test test/"}}'}, "node", "node_spec_fail.txt", ["compares objects", "kaboom"], ["adds ("]),
    ("vitest", {"package.json": '{"devDependencies": {"vitest": "^2"}}'}, "npx", "vitest_fail.txt", ["applies the discount", "expected 100 to be 90"], ["adds items"]),
    ("jest", {"package.json": '{"scripts": {"test": "jest"}, "devDependencies": {"jest": "^29"}}'}, "npx", "jest_fail.txt", ["rejects negative quantities"], ["format.test.ts"]),
    ("python", {"pyproject.toml": "[tool.pytest.ini_options]\n"}, "ptw", "pytest_fail.txt", ["test_bad_sum", "assert"], ["test_adds"]),
]


@pytest.mark.parametrize("name,files,binary,fixture,present,absent", FAMILIES, ids=[f[0] for f in FAMILIES])
def test_each_runner_family_starts_waits_and_prints_only_failures(
    ws: Path, env: dict[str, str], name: str, files: dict[str, str], binary: str, fixture: str, present: list[str], absent: list[str]
) -> None:
    for filename, content in files.items():
        (ws / filename).write_text(content)
    stub(env, binary, fixture)
    started = watch(ws, env, "start")
    assert started.returncode == 0, started.stdout + started.stderr
    waited = watch(ws, env, "wait", "--timeout", "20", "--quiet", "0.5")
    assert waited.returncode == 0, waited.stdout + waited.stderr
    result = watch(ws, env, "results")
    assert result.returncode == 1, result.stdout + result.stderr
    assert result.stdout.startswith("FAIL:"), result.stdout
    for text in present:
        assert text in result.stdout, f"{text!r} missing from:\n{result.stdout}"
    for text in absent:
        assert text not in result.stdout, f"{text!r} leaked into:\n{result.stdout}"


def test_phpunit_watcher_is_taken_from_vendor_bin(ws: Path, env: dict[str, str]) -> None:
    (ws / "composer.json").write_text('{"require-dev": {"phpunit/phpunit": "^11"}}')
    make_bin(ws / "vendor/bin/phpunit-watcher", STUB)
    env["STUB_FIXTURE"] = str(FIXTURES / "phpunit_fail.txt")
    assert watch(ws, env, "start").returncode == 0
    assert watch(ws, env, "wait", "--timeout", "20", "--quiet", "0.5").returncode == 0
    result = watch(ws, env, "results")
    assert result.returncode == 1
    assert "test_rejects_negative_quantity" in result.stdout


def start_cargo(ws: Path, env: dict[str, str], fixture: str = "cargo_fail.txt") -> None:
    (ws / "Cargo.toml").write_text("[package]\nname='x'\n")
    stub(env, "bacon", fixture)
    started = watch(ws, env, "start")
    assert started.returncode == 0, started.stdout + started.stderr
    assert watch(ws, env, "wait", "--timeout", "20", "--quiet", "0.5").returncode == 0


def test_wait_blocks_until_the_rerun_after_an_edit_finishes(ws: Path, env: dict[str, str]) -> None:
    start_cargo(ws, env)
    assert watch(ws, env, "results").returncode == 1
    (ws / ".next-output").write_text((FIXTURES / "cargo_pass.txt").read_text())
    time.sleep(0.05)
    (ws / "lib.src").write_text("changed")
    waited = watch(ws, env, "wait", "--timeout", "20", "--quiet", "0.5")
    assert waited.returncode == 0, waited.stdout + waited.stderr
    result = watch(ws, env, "results")
    assert result.returncode == 0, result.stdout + result.stderr
    assert result.stdout.startswith("PASS: 2 passed")
    assert "tests::wrong_sum" not in result.stdout, "results must describe only the latest run"


def test_wait_returns_at_once_when_the_latest_run_is_current(ws: Path, env: dict[str, str]) -> None:
    start_cargo(ws, env)
    began = time.monotonic()
    assert watch(ws, env, "wait", "--timeout", "20", "--quiet", "0.5").returncode == 0
    assert time.monotonic() - began < 5


def test_wait_does_not_hang_when_an_edit_triggers_no_rerun(ws: Path, env: dict[str, str]) -> None:
    start_cargo(ws, env)
    env["STUB_RERUN"] = "0"
    stop = watch(ws, env, "stop")
    assert stop.returncode == 0
    assert watch(ws, env, "start").returncode == 0
    assert watch(ws, env, "wait", "--timeout", "20", "--quiet", "0.5").returncode == 0
    (ws / "notes.src").write_text("edited")
    waited = watch(ws, env, "wait", "--timeout", "20", "--quiet", "0.5", "--grace", "1")
    assert waited.returncode == 0, waited.stdout + waited.stderr
    assert "no re-run" in waited.stdout.lower()


def test_wait_times_out_with_exit_124(ws: Path, env: dict[str, str]) -> None:
    (ws / "Cargo.toml").write_text("[package]\nname='x'\n")
    stub(env, "bacon", "cargo_fail.txt")
    env["STUB_SILENT"] = "1"
    assert watch(ws, env, "start").returncode == 0
    waited = watch(ws, env, "wait", "--timeout", "1", "--quiet", "0.2")
    assert waited.returncode == 124, waited.stdout + waited.stderr
    assert "timed out" in waited.stdout.lower()


def test_a_second_watcher_for_the_same_target_is_refused(ws: Path, env: dict[str, str]) -> None:
    start_cargo(ws, env)
    again = watch(ws, env, "start")
    assert again.returncode == 3
    assert "already running" in (again.stdout + again.stderr).lower()


def test_stop_ends_the_watcher_and_status_reports_it(ws: Path, env: dict[str, str]) -> None:
    start_cargo(ws, env)
    assert '"running": true' in watch(ws, env, "status").stdout
    pid = int(next((ws / ".agent-workspace/test-watch").glob("*/pid")).read_text())
    assert watch(ws, env, "stop").returncode == 0
    assert '"running": false' in watch(ws, env, "status").stdout or "no watchers" in watch(ws, env, "status").stdout.lower()
    with pytest.raises(ProcessLookupError):
        for _ in range(50):
            os.kill(pid, 0)
            time.sleep(0.1)
        os.kill(pid, 0)


def test_stop_all_ends_every_watcher(ws: Path, env: dict[str, str]) -> None:
    start_cargo(ws, env)
    assert watch(ws, env, "stop", "--all").returncode == 0
    assert "no watchers" in watch(ws, env, "status", "--all").stdout.lower()


def test_results_and_wait_without_a_watcher_are_usage_errors(ws: Path, env: dict[str, str]) -> None:
    for sub in ("results", "wait"):
        proc = watch(ws, env, sub)
        assert proc.returncode == 3, sub
        assert "no watcher" in (proc.stdout + proc.stderr).lower()


def test_a_custom_command_after_double_dash_is_watched(ws: Path, env: dict[str, str]) -> None:
    env["STUB_FIXTURE"] = str(FIXTURES / "pytest_fail.txt")
    make_bin(ws / "mywatch", STUB)
    assert watch(ws, env, "start", "--", str(ws / "mywatch")).returncode == 0
    assert watch(ws, env, "wait", "--timeout", "20", "--quiet", "0.5").returncode == 0
    assert "test_boom" in watch(ws, env, "results").stdout


def test_missing_watcher_binary_prints_the_install_hint_and_runs_once(ws: Path, env: dict[str, str]) -> None:
    (ws / "Cargo.toml").write_text("[package]\nname='x'\n")
    make_bin(Path(env["PATH"]) / "cargo", ONESHOT)
    env["STUB_FIXTURE"] = str(FIXTURES / "cargo_fail.txt")
    env["STUB_EXIT"] = "101"
    proc = watch(ws, env, "start")
    assert "cargo install --locked bacon" in proc.stderr
    assert proc.stdout.startswith("FAIL:"), proc.stdout + proc.stderr
    assert "tests::wrong_sum" in proc.stdout
    assert proc.returncode == 101
    assert "no watchers" in watch(ws, env, "status", "--all").stdout.lower()


def test_unknown_project_without_a_command_is_a_usage_error(ws: Path, env: dict[str, str]) -> None:
    proc = watch(ws, env, "start")
    assert proc.returncode == 2
    assert "runner" in (proc.stdout + proc.stderr).lower()


def test_the_watcher_log_is_private_and_results_strip_ansi(ws: Path, env: dict[str, str]) -> None:
    ansi = ws / "ansi.txt"
    ansi.write_text("\x1b[31mred\x1b[0m\n" + (FIXTURES / "cargo_fail.txt").read_text())
    (ws / "Cargo.toml").write_text("[package]\nname='x'\n")
    stub(env, "bacon", "cargo_fail.txt")
    env["STUB_FIXTURE"] = str(ansi)
    assert watch(ws, env, "start").returncode == 0
    assert watch(ws, env, "wait", "--timeout", "20", "--quiet", "0.5").returncode == 0
    log = next((ws / ".agent-workspace/test-watch").glob("*/output.log"))
    assert stat.S_IMODE(log.stat().st_mode) == 0o600
    assert "\x1b" not in watch(ws, env, "results").stdout


def test_run_tests_has_a_bun_parser(tmp_path: Path) -> None:
    (tmp_path / ".agent-workspace").mkdir()
    cat = "import sys; sys.stdout.write(open(sys.argv[1]).read()); sys.exit(1)"
    proc = subprocess.run([sys.executable, str(RUN_TESTS), "--", sys.executable, "-c", cat, str(FIXTURES / "bun_fail.txt")],
                          capture_output=True, text=True, cwd=tmp_path)
    assert proc.stdout.startswith("FAIL: 1 failed, 1 passed"), proc.stdout
    assert "applies the discount" in proc.stdout and "Expected: 91" in proc.stdout
    assert "adds items" not in proc.stdout
