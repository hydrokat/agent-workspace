#!/usr/bin/env python3
"""Keep one test watcher running per project and read only its failures.

    watch.py start   [--cwd DIR] [--runner R] [-- <watcher command...>]
    watch.py wait    [--cwd DIR] [--timeout SEC] [--quiet SEC] [--grace SEC]
    watch.py results [--cwd DIR] [--max-lines N]
    watch.py status  [--cwd DIR | --all]
    watch.py stop    [--cwd DIR | --all]

Exit codes: 0 ok / tests passed, 1 tests failed, 2 usage error, 3 no watcher running (or one already is),
124 wait timed out. The watcher's output is kept in .agent-workspace/test-watch/<target>/output.log.
"""
import argparse
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import run_tests as rt  # noqa: E402

RUN_TESTS = Path(rt.__file__).resolve()
IGNORED_DIRS = {"target", "node_modules", "__pycache__", "vendor", "dist", "build", "venv"}
MAX_SCANNED_FILES = 20000
POLL = 0.1


@dataclass(frozen=True)
class Runner:
    name: str
    command: tuple[str, ...]
    hint: str
    oneshot: tuple[str, ...]
    fmt: str
    marker: str
    quiet: float = 1.0


RUNNERS = {r.name: r for r in (
    Runner("bacon", ("bacon", "--headless", "-j", "test"), "cargo install --locked bacon", ("cargo", "test"), "cargo",
           r"^test result: |could not compile|^error: test failed|^error: no test target", quiet=3.0),
    Runner("bun", ("bun", "test", "--watch"), "install bun: https://bun.sh", ("bun", "test"), "bun",
           r"^Ran \d+ tests? across"),
    Runner("node", ("node", "--test", "--watch"), "use Node 18+ (node --test --watch)", ("node", "--test"), "node",
           r"^(ℹ|#) duration_ms"),
    Runner("vitest", ("npx", "--no-install", "vitest", "--watch"), "npm install --save-dev vitest",
           ("npx", "--no-install", "vitest", "run"), "vitest", r"^\s*Duration\s+\d"),
    Runner("jest", ("npx", "--no-install", "jest", "--watchAll"), "npm install --save-dev jest",
           ("npx", "--no-install", "jest"), "jest", r"^Time:\s|^Ran all test suites"),
    Runner("phpunit-watcher", ("vendor/bin/phpunit-watcher", "watch"), "composer require --dev spatie/phpunit-watcher",
           ("vendor/bin/phpunit",), "phpunit", r"^(OK \(|OK, but|FAILURES!|ERRORS!|Tests: \d+)"),
    Runner("ptw", ("ptw",), "pip install pytest-watch (or: uv add --dev pytest-watch)", ("python3", "-m", "pytest"), "pytest",
           r"\d+ (passed|failed|error|errors|skipped|deselected)\b.* in [\d.]+s"),
)}
CHOICES = ("auto", *RUNNERS)


def detect_runner(cwd: Path) -> Runner | None:
    if (cwd / "Cargo.toml").is_file():
        return RUNNERS["bacon"]
    package = cwd / "package.json"
    if package.is_file():
        try:
            data = json.loads(package.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            data = {}
        script = str((data.get("scripts") or {}).get("test", ""))
        deps = {**(data.get("dependencies") or {}), **(data.get("devDependencies") or {})}
        if "bun test" in script or any((cwd / lock).is_file() for lock in ("bun.lock", "bun.lockb")):
            return RUNNERS["bun"]
        if "node --test" in script:
            return RUNNERS["node"]
        if "vitest" in script or "vitest" in deps:
            return RUNNERS["vitest"]
        if "jest" in script or "jest" in deps:
            return RUNNERS["jest"]
    if any((cwd / name).is_file() for name in ("composer.json", "phpunit.xml", "phpunit.xml.dist")):
        return RUNNERS["phpunit-watcher"]
    if any((cwd / name).is_file() for name in ("pytest.ini", "pyproject.toml", "setup.cfg", "tox.ini", "conftest.py")):
        return RUNNERS["ptw"]
    return None


def binary_available(argv0: str, cwd: Path) -> bool:
    if "/" in argv0:
        return (cwd / argv0).is_file() and os.access(cwd / argv0, os.X_OK)
    return shutil.which(argv0) is not None


def watch_root(cwd: Path) -> Path:
    return rt.find_root(cwd) / ".agent-workspace" / "test-watch"


def target_of(cwd: Path) -> str:
    root = rt.find_root(cwd)
    try:
        rel = cwd.relative_to(root)
    except ValueError:
        rel = Path(cwd.name)
    return re.sub(r"[^A-Za-z0-9._-]+", "-", "-".join(rel.parts)).strip("-.") or "root"


def folder_of(cwd: Path) -> Path:
    return watch_root(cwd) / target_of(cwd)


def load_meta(folder: Path) -> dict | None:
    try:
        return json.loads((folder / "meta.json").read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None


def save_meta(folder: Path, meta: dict) -> None:
    tmp = folder / "meta.json.tmp"
    tmp.write_text(json.dumps(meta), encoding="utf-8")
    os.replace(tmp, folder / "meta.json")


def alive(pid: object) -> bool:
    if not isinstance(pid, int) or pid <= 1:
        return False
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


def read_log(folder: Path) -> str:
    try:
        return (folder / "output.log").read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""


def log_age(folder: Path) -> float:
    try:
        return time.time() - (folder / "output.log").stat().st_mtime
    except OSError:
        return 0.0


def newest_source_mtime(cwd: Path) -> float:
    newest = 0.0
    seen = 0
    for base, dirs, files in os.walk(cwd):
        dirs[:] = [d for d in dirs if not d.startswith(".") and d not in IGNORED_DIRS]
        for name in files:
            if name.startswith("."):
                continue
            seen += 1
            if seen > MAX_SCANNED_FILES:
                return newest
            try:
                newest = max(newest, os.stat(os.path.join(base, name)).st_mtime)
            except OSError:
                continue
    return newest


def display(path: Path, base: Path) -> str:
    return rt.display(path, base)


def run_once(runner: Runner, cwd: Path) -> int:
    print(f"[watch] `{runner.command[0]}` is not installed. Install it with: {runner.hint}", file=sys.stderr)
    print(f"[watch] running the suite once through run_tests.py instead (`{' '.join(runner.oneshot)}`).", file=sys.stderr)
    sys.stdout.flush()
    return subprocess.run([sys.executable, str(RUN_TESTS), "--cwd", str(cwd), "--", *runner.oneshot]).returncode


def cmd_start(args: argparse.Namespace, command: list[str]) -> int:
    cwd = Path(args.cwd).resolve()
    if not cwd.is_dir():
        print(f"ERROR: --cwd {args.cwd} is not a directory", file=sys.stderr)
        return 2
    folder = folder_of(cwd)
    existing = load_meta(folder)
    if existing and alive(existing["pid"]):
        print(f"ERROR: a watcher is already running for {target_of(cwd)} (pid {existing['pid']}, {existing['runner']}). "
              "Use `watch.py wait` / `results`, or `watch.py stop` first.", file=sys.stderr)
        return 3
    runner = RUNNERS.get(args.runner) if args.runner != "auto" else (None if command else detect_runner(cwd))
    if not command and runner is None:
        print("ERROR: no watch runner detected. Pass --runner "
              f"({', '.join(RUNNERS)}) or a watcher command after `--`, for example: watch.py start -- npm run test:watch",
              file=sys.stderr)
        return 2
    argv = command or list(runner.command)  # type: ignore[union-attr]
    if not binary_available(argv[0], cwd):
        if command or runner is None:
            print(f"ERROR: command not found: {argv[0]}", file=sys.stderr)
            return 127
        return run_once(runner, cwd)
    if "/" in argv[0] and not Path(argv[0]).is_absolute():
        argv = [str(cwd / argv[0]), *argv[1:]]
    shutil.rmtree(folder, ignore_errors=True)
    folder.mkdir(parents=True, exist_ok=True)
    guard = watch_root(cwd) / ".gitignore"
    if not guard.exists():
        guard.write_text("*\n")
    log = folder / "output.log"
    env = {**os.environ, "NO_COLOR": "1", "FORCE_COLOR": "0", "CARGO_TERM_COLOR": "never", "PYTHONUNBUFFERED": "1"}
    fd = os.open(log, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    try:
        proc = subprocess.Popen(argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL, stdout=fd, stderr=subprocess.STDOUT,
                                start_new_session=True)
    except OSError as error:
        print(f"ERROR: could not start {argv[0]}: {error}", file=sys.stderr)
        return 127
    finally:
        os.close(fd)
    (folder / "pid").write_text(str(proc.pid))
    save_meta(folder, {"pid": proc.pid, "runner": runner.name if runner else "custom", "cmd": argv, "cwd": str(cwd),
                       "started": time.time(), "seg_start": 0, "taken": 0, "reported_at": time.time()})
    print(f"started {runner.name if runner else 'custom'} watcher for {target_of(cwd)} (pid {proc.pid}); "
          f"next: watch.py wait, then watch.py results")
    return 0


def marker_regex(meta: dict) -> re.Pattern[str] | None:
    runner = RUNNERS.get(meta["runner"])
    return re.compile(runner.marker, re.M) if runner else None


def load_running(args: argparse.Namespace) -> tuple[Path, Path, dict] | None:
    cwd = Path(args.cwd).resolve()
    folder = folder_of(cwd)
    meta = load_meta(folder)
    if meta is None:
        print(f"ERROR: no watcher for {target_of(cwd)}. Start one: watch.py start", file=sys.stderr)
        return None
    return cwd, folder, meta


def cmd_wait(args: argparse.Namespace) -> int:
    loaded = load_running(args)
    if loaded is None:
        return 3
    cwd, folder, meta = loaded
    runner = RUNNERS.get(meta["runner"])
    quiet = args.quiet if args.quiet is not None else (runner.quiet if runner else 1.0)
    marker = marker_regex(meta)
    deadline = time.monotonic() + args.timeout
    taken = meta["taken"]
    edited = newest_source_mtime(cwd) > meta["reported_at"]

    def fresh() -> str:
        return rt.clean(read_log(folder)[taken:])

    def finished(text: str) -> bool:
        return bool(marker.search(text)) if marker else bool(text.strip())

    def dead() -> int | None:
        if alive(meta["pid"]):
            return None
        print("ERROR: the watcher exited; last lines:")
        print("\n".join(rt.tail_lines(rt.clean(read_log(folder)), 20)))
        return 3

    def complete(note: str, advance: bool) -> int:
        raw = read_log(folder)
        if advance:
            meta["seg_start"] = taken
        meta["taken"] = len(raw)
        meta["reported_at"] = time.time()
        save_meta(folder, meta)
        print(note)
        return 0

    def timed_out() -> int:
        print(f"ERROR: timed out after {args.timeout:g}s waiting for the watcher's run to finish (exit 124)")
        return 124

    text = fresh()
    if not finished(text):
        if not text.strip() and taken > 0:
            if not edited:
                return complete("up to date: no edits since the last report; run: watch.py results", False)
            grace_end = min(deadline, time.monotonic() + args.grace)
            while time.monotonic() < grace_end and not fresh().strip():
                if (code := dead()) is not None:
                    return code
                time.sleep(POLL)
            if not fresh().strip():
                return complete("no re-run was triggered by the latest edit (file not watched, or nothing changed); "
                                "run: watch.py results", False)
        while not finished(fresh()):
            if (code := dead()) is not None:
                return code
            if time.monotonic() >= deadline:
                return timed_out()
            time.sleep(POLL)
    while log_age(folder) < quiet:
        if time.monotonic() >= deadline:
            return timed_out()
        time.sleep(POLL)
    return complete("run finished; run: watch.py results", edited or taken == 0)


def cmd_results(args: argparse.Namespace) -> int:
    loaded = load_running(args)
    if loaded is None:
        return 3
    cwd, folder, meta = loaded
    text = rt.clean(read_log(folder)[meta["seg_start"]:])
    if not text.strip():
        print("WAITING: the watcher has produced no output yet; run: watch.py wait")
        return 3
    runner = RUNNERS.get(meta["runner"])
    fmt, outcome = rt.parse(runner.fmt if runner else "auto", meta["cmd"], text)
    failed = bool(outcome.failures) or (outcome.failed or 0) > 0 or bool(re.search(r"could not compile|error\[E\d+\]", text))
    code = 1 if failed else 0
    out = rt.render(outcome, code, 0.0, text, display(folder / "output.log", cwd), args.max_lines, False, 0)
    first, _, rest = out.partition("\n")
    print(first.replace(" in 0.0s", "") + (f"\n{rest}" if rest else ""))
    return code


def targets(args: argparse.Namespace) -> list[Path]:
    cwd = Path(args.cwd).resolve()
    if args.all:
        base = watch_root(cwd)
        return sorted(p for p in base.iterdir() if p.is_dir()) if base.is_dir() else []
    folder = folder_of(cwd)
    return [folder] if folder.is_dir() else []


def cmd_status(args: argparse.Namespace) -> int:
    found = targets(args)
    if not found:
        print("no watchers")
        return 0
    for folder in found:
        meta = load_meta(folder) or {}
        print(json.dumps({"target": folder.name, "runner": meta.get("runner"), "pid": meta.get("pid"),
                          "running": bool(meta.get("pid")) and alive(meta["pid"]), "cwd": meta.get("cwd"),
                          "command": " ".join(meta.get("cmd", []))}))
    return 0


def terminate(pid: int) -> None:
    for sig, grace in ((signal.SIGTERM, 3.0), (signal.SIGKILL, 3.0)):
        try:
            os.killpg(pid, sig)
        except (ProcessLookupError, PermissionError, OSError):
            try:
                os.kill(pid, sig)
            except OSError:
                return
        end = time.monotonic() + grace
        while time.monotonic() < end:
            if not alive(pid):
                return
            time.sleep(0.05)


def cmd_stop(args: argparse.Namespace) -> int:
    found = targets(args)
    if not found:
        print("no watchers")
        return 0
    for folder in found:
        meta = load_meta(folder)
        if meta and alive(meta["pid"]):
            terminate(meta["pid"])
        shutil.rmtree(folder, ignore_errors=True)
        print(f"stopped {folder.name}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="watch.py", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="action", required=True)

    def common(p: argparse.ArgumentParser) -> None:
        p.add_argument("--cwd", default=".")

    start = sub.add_parser("start", help="start one background watcher for this project")
    common(start)
    start.add_argument("--runner", choices=CHOICES, default="auto")
    wait = sub.add_parser("wait", help="block until the run triggered by the latest edit has finished")
    common(wait)
    wait.add_argument("--timeout", type=float, default=300)
    wait.add_argument("--quiet", type=float, default=None, help="seconds of silence that end a run (default: per runner)")
    wait.add_argument("--grace", type=float, default=10, help="seconds to wait for an edited file to trigger a re-run")
    results = sub.add_parser("results", help="print only the failures of the latest run")
    common(results)
    results.add_argument("--max-lines", type=int, default=rt.DEFAULT_MAX_LINES)
    for name in ("status", "stop"):
        p = sub.add_parser(name)
        common(p)
        p.add_argument("--all", action="store_true")
    return parser


def main(argv: list[str]) -> int:
    command: list[str] = []
    if "--" in argv:
        split = argv.index("--")
        argv, command = argv[:split], argv[split + 1:]
    args = build_parser().parse_args(argv)
    if command and args.action != "start":
        print("ERROR: a command after `--` is only valid for `start`", file=sys.stderr)
        return 2
    handlers = {"wait": cmd_wait, "results": cmd_results, "status": cmd_status, "stop": cmd_stop}
    if args.action == "start":
        return cmd_start(args, command)
    return handlers[args.action](args)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
