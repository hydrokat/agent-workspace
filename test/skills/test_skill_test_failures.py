import os
import shutil
import stat
import subprocess
import sys
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parents[2] / "templates/skills/test-failures/scripts/run_tests.py"
FIXTURES = Path(__file__).resolve().parent / "fixtures/test-failures"
CAT = "import sys; sys.stdout.write(open(sys.argv[1]).read()); sys.exit(int(sys.argv[2]))"


def run(cwd: Path, *args: str, timeout: int = 60) -> subprocess.CompletedProcess[str]:
    return subprocess.run([sys.executable, str(SCRIPT), *args], capture_output=True, text=True, cwd=cwd, timeout=timeout)


def run_fixture(cwd: Path, name: str, code: int, *extra: str) -> subprocess.CompletedProcess[str]:
    return run(cwd, *extra, "--", sys.executable, "-c", CAT, str(FIXTURES / name), str(code))


@pytest.fixture()
def ws(tmp_path: Path) -> Path:
    (tmp_path / ".agent-workspace").mkdir()
    return tmp_path


CASES = [
    # fixture, exit code, failing names that must appear, text that must never appear
    ("pytest_fail.txt", 1, ["test_bad_sum", "test_boom", "assert 6 == 7"], ["test_adds", "test_ok_two"]),
    ("cargo_fail.txt", 101, ["tests::wrong_sum", "tests::explodes", "math is hard"], ["tests::adds"]),
    ("node_spec_fail.txt", 1, ["compares objects", "throws", "kaboom"], ["✔ adds", "adds ("]),
    ("node_tap_fail.txt", 1, ["compares objects", "throws"], ["ok 1 - adds"]),
    ("jest_fail.txt", 1, ["applies the discount to the subtotal", "rejects negative quantities"], ["format.test.ts"]),
    ("vitest_fail.txt", 1, ["applies the discount to the subtotal", "expected 100 to be 90"], ["adds items", "format.test.ts"]),
    ("go_fail.txt", 1, ["TestParse", "TestPanics", "want error"], ["TestAdd"]),
    ("phpunit_fail.txt", 1, ["test_rejects_negative_quantity", "test_discount_applies", "Failed asserting"], []),
    ("generic_fail.txt", 1, ["renders table"], ["parses header", "parses footer", "escapes html"]),
]


@pytest.mark.parametrize("fixture,code,present,absent", CASES, ids=[c[0] for c in CASES])
def test_each_runner_reports_only_failures(ws: Path, fixture: str, code: int, present: list[str], absent: list[str]) -> None:
    proc = run_fixture(ws, fixture, code)
    assert proc.returncode == code, proc.stdout + proc.stderr
    assert proc.stdout.startswith("FAIL:"), proc.stdout
    for text in present:
        assert text in proc.stdout, f"{text!r} missing from:\n{proc.stdout}"
    for text in absent:
        assert text not in proc.stdout, f"{text!r} leaked into:\n{proc.stdout}"


def test_explicit_format_overrides_detection(ws: Path) -> None:
    proc = run_fixture(ws, "pytest_fail.txt", 1, "--format", "generic")
    assert proc.returncode == 1
    assert proc.stdout.startswith("FAIL:") or proc.stdout.startswith("ERROR:")


def test_header_counts_failed_and_passed(ws: Path) -> None:
    assert "FAIL: 2 failed, 2 passed (exit 1)" in run_fixture(ws, "pytest_fail.txt", 1).stdout
    assert "FAIL: 2 failed, 1 passed (exit 101)" in run_fixture(ws, "cargo_fail.txt", 101).stdout
    assert "FAIL: 2 failed, 7 passed (exit 1)" in run_fixture(ws, "jest_fail.txt", 1).stdout
    assert "FAIL: 2 failed, 1 passed (exit 1)" in run_fixture(ws, "node_spec_fail.txt", 1).stdout


def test_success_is_one_line_with_the_log_path(ws: Path) -> None:
    proc = run_fixture(ws, "pytest_pass.txt", 0)
    assert proc.returncode == 0
    lines = proc.stdout.strip().splitlines()
    assert len(lines) == 1, proc.stdout
    assert lines[0].startswith("PASS: 2 passed")
    assert "(exit 0)" in lines[0] and "log:" in lines[0]


def test_success_never_prints_test_names(ws: Path) -> None:
    proc = run_fixture(ws, "generic_pass.txt", 0)
    assert proc.returncode == 0
    assert "parses header" not in proc.stdout
    assert proc.stdout.startswith("PASS")


def test_compile_error_prints_error_and_the_tail(ws: Path) -> None:
    proc = run_fixture(ws, "cargo_compile_error.txt", 101)
    assert proc.returncode == 101
    assert proc.stdout.startswith("ERROR:"), proc.stdout
    assert "error" in proc.stdout.lower() and "FAIL:" not in proc.stdout


def test_nonzero_exit_with_nothing_parsed_is_never_hidden(ws: Path) -> None:
    bad = ws / "crash.txt"
    bad.write_text("starting\nsegmentation fault (core dumped)\n")
    proc = run(ws, "--", sys.executable, "-c", CAT, str(bad), "139")
    assert proc.returncode == 139
    assert proc.stdout.startswith("ERROR:")
    assert "segmentation fault" in proc.stdout


def test_exit_code_is_passed_through(ws: Path) -> None:
    for code in (0, 1, 3, 42):
        assert run_fixture(ws, "generic_pass.txt", code).returncode == code


def test_zero_exit_wins_even_if_output_mentions_failures(ws: Path) -> None:
    proc = run_fixture(ws, "generic_fail.txt", 0)
    assert proc.returncode == 0
    assert proc.stdout.startswith("PASS")


def test_timeout_returns_124(ws: Path) -> None:
    proc = run(ws, "--timeout", "1", "--", sys.executable, "-c", "import time; print('start', flush=True); time.sleep(30)")
    assert proc.returncode == 124
    assert proc.stdout.startswith("ERROR:") and "timed out" in proc.stdout
    assert "start" in proc.stdout


def test_missing_command_returns_127(ws: Path) -> None:
    proc = run(ws, "--", "definitely-not-a-test-runner-xyz")
    assert proc.returncode == 127
    assert proc.stdout.startswith("ERROR:") and "not found" in proc.stdout


def test_heartbeat_goes_to_stderr_only(ws: Path) -> None:
    proc = run(ws, "--heartbeat", "1", "--", sys.executable, "-c", "import time; time.sleep(2.6)")
    assert proc.returncode == 0
    assert "still running" in proc.stderr
    assert "still running" not in proc.stdout


def test_log_is_private_ansi_free_and_inside_the_workspace(ws: Path) -> None:
    red = ws / "ansi.txt"
    red.write_text("\x1b[31mred\x1b[0m FAILED thing\x07\n")
    proc = run(ws, "--", sys.executable, "-c", CAT, str(red), "1")
    logs = sorted((ws / ".agent-workspace/test-runs").glob("*.log"))
    assert len(logs) == 1
    mode = stat.S_IMODE(logs[0].stat().st_mode)
    assert mode == 0o600
    text = logs[0].read_text()
    assert "\x1b" not in text and "\x07" not in text
    assert str(logs[0]) in proc.stdout or logs[0].name in proc.stdout


def test_only_the_newest_twenty_logs_are_kept(ws: Path) -> None:
    folder = ws / ".agent-workspace/test-runs"
    folder.mkdir(parents=True)
    for i in range(25):
        old = folder / f"2020-01-01T00-00-{i:02d}Z-old.log"
        old.write_text("old")
    run_fixture(ws, "generic_pass.txt", 0)
    logs = sorted(p.name for p in folder.glob("*.log"))
    assert len(logs) == 20
    assert not any(name.endswith("00Z-old.log") for name in logs), "oldest logs are the ones removed"


def test_output_is_capped_with_a_pointer_to_the_log(ws: Path) -> None:
    big = ws / "big.txt"
    big.write_text("".join(f"  FAIL case {i}: expected {i}\n" for i in range(200)))
    proc = run(ws, "--max-lines", "15", "--", sys.executable, "-c", CAT, str(big), "1")
    assert len(proc.stdout.splitlines()) <= 25
    assert "more" in proc.stdout and "log" in proc.stdout


def test_show_prints_one_tests_full_block(ws: Path) -> None:
    run_fixture(ws, "pytest_fail.txt", 1)
    proc = run(ws, "show", "test_boom")
    assert proc.returncode == 0, proc.stdout + proc.stderr
    assert "ValueError: boom" in proc.stdout
    assert "test_bad_sum" not in proc.stdout


def test_show_reports_an_unknown_test(ws: Path) -> None:
    run_fixture(ws, "pytest_fail.txt", 1)
    proc = run(ws, "show", "no_such_test")
    assert proc.returncode == 1
    assert "no failing test" in (proc.stdout + proc.stderr).lower()


def test_show_without_any_log(ws: Path) -> None:
    proc = run(ws, "show", "anything")
    assert proc.returncode == 1


def test_show_refuses_logs_outside_the_workspace(ws: Path, tmp_path_factory: pytest.TempPathFactory) -> None:
    outside = tmp_path_factory.mktemp("outside") / "x.log"
    outside.write_text("FAILED nothing\n")
    proc = run(ws, "show", "nothing", "--log", str(outside))
    assert proc.returncode == 2
    assert "inside" in (proc.stdout + proc.stderr).lower()


def test_no_command_and_no_detection_is_a_usage_error(ws: Path) -> None:
    proc = run(ws)
    assert proc.returncode == 2
    assert "command" in (proc.stdout + proc.stderr).lower()


@pytest.mark.skipif(shutil.which("git") is None, reason="needs git")
def test_detects_the_test_command_when_none_is_given(tmp_path: Path) -> None:
    (tmp_path / ".agent-workspace").mkdir()
    (tmp_path / "package.json").write_text('{"name": "x", "scripts": {"test": "echo all-good"}}')
    skill = tmp_path / ".agents/skills"
    shutil.copytree(SCRIPT.parents[2] / "explain", skill / "explain")
    env = {**os.environ, "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null"}
    subprocess.run(["git", "init", "-q"], cwd=tmp_path, check=True, env=env)
    subprocess.run(["git", "add", "-f", "-A"], cwd=tmp_path, check=True, env=env)
    proc = run(tmp_path)
    assert proc.returncode == 0, proc.stdout + proc.stderr
    assert "echo all-good" in proc.stderr
    assert proc.stdout.strip().splitlines()[-1].startswith("PASS")
