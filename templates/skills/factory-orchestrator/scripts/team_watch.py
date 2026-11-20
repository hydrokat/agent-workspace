#!/usr/bin/env python3
"""File-first team watcher for Factory Orchestrator.

Reads `tandem factory agent list <run> --json` (which inspects job directories,
`activity.json`, `result.json`, and pane tags) and invokes `agent capture` ONLY
when a live worker pane has missing or stale `activity.json` (or `--force-capture`).
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path


def resolve_tandem_binary() -> str | None:
    for var in ("TANDEM_SELF", "TANDEM_BIN"):
        val = os.environ.get(var, "").strip()
        if val and Path(val).is_file():
            return val

    state_home = os.environ.get("XDG_STATE_HOME", "").strip()
    base_state = Path(state_home) if state_home else Path.home() / ".local" / "state"
    bin_name = "tandem.exe" if sys.platform == "win32" else "tandem"
    candidate = base_state / "tandem" / "bin" / "current" / "tandem" / bin_name
    if candidate.is_file():
        return str(candidate)

    return shutil.which("tandem")


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="team_watch.py",
        description="Monitor active Factory worker jobs file-first with capture fallback.",
    )
    p.add_argument("--run", required=True, help="Run ID (r-...)")
    p.add_argument(
        "--stale-secs",
        type=float,
        default=180.0,
        help="Capture live panes whose activity.json is missing or older than N seconds.",
    )
    p.add_argument(
        "--lines",
        type=int,
        default=40,
        help="Line count for fallback agent capture.",
    )
    p.add_argument(
        "--force-capture",
        action="store_true",
        help="Capture all live worker panes even if activity.json is fresh.",
    )
    return p


def needs_capture(agent: dict, now: float, stale_secs: float, force: bool) -> bool:
    if not agent.get("pane_alive"):
        return False
    if agent.get("has_result"):
        return False
    if force:
        return True
    activity = agent.get("activity")
    if not isinstance(activity, dict):
        return True
    last_stop = activity.get("last_stop_at")
    if not isinstance(last_stop, (int, float)):
        return True
    return (now - float(last_stop)) >= stale_secs


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    tandem_bin = resolve_tandem_binary()
    if not tandem_bin:
        print("tandem binary not found on PATH or configured locations.", file=sys.stderr)
        return 2

    list_proc = subprocess.run(
        [tandem_bin, "factory", "agent", "list", args.run, "--json"],
        capture_output=True,
        text=True,
    )
    if list_proc.returncode != 0:
        sys.stderr.write(list_proc.stderr or "failed to list agents\n")
        return list_proc.returncode

    agents = json.loads(list_proc.stdout or "[]")
    now = time.time()
    captures_used = 0
    enriched = []
    for agent in agents:
        item = dict(agent)
        if needs_capture(item, now, args.stale_secs, args.force_capture):
            cap_proc = subprocess.run(
                [
                    tandem_bin,
                    "factory",
                    "agent",
                    "capture",
                    args.run,
                    item["job_id"],
                    "--lines",
                    str(args.lines),
                    "--json",
                ],
                capture_output=True,
                text=True,
            )
            if cap_proc.returncode == 0 and cap_proc.stdout.strip():
                try:
                    payload = json.loads(cap_proc.stdout)
                    item["capture"] = payload.get("output", cap_proc.stdout)
                except json.JSONDecodeError:
                    item["capture"] = cap_proc.stdout
                captures_used += 1
            else:
                item["capture_error"] = (cap_proc.stderr or "").strip()
        enriched.append(item)

    print(
        json.dumps(
            {
                "run": args.run,
                "captures_used": captures_used,
                "agents": enriched,
            }
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
