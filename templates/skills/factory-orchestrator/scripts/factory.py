#!/usr/bin/env python3
"""Stdlib-only wrapper around `tandem factory` for the Factory Orchestrator."""

from __future__ import annotations

import argparse
import json
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


def add_idea_arg(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--idea",
        dest="idea_override",
        default=None,
        help="Explicit idea ID (i-...) overriding pane and TANDEM_FACTORY_IDEA.",
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="factory.py",
        description="Wrapper for tandem factory ideation commands.",
    )
    parser.add_argument(
        "--idea",
        dest="top_idea",
        default=None,
        help="Explicit idea ID (i-...) overriding pane and TANDEM_FACTORY_IDEA.",
    )
    sub = parser.add_subparsers(dest="command", required=True)

    p_show = sub.add_parser("show", help="Show the current idea and its events.")
    add_idea_arg(p_show)

    p_new = sub.add_parser(
        "new",
        help="Create a new Factory idea in Ideation and bind the current orchestrator.",
    )
    p_new.add_argument("--title", required=True, help="Idea title.")
    p_new.add_argument("--text-file", required=True, help="Path to description file.")
    p_new.add_argument(
        "--no-bind",
        action="store_true",
        help="Do not pass --bind-current when creating the idea.",
    )
    add_idea_arg(p_new)

    p_ensure = sub.add_parser(
        "ensure-idea",
        help="Reuse a bound blank Inbox placeholder via describe, or create and bind a new idea.",
    )
    p_ensure.add_argument("--title", required=True, help="Idea title.")
    p_ensure.add_argument("--text-file", required=True, help="Path to description file.")
    add_idea_arg(p_ensure)

    p_desc = sub.add_parser("describe", help="Set or update idea title and description.")
    p_desc.add_argument("--title", required=True, help="Idea title.")
    p_desc.add_argument("--text-file", required=True, help="Path to description file.")
    add_idea_arg(p_desc)

    p_feas = sub.add_parser("feasibility", help="Store a feasibility study artifact.")
    p_feas.add_argument("--file", required=True, help="Path to feasibility study file.")
    p_feas.add_argument("--verdict", help="Feasibility verdict.")
    p_feas.add_argument("--risk", help="Risk level.")
    add_idea_arg(p_feas)

    p_plan = sub.add_parser("plan", help="Store a plan artifact.")
    p_plan.add_argument("--file", required=True, help="Path to plan file.")
    p_plan.add_argument("--workflow", help="Workflow recommendation.")
    add_idea_arg(p_plan)

    p_assume = sub.add_parser("assume", help="Record an assumption on the idea.")
    p_assume.add_argument("--text", required=True, help="Assumption text.")
    add_idea_arg(p_assume)

    p_qcheck = sub.add_parser(
        "question-check", help="Check whether the open-question ceiling is reached."
    )
    add_idea_arg(p_qcheck)

    p_sel = sub.add_parser("select-direction", help="Advance idea from Ideation to Planning.")
    add_idea_arg(p_sel)

    p_trec = sub.add_parser(
        "target-recommend",
        help="Record an advisory target repository recommendation for an idea.",
    )
    p_trec.add_argument("--repo", required=True, help="Recommended target repository name.")
    p_trec.add_argument(
        "--reason-file",
        required=True,
        help="Path to concise rationale file (max 2 KB, no secrets).",
    )
    add_idea_arg(p_trec)

    p_assign = sub.add_parser("assign", help="Assign a CLI and model to a run stage.")
    p_assign.add_argument("--run", required=True, help="Run ID (r-...).")
    p_assign.add_argument("--stage", required=True, help="Workflow stage name.")
    p_assign.add_argument("--cli", required=True, help="CLI name (claude, agy, opencode).")
    p_assign.add_argument("--model", required=True, help="Model ID.")
    p_assign.add_argument("--role", help="Optional worker role label.")

    p_rec = sub.add_parser(
        "recommend",
        help="Recommend the least-cost capable CLI and model for a run stage.",
    )
    p_rec.add_argument("--run", required=True, help="Run ID (r-...).")
    p_rec.add_argument("--stage", required=True, help="Workflow stage name.")
    p_rec.add_argument("--complexity", choices=("low", "medium", "high"), help="Task complexity hint.")
    p_rec.add_argument(
        "--required-tier",
        choices=("economy", "standard", "premium"),
        help="Minimum model tier.",
    )

    p_agents = sub.add_parser("agents", help="List worker agents for a run.")
    p_agents.add_argument("--run", required=True, help="Run ID (r-...).")

    p_send = sub.add_parser("send", help="Send a message file to a running worker pane.")
    p_send.add_argument("--run", required=True, help="Run ID (r-...).")
    p_send.add_argument("--job", required=True, help="Job ID (<run>.<stage>.<attempt>).")
    p_send.add_argument("--file", required=True, help="Path to message text file.")

    p_cap = sub.add_parser("capture", help="Capture output from a running worker pane.")
    p_cap.add_argument("--run", required=True, help="Run ID (r-...).")
    p_cap.add_argument("--job", required=True, help="Job ID (<run>.<stage>.<attempt>).")
    p_cap.add_argument("--lines", type=int, default=50, help="Lines to capture.")

    p_term = sub.add_parser("terminate", help="Terminate an idle or stuck worker pane.")
    p_term.add_argument("--run", required=True, help="Run ID (r-...).")
    p_term.add_argument("--job", required=True, help="Job ID (<run>.<stage>.<attempt>).")
    p_term.add_argument("--confirm", required=True, help="Must match --job.")

    p_rel = sub.add_parser("relaunch", help="Relaunch a worker pane with current assignments.")
    p_rel.add_argument("--run", required=True, help="Run ID (r-...).")
    p_rel.add_argument("--job", required=True, help="Job ID (<run>.<stage>.<attempt>).")

    return parser


def resolve_idea_id(tandem_bin: str | None, args: argparse.Namespace) -> str:
    explicit = getattr(args, "idea_override", None) or getattr(args, "top_idea", None)
    if explicit and explicit.strip():
        return explicit.strip()

    if os.environ.get("TMUX_PANE", "").strip() and tandem_bin:
        try:
            proc = subprocess.run(
                [tandem_bin, "factory", "idea", "current", "--json"],
                capture_output=True,
                text=True,
            )
            if proc.returncode == 0 and proc.stdout.strip():
                payload = json.loads(proc.stdout)
                if (
                    isinstance(payload, dict)
                    and payload.get("source") == "pane"
                    and isinstance(payload.get("idea"), str)
                    and payload["idea"].strip()
                ):
                    return payload["idea"].strip()
        except (OSError, json.JSONDecodeError):
            pass

    return os.environ.get("TANDEM_FACTORY_IDEA", "").strip()


def is_blank_inbox_idea(show_payload: object) -> bool:
    if not isinstance(show_payload, dict):
        return False
    idea = show_payload.get("idea")
    if not isinstance(idea, dict):
        return False
    if idea.get("state") != "inbox":
        return False
    text = idea.get("text")
    if isinstance(text, str) and text.strip():
        return False
    if idea.get("feasibility") is not None or idea.get("plan") is not None:
        return False
    return True


def is_described_unstarted_inbox_idea(show_payload: object) -> bool:
    if not isinstance(show_payload, dict):
        return False
    idea = show_payload.get("idea")
    if not isinstance(idea, dict):
        return False
    text = idea.get("text")
    return (
        idea.get("state") == "inbox"
        and isinstance(text, str)
        and bool(text.strip())
        and idea.get("feasibility") is None
        and idea.get("plan") is None
    )


def run_ensure_idea(tandem_bin: str, current_idea_id: str, args: argparse.Namespace) -> int:
    if current_idea_id:
        show_proc = subprocess.run(
            [tandem_bin, "factory", "idea", "show", current_idea_id, "--json"],
            capture_output=True,
            text=True,
        )
        if show_proc.returncode == 0 and show_proc.stdout.strip():
            try:
                show_payload = json.loads(show_proc.stdout)
            except json.JSONDecodeError:
                show_payload = None
            if is_described_unstarted_inbox_idea(show_payload):
                print(
                    json.dumps(
                        {
                            "idea_id": current_idea_id,
                            "action": "reused",
                            "kept_description": True,
                            "idea": show_payload["idea"],
                        }
                    )
                )
                return 0
            if is_blank_inbox_idea(show_payload):
                desc_proc = subprocess.run(
                    [
                        tandem_bin,
                        "factory",
                        "idea",
                        "describe",
                        current_idea_id,
                        "--title",
                        args.title,
                        "--text-file",
                        args.text_file,
                        "--json",
                    ],
                    capture_output=True,
                    text=True,
                )
                if desc_proc.returncode != 0:
                    if desc_proc.stderr:
                        sys.stderr.write(desc_proc.stderr)
                    elif desc_proc.stdout:
                        sys.stdout.write(desc_proc.stdout)
                    return desc_proc.returncode
                updated = json.loads(desc_proc.stdout)
                print(
                    json.dumps(
                        {
                            "idea_id": current_idea_id,
                            "action": "reused",
                            "idea": updated,
                        }
                    )
                )
                return 0

    new_proc = subprocess.run(
        [
            tandem_bin,
            "factory",
            "idea",
            "new",
            "--title",
            args.title,
            "--text-file",
            args.text_file,
            "--start-discovery",
            "--bind-current",
            "--json",
        ],
        capture_output=True,
        text=True,
    )
    if new_proc.returncode != 0:
        if new_proc.stderr:
            sys.stderr.write(new_proc.stderr)
        elif new_proc.stdout:
            sys.stdout.write(new_proc.stdout)
        return new_proc.returncode

    created = json.loads(new_proc.stdout)
    new_id = created.get("id", "") if isinstance(created, dict) else ""
    print(
        json.dumps(
            {
                "idea_id": new_id,
                "action": "created",
                "idea": created,
            }
        )
    )
    return 0


def build_argv(tandem_bin: str, idea_id: str, args: argparse.Namespace) -> list[str]:
    cmd = args.command
    if cmd == "new":
        argv = [
            tandem_bin,
            "factory",
            "idea",
            "new",
            "--title",
            args.title,
            "--text-file",
            args.text_file,
            "--start-discovery",
        ]
        if not args.no_bind:
            argv.append("--bind-current")
        argv.append("--json")
        return argv
    if cmd == "assign":
        argv = [
            tandem_bin,
            "factory",
            "agent",
            "assign",
            args.run,
            args.stage,
            "--cli",
            args.cli,
            "--model",
            args.model,
            "--by",
            "orchestrator",
            "--json",
        ]
        if args.role:
            argv.extend(["--role", args.role])
        return argv
    if cmd == "recommend":
        argv = [tandem_bin, "factory", "models", "recommend", "--run", args.run, "--stage", args.stage]
        if args.complexity:
            argv.extend(["--complexity", args.complexity])
        if args.required_tier:
            argv.extend(["--required-tier", args.required_tier])
        argv.append("--json")
        return argv
    if cmd == "agents":
        return [tandem_bin, "factory", "agent", "list", args.run, "--json"]
    if cmd == "send":
        return [
            tandem_bin,
            "factory",
            "agent",
            "send",
            args.run,
            args.job,
            "--file",
            args.file,
            "--json",
        ]
    if cmd == "capture":
        return [
            tandem_bin,
            "factory",
            "agent",
            "capture",
            args.run,
            args.job,
            "--lines",
            str(args.lines),
            "--json",
        ]
    if cmd == "terminate":
        return [
            tandem_bin,
            "factory",
            "agent",
            "terminate",
            args.run,
            args.job,
            "--confirm",
            args.confirm,
            "--json",
        ]
    if cmd == "relaunch":
        return [tandem_bin, "factory", "agent", "relaunch", args.run, args.job, "--json"]
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
    if cmd == "target-recommend":
        return [
            tandem_bin,
            "factory",
            "idea",
            "target-recommend",
            idea_id,
            "--repo",
            args.repo,
            "--reason-file",
            args.reason_file,
            "--json",
        ]
    raise AssertionError(f"Unhandled command: {cmd}")


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    tandem_bin = resolve_tandem_binary()
    no_idea_required = {
        "new",
        "ensure-idea",
        "assign",
        "recommend",
        "agents",
        "send",
        "capture",
        "terminate",
        "relaunch",
    }
    idea_id = resolve_idea_id(tandem_bin, args)
    if args.command not in no_idea_required and not idea_id:
        print("TANDEM_FACTORY_IDEA environment variable is not set.", file=sys.stderr)
        return 2

    if not tandem_bin:
        print("tandem binary not found on PATH or configured locations.", file=sys.stderr)
        return 2

    if args.command == "ensure-idea":
        return run_ensure_idea(tandem_bin, idea_id, args)

    proc = subprocess.run(build_argv(tandem_bin, idea_id, args))
    return proc.returncode


if __name__ == "__main__":
    raise SystemExit(main())
