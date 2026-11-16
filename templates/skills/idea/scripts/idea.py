#!/usr/bin/env python3
"""Deterministic helpers for the /idea skill.

Stdlib only. JSON on stdout, JSON errors on stderr with exit code 1.
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from datetime import date
from pathlib import Path
from typing import Any, NoReturn

VERDICTS = ("feasible", "feasible-with-caveats", "needs-spike", "not-feasible")
EFFORTS = ("S", "M", "L", "XL")
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


def cmd_verdict(args: argparse.Namespace) -> None:
    target = resolve_report_path(args.report)
    try:
        data = parse_verdict(target.read_text(encoding="utf-8"))
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        fail(str(exc))
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
    if args.status == "planned" and not args.phase:
        fail("--phase is required when status is planned")

    res: dict[str, Any] = {"status": args.status}
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
        res["next"] = f"/spec-implement {args.phase[0]}"
    if args.error is not None:
        res["error"] = args.error

    emit({"idea_result": res})


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Helpers for /idea")
    sub = parser.add_subparsers(dest="command", required=True)

    p_slug = sub.add_parser("slug")
    p_slug.add_argument("text")
    p_slug.set_defaults(func=cmd_slug)

    p_verdict = sub.add_parser("verdict")
    p_verdict.add_argument("report")
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
    p_result.add_argument("--error")
    p_result.set_defaults(func=cmd_result)

    return parser


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
