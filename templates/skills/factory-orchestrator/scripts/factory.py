#!/usr/bin/env python3
"""Stdlib-only wrapper around `tandem factory` for the Factory Orchestrator."""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
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
    parser = argparse.ArgumentParser(
        prog="factory.py",
        description="Wrapper for tandem factory ideation commands.",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("show", help="Show the current idea and its events.")

    p_desc = sub.add_parser("describe", help="Set or update idea title and description.")
    p_desc.add_argument("--title", required=True, help="Idea title.")
    p_desc.add_argument("--text-file", required=True, help="Path to description file.")

    p_feas = sub.add_parser("feasibility", help="Store a feasibility study artifact.")
    p_feas.add_argument("--file", required=True, help="Path to feasibility study file.")
    p_feas.add_argument("--verdict", help="Feasibility verdict.")
    p_feas.add_argument("--risk", help="Risk level.")

    p_plan = sub.add_parser("plan", help="Store a plan artifact.")
    p_plan.add_argument("--file", required=True, help="Path to plan file.")
    p_plan.add_argument("--workflow", help="Workflow recommendation.")

    p_assume = sub.add_parser("assume", help="Record an assumption on the idea.")
    p_assume.add_argument("--text", required=True, help="Assumption text.")

    sub.add_parser("question-check", help="Check whether the open-question ceiling is reached.")
    sub.add_parser("select-direction", help="Advance idea from Ideation to Planning.")

    return parser


def build_argv(tandem_bin: str, idea_id: str, args: argparse.Namespace) -> list[str]:
    cmd = args.command
    if cmd == "show":
        return [tandem_bin, "factory", "idea", "show", idea_id, "--json"]
    if cmd == "describe":
        return [
            tandem_bin,
            "factory",
            "idea",
            "describe",
            idea_id,
            "--title",
            args.title,
            "--text-file",
            args.text_file,
            "--json",
        ]
    if cmd == "feasibility":
        argv = [
            tandem_bin,
            "factory",
            "artifact",
            "put",
            idea_id,
            "--kind",
            "feasibility",
            "--file",
            args.file,
        ]
        if args.verdict:
            argv.extend(["--verdict", args.verdict])
        if args.risk:
            argv.extend(["--risk", args.risk])
        argv.append("--json")
        return argv
    if cmd == "plan":
        argv = [
            tandem_bin,
            "factory",
            "artifact",
            "put",
            idea_id,
            "--kind",
            "plan",
            "--file",
            args.file,
        ]
        if args.workflow:
            argv.extend(["--workflow", args.workflow])
        argv.append("--json")
        return argv
    if cmd == "assume":
        return [
            tandem_bin,
            "factory",
            "idea",
            "assume",
            idea_id,
            "--text",
            args.text,
            "--json",
        ]
    if cmd == "question-check":
        return [tandem_bin, "factory", "question", "check", idea_id, "--json"]
    if cmd == "select-direction":
        return [tandem_bin, "factory", "idea", "select-direction", idea_id, "--json"]
    raise AssertionError(f"Unhandled command: {cmd}")


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    idea_id = os.environ.get("TANDEM_FACTORY_IDEA", "").strip()
    if not idea_id:
        print("TANDEM_FACTORY_IDEA environment variable is not set.", file=sys.stderr)
        return 2

    tandem_bin = resolve_tandem_binary()
    if not tandem_bin:
        print("tandem binary not found on PATH or configured locations.", file=sys.stderr)
        return 2

    proc = subprocess.run(build_argv(tandem_bin, idea_id, args))
    return proc.returncode


if __name__ == "__main__":
    raise SystemExit(main())
