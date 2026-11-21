#!/usr/bin/env python3
"""Deterministic helpers for the /idea skill.

Stdlib only. JSON on stdout, JSON errors on stderr with exit code 1.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from datetime import date
from pathlib import Path
from typing import Any, NoReturn

VERDICTS = ("feasible", "feasible-with-caveats", "needs-spike", "not-feasible")
EFFORTS = ("S", "M", "L", "XL")
MODES = ("classic", "factory")
STATUSES = (
    "planned",
    "assessed",
    "needs-spike",
    "not-feasible",
    "declined",
    "invalid-input",
    "failed",
)


def fail(message: str, code: int = 1) -> NoReturn:
    print(json.dumps({"error": message}, ensure_ascii=False), file=sys.stderr)
    sys.exit(code)


def emit(data: dict[str, Any]) -> None:
    print(json.dumps(data, ensure_ascii=False))


def slugify(text: str) -> str:
    words = re.findall(r"[a-z0-9]+", text.lower())
    slug = "-".join(words)[:60].strip("-")
    return slug or "idea"


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


def detect_mode(force_factory: bool) -> dict[str, str]:
    if force_factory:
        return {"mode": "factory", "reason": "flag"}

    if (
        os.environ.get("TANDEM_FACTORY", "").strip() == "1"
        or os.environ.get("TANDEM_ROLE", "").strip() == "orchestrator"
        or bool(os.environ.get("TANDEM_FACTORY_IDEA", "").strip())
    ):
        return {"mode": "factory", "reason": "env"}

    tandem_bin = resolve_tandem_binary()
    if tandem_bin:
        try:
            proc = subprocess.run(
                [tandem_bin, "factory", "status", "--json"],
                capture_output=True,
                text=True,
                timeout=5,
            )
            if proc.returncode == 0 and proc.stdout.strip():
                payload = json.loads(proc.stdout)
                if isinstance(payload, dict) and payload.get("enabled") is True:
                    return {"mode": "factory", "reason": "status"}
        except (OSError, subprocess.SubprocessError, json.JSONDecodeError):
            pass

    return {"mode": "classic", "reason": "default"}


def map_factory_verdict_and_risk(verdict: str, effort: str, unknowns: int = 0) -> dict[str, str]:
    if verdict == "feasible":
        factory_verdict = "feasible"
    elif verdict == "feasible-with-caveats":
        factory_verdict = "feasible-with-risks"
    else:
        factory_verdict = "infeasible"

    if verdict in ("needs-spike", "not-feasible") or effort == "XL":
        factory_risk = "high"
    elif verdict == "feasible-with-caveats" or effort == "L" or unknowns > 0:
        factory_risk = "medium"
    else:
        factory_risk = "low"

    return {
        "factory_verdict": factory_verdict,
        "factory_risk": factory_risk,
    }


def resolve_report_path(raw_path: str) -> Path:
    feas_root = (Path.cwd() / "specs/feasibility").resolve()
    target = (Path.cwd() / raw_path).resolve()
    if not target.is_relative_to(feas_root):
        fail("report path must be inside specs/feasibility/")
    if not target.is_file():
        fail(f"report not found: {raw_path}")
    return target


def parse_verdict(report_text: str) -> dict[str, Any]:
    match = re.search(r"^<!-- feasibility: (\{.*\}) -->$", report_text, re.MULTILINE)
    if not match:
        raise ValueError("report has no machine-readable feasibility line")
    data: dict[str, Any] = json.loads(match.group(1))
    if data.get("verdict") not in VERDICTS or data.get("effort") not in EFFORTS:
        raise ValueError("unknown verdict or effort")
    return data


def gate_decision(verdict: str, auto: bool) -> dict[str, str]:
    if verdict == "feasible":
        return {"decision": "proceed"}
    if verdict == "feasible-with-caveats":
        return {"decision": "proceed" if auto else "ask", "carry": "caveats"}
    if verdict == "needs-spike":
        return {"decision": "stop", "status": "needs-spike"}
    return {"decision": "stop", "status": "not-feasible"}


def extract_slug_from_report(report: str) -> str:
    stem = Path(report).stem
    stripped = re.sub(r"^\d{4}-\d{2}-\d{2}-", "", stem)
    return stripped or "idea"


def cmd_slug(args: argparse.Namespace) -> None:
    emit({"slug": slugify(args.text), "date": date.today().isoformat()})


def cmd_mode(args: argparse.Namespace) -> None:
    emit(detect_mode(args.factory))


def cmd_factory_map(args: argparse.Namespace) -> None:
    if args.verdict not in VERDICTS:
        fail(f"unknown verdict: {args.verdict}")
    if args.effort not in EFFORTS:
        fail(f"unknown effort: {args.effort}")
    if args.unknowns < 0:
        fail("--unknowns must be >= 0")
    mapped = map_factory_verdict_and_risk(args.verdict, args.effort, args.unknowns)
    emit(
        {
            "verdict": args.verdict,
            "effort": args.effort,
            "unknowns": args.unknowns,
            **mapped,
        }
    )


def cmd_verdict(args: argparse.Namespace) -> None:
    target = resolve_report_path(args.report)
    try:
        data = parse_verdict(target.read_text(encoding="utf-8"))
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        fail(str(exc))
    if args.factory:
        unknowns = int(data.get("unknowns", 0)) if isinstance(data.get("unknowns"), int) else 0
        data.update(map_factory_verdict_and_risk(data["verdict"], data["effort"], unknowns))
    emit(data)


def cmd_gate(args: argparse.Namespace) -> None:
    if args.verdict not in VERDICTS:
        fail(f"unknown verdict: {args.verdict}")
    if args.effort not in EFFORTS:
        fail(f"unknown effort: {args.effort}")
    emit(gate_decision(args.verdict, args.auto))


def cmd_result(args: argparse.Namespace) -> None:
    if args.status not in STATUSES:
        fail(f"unknown status: {args.status}")
    if args.verdict is not None and args.verdict not in VERDICTS:
        fail(f"unknown verdict: {args.verdict}")
    if args.effort is not None and args.effort not in EFFORTS:
        fail(f"unknown effort: {args.effort}")
    if args.mode is not None and args.mode not in MODES:
        fail(f"unknown mode: {args.mode}")
    if args.status == "planned" and not args.phase and not args.idea:
        fail("--phase or --idea is required when status is planned")

    res: dict[str, Any] = {"status": args.status}
    effective_mode = args.mode or ("factory" if args.idea else None)
    if effective_mode is not None:
        res["mode"] = effective_mode
    if args.idea is not None:
        res["idea"] = args.idea
    slug = args.slug or (extract_slug_from_report(args.report) if args.report else None)
    if slug is not None:
        res["slug"] = slug
    if args.verdict is not None:
        res["verdict"] = args.verdict
    if args.effort is not None:
        res["effort"] = args.effort
    if args.report is not None:
        res["report"] = args.report
    if args.phase:
        res["phases"] = args.phase
    if args.status == "planned":
        if args.phase:
            res["next"] = f"/spec-implement {args.phase[0]}"
        elif args.idea:
            res["next"] = f"tandem factory idea ready {args.idea} --confirm {args.idea}"
    if args.error is not None:
        res["error"] = args.error

    emit({"idea_result": res})


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Helpers for /idea")
    sub = parser.add_subparsers(dest="command", required=True)

    p_slug = sub.add_parser("slug")
    p_slug.add_argument("text")
    p_slug.set_defaults(func=cmd_slug)

    p_mode = sub.add_parser("mode")
    p_mode.add_argument("--factory", action="store_true")
    p_mode.set_defaults(func=cmd_mode)

    p_fmap = sub.add_parser("factory-map")
    p_fmap.add_argument("--verdict", required=True)
    p_fmap.add_argument("--effort", required=True)
    p_fmap.add_argument("--unknowns", type=int, default=0)
    p_fmap.set_defaults(func=cmd_factory_map)

    p_verdict = sub.add_parser("verdict")
    p_verdict.add_argument("report")
    p_verdict.add_argument("--factory", action="store_true")
    p_verdict.set_defaults(func=cmd_verdict)

    p_gate = sub.add_parser("gate")
    p_gate.add_argument("--verdict", required=True)
    p_gate.add_argument("--effort", required=True)
    p_gate.add_argument("--auto", action="store_true")
    p_gate.set_defaults(func=cmd_gate)

    p_result = sub.add_parser("result")
    p_result.add_argument("--status", required=True)
    p_result.add_argument("--slug")
    p_result.add_argument("--report")
    p_result.add_argument("--verdict")
    p_result.add_argument("--effort")
    p_result.add_argument("--phase", action="append", default=[])
    p_result.add_argument("--idea")
    p_result.add_argument("--mode")
    p_result.add_argument("--error")
    p_result.set_defaults(func=cmd_result)

    return parser


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
