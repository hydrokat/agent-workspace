#!/usr/bin/env python3
"""Run a test command and print only what failed.

    run_tests.py [--cwd DIR] [--timeout SEC] [--max-lines N] [--format F] -- <test command...>
    run_tests.py show <test-name> [--log latest|<path>]

The command's own exit code is passed through (124 on timeout, 127 if it cannot be started).
The full output is kept in .agent-workspace/test-runs/ so nothing is lost.
"""
import argparse
import json
import os
import re
import shlex
import signal
import subprocess
import sys
import threading
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

KEEP_LOGS = 20
DEFAULT_MAX_LINES = 120
PER_TEST_LINES = 25
TAIL_LINES = 40
FORMATS = ("auto", "cargo", "pytest", "node", "jest", "vitest", "go", "phpunit", "tap", "generic")

ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]")
CONTROL = re.compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]")
FRAME = re.compile(r"^\s+at ")
INTERNAL_FRAME = re.compile(r"node:internal|node_modules|site-packages")


@dataclass
class Failure:
    name: str
    location: str = ""
    block: list[str] = field(default_factory=list)


@dataclass
class Outcome:
    passed: int | None = None
    failed: int | None = None
    skipped: int | None = None
    failures: list[Failure] = field(default_factory=list)


def clean(text: str) -> str:
    text = ANSI.sub("", text).replace("\r\n", "\n")
    lines = []
    for line in text.split("\n"):
        if "\r" in line:
            segments = [s for s in line.split("\r") if s]
            line = segments[-1] if segments else ""
        lines.append(CONTROL.sub("", line))
    return "\n".join(lines)


def trim_block(lines: list[str]) -> list[str]:
    out: list[str] = []
    frames = 0
    for line in lines:
        if FRAME.match(line):
            if INTERNAL_FRAME.search(line) or frames >= 3:
                continue
            frames += 1
        else:
            frames = 0
        if not line.strip() and (not out or not out[-1].strip()):
            continue
        out.append(line.rstrip())
    while out and not out[-1].strip():
        out.pop()
    return out


def first_match(lines: list[str], pattern: str) -> str:
    rx = re.compile(pattern)
    for line in lines:
        found = rx.search(line)
        if found:
            return found.group(1)
    return ""


def count(text: str, label: str) -> int | None:
    found = re.search(rf"(\d+) {label}", text)
    return int(found.group(1)) if found else None


def parse_pytest(lines: list[str]) -> Outcome:
    out = Outcome()
    summary = ""
    for line in lines:
        if re.search(r"\d+ (failed|passed|skipped|error|errors)\b.* in [\d.]+s", line):
            summary = line
    if summary:
        out.failed = (count(summary, "failed") or 0) + (count(summary, "errors?") or 0)
        out.passed = count(summary, "passed") or 0
        out.skipped = count(summary, "skipped")
    header = re.compile(r"^_{3,} (.+?) _{3,}$")
    current: Failure | None = None
    for line in lines:
        found = header.match(line)
        if found:
            current = Failure(found.group(1))
            out.failures.append(current)
        elif line.startswith("====="):
            current = None
        elif current is not None:
            current.block.append(line)
    for failure in out.failures:
        failure.location = first_match(failure.block, r"^(\S+\.py:\d+):")
    if not out.failures:
        for line in lines:
            if line.startswith(("FAILED ", "ERROR ")):
                name, _, message = line.partition(" - ")
                out.failures.append(Failure(name.split(" ", 1)[1], block=[message] if message else []))
    return out


def parse_cargo(lines: list[str]) -> Outcome:
    out = Outcome()
    passed = failed = skipped = 0
    seen = False
    for line in lines:
        found = re.match(r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored", line)
        if found:
            seen = True
            passed += int(found.group(1))
            failed += int(found.group(2))
            skipped += int(found.group(3))
    if seen:
        out.passed, out.failed, out.skipped = passed, failed, skipped
    current: Failure | None = None
    for line in lines:
        found = re.match(r"^---- (.+?) stdout ----$", line)
        if found:
            current = Failure(found.group(1))
            out.failures.append(current)
        elif line == "failures:" or line.startswith("test result:"):
            current = None
        elif current is not None and "RUST_BACKTRACE" not in line:
            current.block.append(line)
    for failure in out.failures:
        failure.location = first_match(failure.block, r"panicked at (\S+:\d+:\d+)")
    if not out.failures and failed:
        tail = lines[len(lines) - 1 - lines[::-1].index("failures:"):] if "failures:" in lines else []
        for line in tail[1:]:
            if line.startswith("    "):
                out.failures.append(Failure(line.strip()))
    return out


def parse_tap(lines: list[str]) -> Outcome:
    out = Outcome()
    text = "\n".join(lines)
    found = re.search(r"^# pass (\d+)", text, re.M)
    out.passed = int(found.group(1)) if found else None
    found = re.search(r"^# fail (\d+)", text, re.M)
    out.failed = int(found.group(1)) if found else None
    found = re.search(r"^# skipped (\d+)", text, re.M)
    out.skipped = int(found.group(1)) if found else None
    i = 0
    while i < len(lines):
        found = re.match(r"^(\s*)not ok \d+ - (.*?)(?: # .*)?$", lines[i])
        if not found:
            i += 1
            continue
        indent = found.group(1)
        block: list[str] = []
        i += 1
        while i < len(lines) and (lines[i].startswith(indent + "  ") or not lines[i].strip()) and not re.match(rf"^{indent}(not )?ok \d+", lines[i]):
            block.append(lines[i][len(indent):])
            i += 1
            if block[-1].strip() == "...":
                break
        body = "\n".join(block)
        if "subtestsFailed" in body:
            continue
        failure = Failure(found.group(2), block=[b for b in block if b.strip() not in ("---", "...")])
        failure.location = first_match(block, r"location: '?\"?([^'\"\n]+)")
        out.failures.append(failure)
    return out


def parse_node_spec(lines: list[str]) -> Outcome:
    out = Outcome()
    text = "\n".join(lines)
    for key, attr in (("pass", "passed"), ("fail", "failed"), ("skipped", "skipped")):
        found = re.search(rf"^ℹ {key} (\d+)", text, re.M)
        if found:
            setattr(out, attr, int(found.group(1)))
    start = next((i for i, line in enumerate(lines) if "failing tests:" in line), None)
    if start is None:
        return out
    current: Failure | None = None
    location = ""
    for line in lines[start + 1:]:
        found = re.match(r"^test at (.+)$", line)
        if found:
            location = found.group(1)
            current = None
            continue
        found = re.match(r"^✖ (.+?)(?: \([\d.]+ms\))?$", line)
        if found and not line.startswith("✖ failing"):
            current = Failure(found.group(1), location)
            out.failures.append(current)
            continue
        if current is not None:
            current.block.append(line[2:] if line.startswith("  ") else line)
    return out


def parse_node(lines: list[str]) -> Outcome:
    if any("failing tests:" in line for line in lines) or any(line.startswith("ℹ tests") for line in lines):
        return parse_node_spec(lines)
    return parse_tap(lines)


def parse_jest(lines: list[str]) -> Outcome:
    out = Outcome()
    text = "\n".join(lines)
    found = re.search(r"^Tests:\s+(.*)$", text, re.M)
    if found:
        out.failed = count(found.group(1), "failed") or 0
        out.passed = count(found.group(1), "passed") or 0
        out.skipped = count(found.group(1), "skipped")
    seen: set[str] = set()
    current: Failure | None = None
    for line in lines:
        if re.match(r"^(Summary of all failing tests|Test Suites:)", line):
            current = None
            continue
        found = re.match(r"^\s*● (.+)$", line)
        if found and not found.group(1).startswith("Console"):
            name = found.group(1).strip()
            current = None
            if name not in seen:
                seen.add(name)
                current = Failure(name)
                out.failures.append(current)
        elif current is not None:
            current.block.append(line)
    for failure in out.failures:
        failure.location = first_match(failure.block, r"\(([^()\s]+:\d+:\d+)\)")
    return out


def parse_vitest(lines: list[str]) -> Outcome:
    out = Outcome()
    text = "\n".join(lines)
    found = re.search(r"^\s*Tests\s+(.*)$", text, re.M)
    if found:
        out.failed = count(found.group(1), "failed") or 0
        out.passed = count(found.group(1), "passed") or 0
        out.skipped = count(found.group(1), "skipped")
    current: Failure | None = None
    for line in lines:
        found = re.match(r"^\s*FAIL\s+(.+? > .+)$", line)
        if found:
            current = Failure(found.group(1))
            out.failures.append(current)
        elif re.match(r"^⎯{3,}", line) or re.match(r"^\s*(Test Files|Tests)\s", line):
            current = None
        elif current is not None:
            current.block.append(line)
    for failure in out.failures:
        failure.location = first_match(failure.block, r"❯ (\S+:\d+:\d+)")
    return out


def parse_jest_family(lines: list[str]) -> Outcome:
    text = "\n".join(lines)
    if "Test Files" in text or re.search(r"^\s*FAIL\s+\S+ > ", text, re.M):
        return parse_vitest(lines)
    return parse_jest(lines)


def parse_go(lines: list[str]) -> Outcome:
    out = Outcome()
    buffer: list[str] = []
    current: Failure | None = None
    passed = 0
    for line in lines:
        if line.startswith("=== RUN"):
            current = None
            buffer = []
        elif line.startswith("--- PASS"):
            passed += 1
            buffer = []
        elif line.startswith("--- FAIL"):
            found = re.match(r"^--- FAIL: (\S+)", line)
            current = Failure(found.group(1) if found else line, block=list(buffer))
            out.failures.append(current)
            buffer = []
        elif re.match(r"^(FAIL|ok|PASS)(\s|$)", line):
            current = None
        elif current is not None and (line.startswith((" ", "\t", "panic:", "goroutine")) or not line.strip()):
            current.block.append(line)
        elif line.startswith("    ") or line.startswith("\t"):
            buffer.append(line)
    for failure in out.failures:
        failure.location = first_match(failure.block, r"(\S+\.go:\d+)")
    out.failed = len(out.failures)
    out.passed = passed or None
    return out


def parse_phpunit(lines: list[str]) -> Outcome:
    out = Outcome()
    text = "\n".join(lines)
    found = re.search(r"Tests: (\d+), Assertions: \d+(?:, Errors: (\d+))?(?:, Failures: (\d+))?(?:, Skipped: (\d+))?", text)
    if found:
        total = int(found.group(1))
        out.failed = int(found.group(2) or 0) + int(found.group(3) or 0)
        out.skipped = int(found.group(4) or 0) or None
        out.passed = total - out.failed - (out.skipped or 0)
    else:
        found = re.search(r"OK \((\d+) tests?", text)
        if found:
            out.passed, out.failed = int(found.group(1)), 0
    in_section = False
    current: Failure | None = None
    for line in lines:
        if re.match(r"^There (was|were) \d+ (failure|error)s?:", line):
            in_section, current = True, None
            continue
        if re.match(r"^(FAILURES!|ERRORS!|Tests: )", line):
            in_section, current = False, None
            continue
        found = re.match(r"^(\d+)\) (.+)$", line)
        if in_section and found:
            current = Failure(found.group(2))
            out.failures.append(current)
        elif in_section and line == "--":
            current = None
        elif current is not None:
            current.block.append(line)
    for failure in out.failures:
        failure.location = first_match(failure.block, r"(\S+\.php:\d+)")
    return out


FAIL_WORDS = re.compile(r"\b(FAIL|FAILED|FAILURE|ERROR|panicked|AssertionError|Traceback)\b")


def parse_generic(lines: list[str]) -> Outcome:
    out = Outcome()
    i = 0
    while i < len(lines):
        line = lines[i]
        if not FAIL_WORDS.search(line):
            i += 1
            continue
        indent = len(line) - len(line.lstrip())
        name = re.sub(r"^\W*(FAIL(ED|URE)?|ERROR)\b\W*", "", line.strip()) or line.strip()
        failure = Failure(name, block=[line])
        i += 1
        while i < len(lines) and lines[i].strip() and len(lines[i]) - len(lines[i].lstrip()) > indent and len(failure.block) < 6:
            failure.block.append(lines[i])
            i += 1
        out.failures.append(failure)
    out.failed = len(out.failures)
    return out


PARSERS = {
    "cargo": parse_cargo, "pytest": parse_pytest, "node": parse_node, "jest": parse_jest_family,
    "vitest": parse_jest_family, "go": parse_go, "phpunit": parse_phpunit, "tap": parse_tap, "generic": parse_generic,
}


def detect_format(argv: list[str], text: str) -> str:
    names = [os.path.basename(a) for a in argv[:8]]
    joined = " ".join(names)
    if "cargo" in names or "nextest" in names:
        return "cargo"
    if any(n in ("pytest", "py.test") for n in names) or re.search(r"-m pytest", joined):
        return "pytest"
    if "jest" in names:
        return "jest"
    if "vitest" in names:
        return "vitest"
    if names[:1] == ["go"] and "test" in names:
        return "go"
    if "phpunit" in names or "pest" in names or ("artisan" in names and "test" in names):
        return "phpunit"
    if "node" in names and "--test" in names:
        return "node"
    if re.search(r"^test result: ", text, re.M) or re.search(r"^---- .+ stdout ----$", text, re.M):
        return "cargo"
    if re.search(r"^(={3,} .* in [\d.]+s|_{3,} .+ _{3,})", text, re.M) or re.search(r"\d+ (passed|failed).* in [\d.]+s", text):
        return "pytest"
    if "failing tests:" in text or re.search(r"^ℹ tests \d+", text, re.M):
        return "node"
    if re.search(r"^\s*(not )?ok \d+ - ", text, re.M):
        return "tap"
    if "Test Files" in text or re.search(r"^\s*FAIL\s+\S+ > ", text, re.M):
        return "vitest"
    if re.search(r"^\s*● .+", text, re.M) and re.search(r"^Tests:", text, re.M):
        return "jest"
    if re.search(r"^(--- (FAIL|PASS)|=== RUN)", text, re.M):
        return "go"
    if "PHPUnit" in text:
        return "phpunit"
    return "generic"


def parse(fmt: str, argv: list[str], text: str) -> tuple[str, Outcome]:
    chosen = detect_format(argv, text) if fmt == "auto" else fmt
    return chosen, PARSERS[chosen](text.split("\n"))


def find_root(start: Path) -> Path:
    for candidate in (start, *start.parents):
        if (candidate / ".agent-workspace").is_dir():
            return candidate
    return start


def runs_dir(root: Path) -> Path:
    return root / ".agent-workspace" / "test-runs"


def slug_of(argv: list[str]) -> str:
    base = re.sub(r"[^A-Za-z0-9._-]+", "-", os.path.basename(argv[0]) if argv else "run").strip("-")
    return (base or "run")[:40]


def prune_logs(folder: Path) -> None:
    logs = sorted(folder.glob("*.log"), key=lambda p: p.name, reverse=True)
    for old in logs[KEEP_LOGS:]:
        try:
            old.unlink()
        except OSError:
            pass


def open_log(root: Path, argv: list[str]):
    folder = runs_dir(root)
    folder.mkdir(parents=True, exist_ok=True)
    guard = folder / ".gitignore"
    if not guard.exists():
        guard.write_text("*\n")
    stamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H-%M-%SZ")
    path = folder / f"{stamp}-{slug_of(argv)}.log"
    n = 1
    while path.exists():
        n += 1
        path = folder / f"{stamp}-{slug_of(argv)}-{n}.log"
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    return path, os.fdopen(fd, "w", encoding="utf-8", errors="replace")


def display(path: Path, base: Path) -> str:
    try:
        return str(path.relative_to(base))
    except ValueError:
        return str(path)


def kill_group(proc: subprocess.Popen[str]) -> None:
    try:
        os.killpg(proc.pid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError, OSError):
        proc.kill()


def execute(argv: list[str], cwd: Path, timeout: float, heartbeat: float, log) -> tuple[int, bool, float]:
    env = {**os.environ, "NO_COLOR": "1", "FORCE_COLOR": "0", "CARGO_TERM_COLOR": "never", "PYTHONUNBUFFERED": "1"}
    started = time.monotonic()
    try:
        proc = subprocess.Popen(argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, text=True, errors="replace", start_new_session=True)
    except FileNotFoundError:
        log.write(f"command not found: {argv[0]}\n")
        return 127, False, 0.0
    except PermissionError:
        log.write(f"permission denied: {argv[0]}\n")
        return 126, False, 0.0

    def pump() -> None:
        assert proc.stdout is not None
        for line in proc.stdout:
            log.write(CONTROL.sub("", ANSI.sub("", line.replace("\r\n", "\n"))))
            log.flush()

    reader = threading.Thread(target=pump, daemon=True)
    reader.start()
    timed_out = False
    next_beat = started + heartbeat
    while True:
        try:
            proc.wait(timeout=0.2)
            break
        except subprocess.TimeoutExpired:
            now = time.monotonic()
            if timeout and now - started >= timeout:
                timed_out = True
                kill_group(proc)
                proc.wait()
                break
            if heartbeat and now >= next_beat:
                print(f"[run_tests] still running ({int(now - started)}s)...", file=sys.stderr, flush=True)
                next_beat = now + heartbeat
    reader.join(timeout=5)
    return (124 if timed_out else proc.returncode), timed_out, time.monotonic() - started


def format_failure(failure: Failure, limit: int) -> list[str]:
    body = trim_block(failure.block)
    lines = [f"✖ {failure.name}"]
    if failure.location:
        lines.append(f"  {failure.location}")
    shown = body[:limit]
    lines.extend(f"  {line}" for line in shown)
    if len(body) > limit:
        lines.append(f"  … {len(body) - limit} more lines (run: run_tests.py show \"{failure.name}\")")
    return lines


def tail_lines(text: str, n: int) -> list[str]:
    lines = [line for line in text.rstrip("\n").split("\n")]
    return lines[-n:] if lines != [""] else []


def render(outcome: Outcome, code: int, duration: float, text: str, log_ref: str, max_lines: int, timed_out: bool, timeout: float) -> str:
    if code == 0:
        parts = []
        if outcome.passed is not None:
            parts.append(f"{outcome.passed} passed")
        if outcome.skipped:
            parts.append(f"{outcome.skipped} skipped")
        summary = ", ".join(parts) or "no failures reported"
        return f"PASS: {summary} in {duration:.1f}s (exit 0) · log: {log_ref}"

    if timed_out:
        head = f"ERROR: timed out after {timeout:g}s (exit 124)"
    elif code == 127:
        head = "ERROR: command not found (exit 127)"
    elif not outcome.failures:
        head = f"ERROR: the test command failed with exit {code} and no failing tests were recognised; last lines:"
    else:
        head = ""

    if head:
        tail = tail_lines(text, min(max_lines, TAIL_LINES))
        return "\n".join([head, *tail, f"log: {log_ref}"])

    failed = outcome.failed if outcome.failed is not None else len(outcome.failures)
    counts = f"{failed} failed" + (f", {outcome.passed} passed" if outcome.passed is not None else "")
    lines = [f"FAIL: {counts} (exit {code})"]
    budget = max_lines - 2
    for index, failure in enumerate(outcome.failures):
        block = format_failure(failure, min(PER_TEST_LINES, max(3, budget)))
        if len(block) > budget and index > 0:
            hidden = len(outcome.failures) - index
            lines.append(f"… {hidden} more failing test(s) not shown (see log)")
            break
        lines.extend(block[:max(budget, 3)])
        budget -= len(block)
    lines.append(f"log: {log_ref}")
    return "\n".join(lines)


def detect_command(cwd: Path, root: Path) -> list[str] | None:
    candidates = [Path(__file__).resolve().parents[2] / "explain/scripts/locate.py",
                  root / ".agents/skills/explain/scripts/locate.py"]
    locate = next((c for c in candidates if c.is_file()), None)
    if locate is None:
        return None
    try:
        proc = subprocess.run([sys.executable, str(locate), "stack"], cwd=cwd, capture_output=True, text=True, timeout=60)
        data = json.loads(proc.stdout)
    except (subprocess.SubprocessError, json.JSONDecodeError, OSError):
        return None
    runners = [r for repo in data.get("repos", []) for r in repo.get("test_runners", []) if r.get("command")]
    if not runners:
        return None
    others = [r["command"] for r in runners[1:]]
    print(f"[run_tests] no command given; using `{runners[0]['command']}` (detected by locate.py)"
          + (f"; also found: {'; '.join(others)}" if others else ""), file=sys.stderr)
    return shlex.split(runners[0]["command"])


def cmd_run(args: argparse.Namespace, command: list[str]) -> int:
    cwd = Path(args.cwd).resolve()
    if not cwd.is_dir():
        print(f"ERROR: --cwd {args.cwd} is not a directory", file=sys.stderr)
        return 2
    root = find_root(cwd)
    if not command:
        detected = detect_command(cwd, root)
        if not detected:
            print("ERROR: no test command given and none detected. Pass one after `--`, for example: run_tests.py -- cargo test",
                  file=sys.stderr)
            return 2
        command = detected
    path, log = open_log(root, command)
    code, timed_out, duration = execute(command, cwd, args.timeout, args.heartbeat, log)
    log.close()
    text = path.read_text(encoding="utf-8", errors="replace")
    fmt, outcome = parse(args.format, command, text)
    with open(path, "a", encoding="utf-8") as handle:
        handle.write(f"\n# run_tests: format={fmt} exit={code} duration={duration:.1f}s cmd={shlex.join(command)}\n")
    prune_logs(path.parent)
    print(render(outcome, code, duration, text, display(path, cwd), args.max_lines, timed_out, args.timeout))
    return code


def resolve_log(root: Path, choice: str) -> Path | None:
    folder = runs_dir(root)
    if choice == "latest":
        logs = sorted(folder.glob("*.log"), key=lambda p: p.name, reverse=True) if folder.is_dir() else []
        return logs[0] if logs else None
    path = Path(choice).resolve()
    try:
        path.relative_to(root.resolve())
    except ValueError:
        print(f"ERROR: the log must be inside the workspace ({root})", file=sys.stderr)
        raise SystemExit(2)
    return path if path.is_file() else None


def cmd_show(args: argparse.Namespace) -> int:
    root = find_root(Path(args.cwd).resolve())
    log = resolve_log(root, args.log)
    if log is None:
        print("ERROR: no saved test log found; run tests through run_tests.py first", file=sys.stderr)
        return 1
    text = log.read_text(encoding="utf-8", errors="replace")
    meta = re.search(r"^# run_tests: format=(\w+) exit=\S+ duration=\S+ cmd=(.*)$", text, re.M)
    fmt = meta.group(1) if meta else "auto"
    argv = shlex.split(meta.group(2)) if meta else []
    _, outcome = parse(fmt, argv, text)
    wanted = args.name.lower()
    exact = [f for f in outcome.failures if f.name.lower() == wanted]
    matches = exact or [f for f in outcome.failures if wanted in f.name.lower()]
    if not matches:
        names = ", ".join(f.name for f in outcome.failures[:10]) or "none"
        print(f"No failing test matching '{args.name}' in {log.name}. Failing tests: {names}", file=sys.stderr)
        return 1
    for failure in matches:
        print(f"✖ {failure.name}")
        if failure.location:
            print(f"  {failure.location}")
        for line in failure.block:
            print(f"  {line}")
        print()
    return 0


def main(argv: list[str]) -> int:
    if argv and argv[0] == "show":
        parser = argparse.ArgumentParser(prog="run_tests.py show", description="Print one failing test's full block from a saved log")
        parser.add_argument("name")
        parser.add_argument("--log", default="latest")
        parser.add_argument("--cwd", default=".")
        return cmd_show(parser.parse_args(argv[1:]))
    command: list[str] = []
    if "--" in argv:
        split = argv.index("--")
        argv, command = argv[:split], argv[split + 1:]
    parser = argparse.ArgumentParser(prog="run_tests.py", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--cwd", default=".")
    parser.add_argument("--timeout", type=float, default=1800, help="seconds before the run is killed (exit 124); 0 disables")
    parser.add_argument("--max-lines", type=int, default=DEFAULT_MAX_LINES)
    parser.add_argument("--format", choices=FORMATS, default="auto")
    parser.add_argument("--heartbeat", type=float, default=30, help="seconds between progress lines on stderr; 0 disables")
    args = parser.parse_args(argv)
    return cmd_run(args, command)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
