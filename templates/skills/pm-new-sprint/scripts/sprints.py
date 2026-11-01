#!/usr/bin/env python3
"""Manage sprint lifecycle, RICE & MoSCoW prioritization, and collision-free work.

Stdlib only. JSON on stdout. Exit codes: 0 ok, 1 conflict/gate/locked, 2 invalid input.
"""
from __future__ import annotations

import argparse
import datetime
import html
import json
import os
import re
import subprocess
import sys
import tempfile
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any, NoReturn

SPRINT_NUM_RE = re.compile(r"^\d{3}$")
SPRINT_DIR_RE = re.compile(r"^s-(\d{3})(?:-([a-z0-9][a-z0-9-]{0,47}))?$")
SLUG_RE = re.compile(r"^[a-z0-9][a-z0-9-]{0,47}$")
PHASE_RE = re.compile(r"^p-\d{3}-[a-z0-9-]{1,48}$")
TASK_RE = re.compile(r"^t-\d{3}$")
FIELD_RE = re.compile(r"\{\{([a-z_]+)\}\}")
PLACEHOLDER_RE = re.compile(r"\[[A-Za-z0-9_\-\s]{3,}\]")
SCRIPT_TAG_RE = re.compile(r"<\s*script\b", re.IGNORECASE)
JAVASCRIPT_URL_RE = re.compile(r"javascript\s*:", re.IGNORECASE)
EVENT_HANDLER_RE = re.compile(r"\bon[a-z]+\s*=", re.IGNORECASE)
EXTERNAL_RESOURCE_RE = re.compile(r"<\s*(?:link|script)[^>]+(?:href|src)\s*=\s*[\"']https?://", re.IGNORECASE)

VALID_STATUSES = ("open", "locked", "closed", "planned")
VALID_MOSCOW = ("Must Have", "Should Have", "Could Have", "Won't Have")
VALID_IMPACT = (3.0, 2.0, 1.0, 0.5, 0.25)
VALID_CONFIDENCE = (1.0, 0.8, 0.5)

HTML_DOCS = (
    "business-requirements-document.html",
    "functional-requirements-document.html",
    "functional-specification-document.html",
)

HOT_FILES = (
    "CHANGELOG.md", "package.json", "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "uv.lock",
    "poetry.lock", "Cargo.lock", "composer.lock", "Gemfile.lock", "go.sum", "test/pack.test.js",
)


def fail(message: str, code: int = 2, **extra: Any) -> NoReturn:
    print(json.dumps({"error": message, **extra}))
    sys.exit(code)


def emit(data: dict[str, Any]) -> None:
    print(json.dumps(data))


def write_atomic(target: Path, content: str) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=target.parent, prefix=".tmp-sprints-")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(content)
        os.replace(tmp, target)
    except Exception:
        if os.path.exists(tmp):
            os.remove(tmp)
        raise


def find_workspace_root(root_arg: str | None) -> Path:
    if root_arg:
        p = Path(root_arg).resolve()
        if not p.is_dir():
            fail(f"specified workspace root does not exist: {root_arg}", 2)
        return p
    here = Path.cwd().resolve()
    for candidate in (here, *here.parents):
        if (candidate / "specs").is_dir() or (candidate / ".agent-workspace").is_dir():
            return candidate
    return here


def find_sprints_dir(root: Path) -> Path:
    sprints = root / "sprints"
    sprints.mkdir(parents=True, exist_ok=True)
    return sprints


def safe_relative_path(target: Path, base: Path) -> bool:
    try:
        target.resolve().relative_to(base.resolve())
        return True
    except ValueError:
        return False


def get_specs_script(root: Path) -> Path | None:
    candidate = root / ".agents/skills/spec-planner/scripts/specs.py"
    if candidate.is_file():
        return candidate
    pkg_candidate = root / "templates/skills/spec-planner/scripts/specs.py"
    if pkg_candidate.is_file():
        return pkg_candidate
    bundled = Path(__file__).resolve().parents[2] / "spec-planner/scripts/specs.py"
    if bundled.is_file():
        return bundled
    return None


def get_coord_script(root: Path) -> Path | None:
    candidate = root / ".agents/skills/agent-coordination/scripts/coord.py"
    if candidate.is_file():
        return candidate
    pkg_candidate = root / "templates/skills/agent-coordination/scripts/coord.py"
    if pkg_candidate.is_file():
        return pkg_candidate
    bundled = Path(__file__).resolve().parents[2] / "agent-coordination/scripts/coord.py"
    if bundled.is_file():
        return bundled
    return None


@dataclass
class PhaseScope:
    phase: str
    title: str
    moscow: str
    reach: float
    impact: float
    confidence: float
    effort: float
    rice_score: float
    spec_link: str


@dataclass
class SprintModel:
    sprint_id: str
    number: str
    slug: str
    title: str
    status: str
    start_date: str
    end_date: str
    capacity_days: float
    committed_effort: float
    phases: list[PhaseScope]
    deliverables: list[str]
    audit_log: list[str]
    path: Path


def parse_sprint_md(sprint_dir: Path) -> SprintModel:
    sprint_file = sprint_dir / "sprint.md"
    if not sprint_file.is_file():
        fail(f"sprint file not found: {sprint_file}", 1)

    content = sprint_file.read_text(encoding="utf-8")
    lines = content.splitlines()

    title = sprint_dir.name
    first_line = lines[0] if lines else ""
    if first_line.startswith("# Sprint "):
        title = first_line.partition(":")[2].strip() or title

    m = SPRINT_DIR_RE.match(sprint_dir.name)
    num = m.group(1) if m else "000"
    slug = m.group(2) if m and m.group(2) else ""

    status = "open"
    start_date = ""
    end_date = ""
    capacity_days = 5.0
    committed_effort = 0.0

    in_audit = False
    in_scope_table = False
    audit_log: list[str] = []
    phases: list[PhaseScope] = []

    for line in lines:
        stripped = line.strip()
        if stripped.startswith("## Metadata"):
            in_audit = False
            in_scope_table = False
            continue
        elif stripped.startswith("## Prioritized Scope") or stripped.startswith("## Scope"):
            in_audit = False
            in_scope_table = True
            continue
        elif stripped.startswith("## Lock & Audit Log") or stripped.startswith("## Audit Log"):
            in_audit = True
            in_scope_table = False
            continue
        elif stripped.startswith("## "):
            in_audit = False
            in_scope_table = False

        if in_audit:
            if stripped.startswith("- "):
                audit_log.append(stripped[2:].strip())
            continue

        if in_scope_table:
            if stripped.startswith("|") and not stripped.startswith("|---") and not stripped.startswith("| Phase"):
                cols = [c.strip() for c in stripped.strip("|").split("|")]
                if len(cols) >= 8:
                    p_name = cols[0]
                    p_title = cols[1]
                    p_moscow = cols[2]
                    try:
                        p_reach = float(cols[3])
                        p_impact = float(cols[4])
                        p_conf = float(cols[5])
                        p_effort = float(cols[6].rstrip("d"))
                        p_rice = float(cols[7])
                    except ValueError:
                        p_reach, p_impact, p_conf, p_effort, p_rice = 100.0, 1.0, 0.8, 1.0, 80.0
                    p_link = cols[8] if len(cols) > 8 else f"../../specs/{p_name}/impl.md"
                    phases.append(PhaseScope(
                        phase=p_name,
                        title=p_title,
                        moscow=p_moscow,
                        reach=p_reach,
                        impact=p_impact,
                        confidence=p_conf,
                        effort=p_effort,
                        rice_score=p_rice,
                        spec_link=p_link,
                    ))
                    if p_moscow != "Won't Have":
                        committed_effort += p_effort
            continue

        if stripped.startswith("- **Status**:"):
            status = stripped.partition(":")[2].strip().lower()
        elif stripped.startswith("- **Start Date**:"):
            start_date = stripped.partition(":")[2].strip()
        elif stripped.startswith("- **End Date**:"):
            end_date = stripped.partition(":")[2].strip()
        elif stripped.startswith("- **AI-Assisted Capacity Limit**:"):
            val = stripped.partition(":")[2].strip().partition(" ")[0].rstrip("d")
            try:
                capacity_days = float(val)
            except ValueError:
                capacity_days = 5.0
        elif stripped.startswith("- **Committed Capacity**:"):
            val = stripped.partition(":")[2].strip().partition(" ")[0].rstrip("d")
            try:
                committed_effort = float(val)
            except ValueError:
                pass

    deliverables = [f for f in HTML_DOCS if (sprint_dir / f).is_file()]

    return SprintModel(
        sprint_id=sprint_dir.name,
        number=num,
        slug=slug,
        title=title,
        status=status,
        start_date=start_date,
        end_date=end_date,
        capacity_days=capacity_days,
        committed_effort=committed_effort,
        phases=phases,
        deliverables=deliverables,
        audit_log=audit_log,
        path=sprint_dir,
    )


def resolve_sprint_dir(sprints_dir: Path, sprint_arg: str | None) -> tuple[Path, str]:
    if not sprint_arg:
        open_or_locked: list[tuple[Path, str]] = []
        for entry in sorted(sprints_dir.iterdir()):
            if entry.is_dir() and SPRINT_DIR_RE.match(entry.name) and (entry / "sprint.md").is_file():
                model = parse_sprint_md(entry)
                if model.status in ("open", "locked"):
                    open_or_locked.append((entry, model.status))
        if not open_or_locked:
            fail("no active sprint (open or locked) found", 1)
        return open_or_locked[-1][0], "active"

    if "/" in sprint_arg or "\\" in sprint_arg or ".." in sprint_arg:
        fail(f"invalid sprint argument: {sprint_arg}", 2)

    target_num: str | None = None
    if SPRINT_NUM_RE.match(sprint_arg):
        target_num = sprint_arg
    elif sprint_arg.isdigit():
        target_num = f"{int(sprint_arg):03d}"
    elif sprint_arg.startswith("s-"):
        rest = sprint_arg[2:]
        if SPRINT_NUM_RE.match(rest):
            target_num = rest
        elif rest.partition("-")[0].isdigit():
            target_num = f"{int(rest.partition('-')[0]):03d}"

    for entry in sorted(sprints_dir.iterdir()):
        if entry.is_dir():
            m = SPRINT_DIR_RE.match(entry.name)
            if m:
                if entry.name == sprint_arg:
                    return entry, "explicit"
                if target_num and m.group(1) == target_num:
                    return entry, "explicit"

    fail(f"sprint not found: {sprint_arg}", 1)


def generate_minimal_html(doc_type: str, title: str, sprint_id: str, status: str, phases: list[PhaseScope]) -> str:
    escaped_title = html.escape(title, quote=True)
    escaped_sprint = html.escape(sprint_id, quote=True)
    escaped_status = html.escape(status.upper(), quote=True)
    escaped_doc = html.escape(doc_type, quote=True)

    rows: list[str] = []
    for p in phases:
        rows.append(
            f"<tr>"
            f"<td>{html.escape(p.phase, quote=True)}</td>"
            f"<td>{html.escape(p.moscow, quote=True)}</td>"
            f"<td>{p.reach}</td>"
            f"<td>{p.impact}</td>"
            f"<td>{p.confidence}</td>"
            f"<td>{p.effort}d</td>"
            f"<td>{p.rice_score:.2f}</td>"
            f"</tr>"
        )
    table_body = "\n".join(rows) if rows else "<tr><td colspan='7'>No phases assigned</td></tr>"

    return f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{escaped_doc} - {escaped_sprint}</title>
  <style>
    body {{ font-family: system-ui, -apple-system, sans-serif; line-height: 1.5; color: #111; max-width: 960px; margin: 2rem auto; padding: 0 1rem; }}
    header {{ border-bottom: 2px solid #ddd; padding-bottom: 1rem; margin-bottom: 2rem; }}
    .badge {{ display: inline-block; padding: 0.25rem 0.5rem; background: #eee; font-weight: bold; border-radius: 4px; font-size: 0.85rem; }}
    table {{ width: 100%; border-collapse: collapse; margin-top: 1rem; }}
    th, td {{ border: 1px solid #ccc; padding: 0.5rem; text-align: left; }}
    th {{ background: #f8f9fa; }}
  </style>
</head>
<body>
  <header>
    <div class="badge">{escaped_doc}</div>
    <span class="badge">Sprint {escaped_sprint} &middot; {escaped_status}</span>
    <h1>{escaped_title}</h1>
  </header>
  <main>
    <section>
      <h2>Prioritized Scope</h2>
      <table>
        <thead>
          <tr>
            <th>Phase</th>
            <th>MoSCoW</th>
            <th>Reach</th>
            <th>Impact</th>
            <th>Confidence</th>
            <th>Effort</th>
            <th>RICE</th>
          </tr>
        </thead>
        <tbody>
          {table_body}
        </tbody>
      </table>
    </section>
  </main>
</body>
</html>
"""


def render_sprint_markdown(
    sprint_id: str,
    title: str,
    status: str,
    start_date: str,
    end_date: str,
    capacity_days: float,
    phases: list[PhaseScope],
    audit_entry: str,
) -> str:
    committed = sum(p.effort for p in phases if p.moscow != "Won't Have")
    overloaded = "Yes" if committed > capacity_days or len(phases) > 3 else "No"

    rows: list[str] = []
    for p in phases:
        rows.append(
            f"| {p.phase} | {p.title} | {p.moscow} | {p.reach:g} | {p.impact:g} | {p.confidence:g} | {p.effort:g}d | {p.rice_score:.2f} | [{p.phase}](../../specs/{p.phase}/impl.md) |"
        )
    scope_table = "\n".join(rows) if rows else "| None | - | - | - | - | - | - | - | - |"

    return f"""# Sprint {sprint_id.partition("-")[0]}: {title}

## Metadata
- **Sprint**: {sprint_id}
- **Status**: {status}
- **Start Date**: {start_date}
- **End Date**: {end_date}
- **Target Window**: 1 Week (7 Days)
- **AI-Assisted Capacity Limit**: {capacity_days:.1f} Days
- **Committed Capacity**: {committed:.1f} Days
- **Total Phases**: {len(phases)}
- **Total Tasks**: {len(phases) * 3}
- **Overloaded**: {overloaded}

## Deliverables
- [Business Requirements Document (BRD)](./business-requirements-document.html)
- [Functional Requirements Document (FRD)](./functional-requirements-document.html)
- [Functional Specification Document (FSD)](./functional-specification-document.html)

## Prioritized Scope (MoSCoW & RICE)

| Phase | Title | MoSCoW | Reach | Impact | Confidence | Effort | RICE Score | Spec Link |
|---|---|---|---|---|---|---|---|---|
{scope_table}

## Mandatory & Auto-Included Items
- None

## Deferred Backlog (Won't Have)
- None

## Lock & Audit Log
- {audit_entry}
"""


def cmd_next_sprint(args: argparse.Namespace) -> None:
    root = find_workspace_root(args.root)
    sprints = find_sprints_dir(root)
    nums: list[int] = []
    for entry in sprints.iterdir():
        if entry.is_dir():
            m = SPRINT_DIR_RE.match(entry.name)
            if m:
                nums.append(int(m.group(1)))
    next_num = max(nums, default=0) + 1
    emit({"next": f"{next_num:03d}"})


def cmd_resolve(args: argparse.Namespace) -> None:
    root = find_workspace_root(args.root)
    sprints = find_sprints_dir(root)
    sprint_dir, selection_type = resolve_sprint_dir(sprints, args.sprint)
    model = parse_sprint_md(sprint_dir)
    emit({
        "sprint": model.sprint_id,
        "number": model.number,
        "slug": model.slug,
        "path": str(sprint_dir.relative_to(root)),
        "status": model.status,
        "selected": selection_type,
    })


def cmd_create(args: argparse.Namespace) -> None:
    root = find_workspace_root(args.root)
    sprints = find_sprints_dir(root)

    title = args.title.strip()
    if not title:
        fail("sprint title must not be empty", 2)

    slug = args.slug.strip() if args.slug else ""
    if not slug:
        slug = re.sub(r"[^a-z0-9]+", "-", title.lower()).strip("-")[:48]
    if not SLUG_RE.match(slug):
        fail(f"invalid slug: {slug}", 2)

    status = (args.status or "open").strip().lower()
    if status not in VALID_STATUSES:
        fail(f"invalid status: {status}", 2)

    if status in ("open", "locked"):
        for entry in sprints.iterdir():
            if entry.is_dir() and SPRINT_DIR_RE.match(entry.name) and (entry / "sprint.md").is_file():
                existing = parse_sprint_md(entry)
                if existing.status in ("open", "locked"):
                    fail(
                        f"an active sprint already exists ({existing.sprint_id}), pass --status planned to queue an upcoming sprint",
                        1,
                    )

    nums: list[int] = []
    for entry in sprints.iterdir():
        if entry.is_dir():
            m = SPRINT_DIR_RE.match(entry.name)
            if m:
                nums.append(int(m.group(1)))
    next_num = max(nums, default=0) + 1
    sprint_id = f"s-{next_num:03d}-{slug}"
    sprint_dir = sprints / sprint_id
    if sprint_dir.exists():
        fail(f"sprint directory already exists: {sprint_id}", 1)

    today = datetime.date.today()
    start_date = args.start or today.isoformat()
    end_date = args.end or (today + datetime.timedelta(days=7)).isoformat()
    capacity_days = float(args.capacity_days) if args.capacity_days is not None else 5.0

    phases: list[PhaseScope] = []
    if args.phase:
        moscow = args.moscow or "Must Have"
        if moscow not in VALID_MOSCOW:
            fail(f"invalid MoSCoW priority: {moscow}", 2)

        reach = float(args.reach) if args.reach is not None else 100.0
        if reach <= 0:
            fail("Reach must be greater than 0", 2)

        impact = float(args.impact) if args.impact is not None else 1.0
        if impact not in VALID_IMPACT:
            fail(f"Impact must be one of {VALID_IMPACT}", 2)

        confidence = float(args.confidence) if args.confidence is not None else 0.8
        if confidence not in VALID_CONFIDENCE:
            fail(f"Confidence must be one of {VALID_CONFIDENCE}", 2)

        effort = float(args.effort) if args.effort is not None else 1.0
        if effort <= 0:
            fail("Effort must be greater than 0", 2)

        score = round((reach * impact * confidence) / effort, 2)
        phases.append(PhaseScope(
            phase=args.phase,
            title=args.phase,
            moscow=moscow,
            reach=reach,
            impact=impact,
            confidence=confidence,
            effort=effort,
            rice_score=score,
            spec_link=f"../../specs/{args.phase}/impl.md",
        ))

    sprint_dir.mkdir(parents=True, exist_ok=True)
    audit_entry = f"{datetime.datetime.now().strftime('%Y-%m-%d %H:%M:%S')}: Sprint created with status {status}"

    assets_dir = Path(__file__).resolve().parents[1] / "assets"
    sprint_template = assets_dir / "sprint.md"
    if sprint_template.is_file():
        text = sprint_template.read_text(encoding="utf-8")
        text = text.replace("{{sprint_id}}", sprint_id)
        text = text.replace("{{title}}", title)
        text = text.replace("{{status}}", status)
        text = text.replace("{{start_date}}", start_date)
        text = text.replace("{{end_date}}", end_date)
        text = text.replace("{{capacity_days}}", f"{capacity_days:.1f}")
        write_atomic(sprint_dir / "sprint.md", text)
    else:
        text = render_sprint_markdown(
            sprint_id, title, status, start_date, end_date, capacity_days, phases, audit_entry
        )
        write_atomic(sprint_dir / "sprint.md", text)

    doc_titles = {
        "business-requirements-document.html": "Business Requirements Document",
        "functional-requirements-document.html": "Functional Requirements Document",
        "functional-specification-document.html": "Functional Specification Document",
    }
    for doc_name, doc_label in doc_titles.items():
        doc_template = assets_dir / doc_name
        if doc_template.is_file():
            d_content = doc_template.read_text(encoding="utf-8")
            d_content = d_content.replace("{{sprint_id}}", html.escape(sprint_id, quote=True))
            d_content = d_content.replace("{{title}}", html.escape(title, quote=True))
            d_content = d_content.replace("{{status}}", html.escape(status, quote=True))
            write_atomic(sprint_dir / doc_name, d_content)
        else:
            d_content = generate_minimal_html(doc_label, title, sprint_id, status, phases)
            write_atomic(sprint_dir / doc_name, d_content)

    emit({
        "sprint": sprint_id,
        "path": str(sprint_dir.relative_to(root)),
        "status": status,
        "phases": [p.phase for p in phases],
        "deliverables": list(HTML_DOCS),
    })


def cmd_add_phase(args: argparse.Namespace) -> None:
    root = find_workspace_root(args.root)
    sprints = find_sprints_dir(root)
    sprint_dir, _ = resolve_sprint_dir(sprints, args.sprint)
    model = parse_sprint_md(sprint_dir)

    if model.status == "closed":
        fail("cannot add phase to closed sprint", 1)

    if model.status == "locked":
        if not args.hotfix or not (args.reason and args.reason.strip()):
            fail(
                f"sprint {model.sprint_id} is locked; pass --hotfix and --reason to add emergency hotfix phase",
                1,
            )

    moscow = args.moscow or "Must Have"
    if moscow not in VALID_MOSCOW:
        fail(f"invalid MoSCoW: {moscow}", 2)

    reach = float(args.reach) if args.reach is not None else 100.0
    if reach <= 0:
        fail("Reach must be > 0", 2)

    impact = float(args.impact) if args.impact is not None else 1.0
    if impact not in VALID_IMPACT:
        fail(f"Impact must be one of {VALID_IMPACT}", 2)

    confidence = float(args.confidence) if args.confidence is not None else 0.8
    if confidence not in VALID_CONFIDENCE:
        fail(f"Confidence must be one of {VALID_CONFIDENCE}", 2)

    effort = float(args.effort) if args.effort is not None else 1.0
    if effort <= 0:
        fail("Effort must be > 0", 2)

    score = round((reach * impact * confidence) / effort, 2)

    new_phase = PhaseScope(
        phase=args.phase,
        title=args.phase,
        moscow=moscow,
        reach=reach,
        impact=impact,
        confidence=confidence,
        effort=effort,
        rice_score=score,
        spec_link=f"../../specs/{args.phase}/impl.md",
    )
    all_phases = [*model.phases, new_phase]

    now_str = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    log_entry = (
        f"{now_str}: Added phase {args.phase} ({moscow}, RICE: {score:.2f})"
        if not args.hotfix
        else f"{now_str}: HOTFIX BYPASS added phase {args.phase}. Reason: {args.reason}"
    )

    new_content = render_sprint_markdown(
        model.sprint_id,
        model.title,
        model.status,
        model.start_date,
        model.end_date,
        model.capacity_days,
        all_phases,
        "\n- ".join([*model.audit_log, log_entry]),
    )
    write_atomic(sprint_dir / "sprint.md", new_content)

    emit({
        "sprint": model.sprint_id,
        "added": args.phase,
        "status": model.status,
        "phases": [p.phase for p in all_phases],
    })


def cmd_lock(args: argparse.Namespace) -> None:
    root = find_workspace_root(args.root)
    sprints = find_sprints_dir(root)
    sprint_dir, _ = resolve_sprint_dir(sprints, args.sprint)
    model = parse_sprint_md(sprint_dir)

    if model.status == "closed":
        fail("cannot lock closed sprint", 1)

    if model.status == "locked":
        emit({"sprint": model.sprint_id, "status": "locked", "message": "already locked"})
        return

    reason = args.reason.strip() if args.reason else "Sprint scope locked"
    now_str = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    audit_entry = f"{now_str}: Locked. Reason: {reason}"

    new_content = render_sprint_markdown(
        model.sprint_id,
        model.title,
        "locked",
        model.start_date,
        model.end_date,
        model.capacity_days,
        model.phases,
        "\n- ".join([*model.audit_log, audit_entry]),
    )
    write_atomic(sprint_dir / "sprint.md", new_content)

    emit({"sprint": model.sprint_id, "status": "locked", "reason": reason})


def cmd_unlock(args: argparse.Namespace) -> None:
    root = find_workspace_root(args.root)
    sprints = find_sprints_dir(root)
    sprint_dir, _ = resolve_sprint_dir(sprints, args.sprint)
    model = parse_sprint_md(sprint_dir)

    if model.status != "locked":
        fail(f"sprint is {model.status}, not locked", 1)

    if not args.confirm:
        fail("unlocking a sprint requires explicit --confirm", 1)

    if not args.reason or not args.reason.strip():
        fail("unlocking a sprint requires non-empty --reason", 1)

    now_str = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    audit_entry = f"{now_str}: Unlocked scope. Reason: {args.reason.strip()}"

    new_content = render_sprint_markdown(
        model.sprint_id,
        model.title,
        "open",
        model.start_date,
        model.end_date,
        model.capacity_days,
        model.phases,
        "\n- ".join([*model.audit_log, audit_entry]),
    )
    write_atomic(sprint_dir / "sprint.md", new_content)

    emit({"sprint": model.sprint_id, "status": "open", "reason": args.reason.strip()})


def cmd_close(args: argparse.Namespace) -> None:
    root = find_workspace_root(args.root)
    sprints = find_sprints_dir(root)
    sprint_dir, _ = resolve_sprint_dir(sprints, args.sprint)
    model = parse_sprint_md(sprint_dir)

    specs_script = get_specs_script(root)
    incomplete_tasks: list[dict[str, str]] = []
    incomplete_phases: list[str] = []

    if specs_script:
        for p in model.phases:
            try:
                proc = subprocess.run(
                    [sys.executable, str(specs_script), "--root", str(root), "show", p.phase],
                    capture_output=True,
                    text=True,
                    timeout=10,
                    check=True,
                )
                phase_data = json.loads(proc.stdout)
                tasks = phase_data.get("tasks", [])
                p_incomplete = [t for t in tasks if t.get("status") != "Done"]
                if p_incomplete:
                    incomplete_phases.append(p.phase)
                    for t in p_incomplete:
                        incomplete_tasks.append({
                            "phase": p.phase,
                            "task": t.get("id", ""),
                            "title": t.get("title", ""),
                            "status": t.get("status", ""),
                        })
            except Exception:
                pass

    if incomplete_tasks and not args.carry_over:
        fail(
            "incomplete tasks remain in sprint",
            1,
            sprint=model.sprint_id,
            can_close=False,
            incomplete_phases=incomplete_phases,
            incomplete_tasks=incomplete_tasks,
            recommendation="carry-over" if len(incomplete_tasks) > 3 else "implement",
        )

    now_str = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    audit_entry = (
        f"{now_str}: Closed sprint with {len(incomplete_tasks)} tasks carried over"
        if incomplete_tasks
        else f"{now_str}: Closed sprint - all deliverables complete"
    )

    new_content = render_sprint_markdown(
        model.sprint_id,
        model.title,
        "closed",
        model.start_date,
        model.end_date,
        model.capacity_days,
        model.phases,
        "\n- ".join([*model.audit_log, audit_entry]),
    )
    write_atomic(sprint_dir / "sprint.md", new_content)

    emit({
        "sprint": model.sprint_id,
        "status": "closed",
        "carried_over": incomplete_tasks if args.carry_over else [],
    })


def cmd_list(args: argparse.Namespace) -> None:
    root = find_workspace_root(args.root)
    sprints = find_sprints_dir(root)
    sprint_list: list[dict[str, Any]] = []

    for entry in sorted(sprints.iterdir()):
        if entry.is_dir() and SPRINT_DIR_RE.match(entry.name) and (entry / "sprint.md").is_file():
            model = parse_sprint_md(entry)
            sprint_list.append({
                "sprint": model.sprint_id,
                "number": model.number,
                "slug": model.slug,
                "title": model.title,
                "status": model.status,
                "phases_count": len(model.phases),
                "committed_effort": model.committed_effort,
                "capacity_days": model.capacity_days,
            })

    emit({"sprints": sprint_list})


def cmd_show(args: argparse.Namespace) -> None:
    root = find_workspace_root(args.root)
    sprints = find_sprints_dir(root)
    sprint_dir, _ = resolve_sprint_dir(sprints, args.sprint)
    model = parse_sprint_md(sprint_dir)

    emit({
        "sprint": model.sprint_id,
        "number": model.number,
        "slug": model.slug,
        "title": model.title,
        "status": model.status,
        "start_date": model.start_date,
        "end_date": model.end_date,
        "capacity_days": model.capacity_days,
        "committed_effort": model.committed_effort,
        "phases": [asdict(p) for p in model.phases],
        "deliverables": model.deliverables,
        "audit_log": model.audit_log,
    })


def cmd_discover(args: argparse.Namespace) -> None:
    root = find_workspace_root(args.root)
    capacity_days = float(args.capacity_days) if args.capacity_days is not None else 5.0

    specs_script = get_specs_script(root)
    phases_raw: list[dict[str, Any]] = []
    if specs_script:
        try:
            proc = subprocess.run(
                [sys.executable, str(specs_script), "--root", str(root), "list", "--json"],
                capture_output=True,
                text=True,
                timeout=10,
                check=True,
            )
            phases_raw = json.loads(proc.stdout).get("phases", [])
        except Exception:
            pass

    mandatory: list[dict[str, Any]] = []
    recommended: list[dict[str, Any]] = []
    optional: list[dict[str, Any]] = []
    total_effort = 0.0

    for p in phases_raw:
        counts = p.get("counts", {})
        total_tasks = p.get("total", 0)
        done_tasks = counts.get("Done", 0)
        in_progress = counts.get("In Progress", 0)
        blocked = counts.get("Blocked", 0)
        in_review = counts.get("In Review", 0)
        title = p.get("title", "")
        name = p.get("dir", "")

        is_bugfix = "bug fix" in title.lower() or "fix" in name.lower()
        has_active_work = in_progress > 0 or blocked > 0 or in_review > 0
        rem_tasks = max(0, total_tasks - done_tasks)
        if rem_tasks == 0:
            continue

        effort_days = round(max(0.5, rem_tasks * 0.5), 1)
        oversized = effort_days > 5.0 or rem_tasks > 12

        item = {
            "phase": name,
            "title": title,
            "remaining_tasks": rem_tasks,
            "estimated_effort_days": effort_days,
            "oversized": oversized,
        }

        if is_bugfix or has_active_work:
            item["reason"] = "hotfix/bugfix" if is_bugfix else "active carry-over"
            mandatory.append(item)
            total_effort += effort_days
        elif len(recommended) < 2:
            recommended.append(item)
            total_effort += effort_days
        else:
            optional.append(item)

    emit({
        "mandatory": mandatory,
        "recommended": recommended,
        "optional": optional,
        "capacity_days": capacity_days,
        "total_estimated_effort": round(total_effort, 1),
        "overloaded": total_effort > capacity_days or (len(mandatory) + len(recommended)) > 3,
    })


def cmd_status(args: argparse.Namespace) -> None:
    root = find_workspace_root(args.root)
    sprints = find_sprints_dir(root)
    sprint_dir, _ = resolve_sprint_dir(sprints, args.sprint)
    model = parse_sprint_md(sprint_dir)

    coord_script = get_coord_script(root)
    claims: list[dict[str, Any]] = []
    if coord_script:
        try:
            env = {**os.environ}
            proc = subprocess.run(
                [sys.executable, str(coord_script), "overview"],
                capture_output=True,
                text=True,
                timeout=10,
                check=True,
                env=env,
            )
            claims = json.loads(proc.stdout).get("claims", [])
        except Exception:
            pass

    specs_script = get_specs_script(root)
    remaining_tasks: list[dict[str, Any]] = []
    in_development: list[dict[str, Any]] = []

    for p in model.phases:
        if not specs_script:
            continue
        try:
            proc = subprocess.run(
                [sys.executable, str(specs_script), "--root", str(root), "show", p.phase],
                capture_output=True,
                text=True,
                timeout=10,
                check=True,
            )
            p_data = json.loads(proc.stdout)
            for t in p_data.get("tasks", []):
                t_id = t.get("id")
                t_status = t.get("status")
                is_claimed = any(c.get("phase") == p.phase and c.get("task") == t_id for c in claims)
                if t_status == "In Progress" or is_claimed:
                    in_development.append({
                        "phase": p.phase,
                        "task": t_id,
                        "title": t.get("title"),
                        "claimed": is_claimed,
                    })
                elif t_status != "Done":
                    remaining_tasks.append({
                        "phase": p.phase,
                        "task": t_id,
                        "title": t.get("title"),
                        "moscow": p.moscow,
                        "rice": p.rice_score,
                        "status": t_status,
                    })
        except Exception:
            pass

    by_moscow: dict[str, list[dict[str, Any]]] = {m: [] for m in VALID_MOSCOW}
    for t in remaining_tasks:
        by_moscow[t["moscow"]].append(t)

    total_rem = len(remaining_tasks)
    seq_hours = total_rem * 3.0
    parallel_hours = max(3.0, (total_rem / 2.0) * 3.0) if total_rem else 0.0

    emit({
        "sprint": model.sprint_id,
        "status": model.status,
        "in_development": in_development,
        "remaining_by_moscow": by_moscow,
        "total_remaining_tasks": total_rem,
        "eta": {
            "sequential_hours": round(seq_hours, 1),
            "sequential_days": round(seq_hours / 6.0, 1),
            "parallel_waves_hours": round(parallel_hours, 1),
            "parallel_waves_days": round(parallel_hours / 6.0, 1),
        },
    })


def is_path_overlapping(paths_a: list[str], paths_b: list[str]) -> bool:
    for a in paths_a:
        for b in paths_b:
            if a == b:
                return True
            if a.startswith(b.rstrip("/") + "/") or b.startswith(a.rstrip("/") + "/"):
                return True
            for hot in HOT_FILES:
                if (a == hot or a.endswith("/" + hot)) and (b == hot or b.endswith("/" + hot)):
                    return True
    return False


def cmd_next_work(args: argparse.Namespace) -> None:
    root = find_workspace_root(args.root)
    sprints = find_sprints_dir(root)
    sprint_dir, _ = resolve_sprint_dir(sprints, args.sprint)
    model = parse_sprint_md(sprint_dir)

    coord_script = get_coord_script(root)
    claims: list[dict[str, Any]] = []
    if coord_script:
        try:
            env = {**os.environ}
            proc = subprocess.run(
                [sys.executable, str(coord_script), "overview"],
                capture_output=True,
                text=True,
                timeout=10,
                check=True,
                env=env,
            )
            claims = json.loads(proc.stdout).get("claims", [])
        except Exception:
            pass

    claimed_task_keys = {f"{c.get('phase')}::{c.get('task')}" for c in claims}

    specs_script = get_specs_script(root)
    if not specs_script:
        fail("specs.py script unavailable", 1)

    running_paths: list[str] = []
    tasks_by_phase: dict[str, list[dict[str, Any]]] = {}

    for p in model.phases:
        try:
            proc = subprocess.run(
                [sys.executable, str(specs_script), "--root", str(root), "show", p.phase],
                capture_output=True,
                text=True,
                timeout=10,
                check=True,
            )
            p_data = json.loads(proc.stdout)
            t_list = p_data.get("tasks", [])
            tasks_by_phase[p.phase] = t_list
            for t in t_list:
                key = f"{p.phase}::{t.get('id')}"
                if t.get("status") == "In Progress" or key in claimed_task_keys:
                    running_paths.extend(t.get("affected_paths") or [])
        except Exception:
            pass

    sorted_phases = sorted(
        model.phases,
        key=lambda x: (VALID_MOSCOW.index(x.moscow) if x.moscow in VALID_MOSCOW else 99, -x.rice_score),
    )

    chosen_phase: str | None = None
    chosen_task: dict[str, Any] | None = None
    parallel_candidates: list[dict[str, Any]] = []

    for p in sorted_phases:
        t_list = tasks_by_phase.get(p.phase, [])
        for t in t_list:
            t_id = t.get("id")
            key = f"{p.phase}::{t_id}"
            if t.get("status") != "Pending" and t.get("status") != "In Progress":
                continue
            if key in claimed_task_keys or t.get("status") == "In Progress":
                continue

            paths = t.get("affected_paths") or []
            if running_paths and is_path_overlapping(paths, running_paths):
                continue

            if chosen_task is None:
                chosen_phase = p.phase
                chosen_task = t
            else:
                chosen_paths = chosen_task.get("affected_paths") or []
                if not is_path_overlapping(paths, chosen_paths):
                    parallel_candidates.append({"phase": p.phase, "task": t_id, "title": t.get("title")})

    emit({
        "sprint": model.sprint_id,
        "next_phase": chosen_phase,
        "next_task": chosen_task.get("id") if chosen_task else None,
        "task": chosen_task,
        "parallel_candidates": parallel_candidates,
    })


def cmd_lint(args: argparse.Namespace) -> None:
    root = find_workspace_root(args.root)
    sprints = find_sprints_dir(root)
    sprint_dir, _ = resolve_sprint_dir(sprints, args.sprint)

    problems: list[dict[str, str]] = []
    sprint_file = sprint_dir / "sprint.md"
    if not sprint_file.is_file():
        problems.append({"file": "sprint.md", "code": "missing-file", "message": "sprint.md does not exist"})
    else:
        content = sprint_file.read_text(encoding="utf-8")
        required_sections = ("## Metadata", "## Deliverables", "## Prioritized Scope", "## Lock & Audit Log")
        for sec in required_sections:
            if sec not in content and "## Scope" not in content:
                problems.append({"file": "sprint.md", "code": "missing-section", "message": f"missing section: {sec}"})
        for m in FIELD_RE.finditer(content):
            problems.append({"file": "sprint.md", "code": "unrendered-field", "message": f"unrendered token: {m.group(0)}"})

    for doc in HTML_DOCS:
        doc_path = sprint_dir / doc
        if not doc_path.is_file():
            problems.append({"file": doc, "code": "missing-html", "message": f"{doc} missing"})
            continue
        html_text = doc_path.read_text(encoding="utf-8")
        if SCRIPT_TAG_RE.search(html_text):
            problems.append({"file": doc, "code": "security-script-tag", "message": "script tag forbidden"})
        if JAVASCRIPT_URL_RE.search(html_text):
            problems.append({"file": doc, "code": "security-javascript-url", "message": "javascript: URI forbidden"})
        if EVENT_HANDLER_RE.search(html_text):
            problems.append({"file": doc, "code": "security-event-handler", "message": "inline event handler forbidden"})
        if EXTERNAL_RESOURCE_RE.search(html_text):
            problems.append({"file": doc, "code": "security-external-resource", "message": "external stylesheet or script forbidden"})
        for m in FIELD_RE.finditer(html_text):
            problems.append({"file": doc, "code": "unrendered-field", "message": f"unrendered token: {m.group(0)}"})

    ok = len(problems) == 0
    if not ok:
        fail("lint failed", 1, sprint=sprint_dir.name, ok=False, problems=problems)
    emit({"sprint": sprint_dir.name, "ok": True, "problems": []})


def cmd_commands(args: argparse.Namespace) -> None:
    root = find_workspace_root(args.root)
    sprints = find_sprints_dir(root)

    active_status: str | None = None
    for entry in sprints.iterdir():
        if entry.is_dir() and SPRINT_DIR_RE.match(entry.name) and (entry / "sprint.md").is_file():
            model = parse_sprint_md(entry)
            if model.status in ("open", "locked"):
                active_status = model.status
                break

    catalog = [
        {
            "command": "/pm-new-sprint",
            "title": "Create New Sprint",
            "description": "Scaffolds new 1-week sprint with RICE and MoSCoW prioritization",
            "usable": active_status is None,
            "reason": "An active sprint is currently open or locked" if active_status else "Ready to plan new sprint",
        },
        {
            "command": "/pm-sprint-explain",
            "title": "Explain Sprint",
            "description": "Briefly walks through sprint goals, priorities, and deliverables",
            "usable": active_status is not None,
            "reason": "Sprint available to explain" if active_status else "No active sprint",
        },
        {
            "command": "/pm-sprint-lock",
            "title": "Lock Sprint Scope",
            "description": "Locks sprint to prevent mid-flight scope creep",
            "usable": active_status == "open",
            "reason": "Sprint is open and ready to lock" if active_status == "open" else "Sprint is not in open status",
        },
        {
            "command": "/pm-sprint-unlock",
            "title": "Unlock Sprint Scope",
            "description": "Unlocks locked sprint with explicit confirmation and reason",
            "usable": active_status == "locked",
            "reason": "Sprint is locked" if active_status == "locked" else "Sprint is not locked",
        },
        {
            "command": "/pm-sprint-close",
            "title": "Close Sprint",
            "description": "Closes sprint and checks remaining deliverables",
            "usable": active_status is not None,
            "reason": "Sprint available to close" if active_status else "No active sprint",
        },
        {
            "command": "/pm-sprint-implement",
            "title": "Implement Sprint",
            "description": "Executes full sprint work across non-colliding parallel waves",
            "usable": active_status is not None,
            "reason": "Sprint ready for implementation" if active_status else "No active sprint",
        },
        {
            "command": "/pm-sprint-status",
            "title": "Sprint Status",
            "description": "Shows real-time progress, claims, blockers, and ETA",
            "usable": active_status is not None,
            "reason": "Sprint status available" if active_status else "No active sprint",
        },
        {
            "command": "/pm-sprint-list",
            "title": "List Sprints",
            "description": "Lists all active, planned, and closed sprints",
            "usable": True,
            "reason": "Always available",
        },
        {
            "command": "/pm-commands",
            "title": "Sprint Commands Catalog",
            "description": "Lists all /pm-* commands and their availability",
            "usable": True,
            "reason": "Always available",
        },
    ]

    emit({"active_status": active_status, "commands": catalog})


class JsonArgumentParser(argparse.ArgumentParser):
    def error(self, message: str) -> NoReturn:
        fail(message, 2)


def build_parser() -> argparse.ArgumentParser:
    parser = JsonArgumentParser(description="Deterministic Sprint Engine")
    parser.add_argument("--root", help="workspace root containing specs/ and sprints/")
    sub = parser.add_subparsers(dest="command", required=True, parser_class=JsonArgumentParser)

    sub.add_parser("next-sprint")
    res = sub.add_parser("resolve")
    res.add_argument("sprint", nargs="?")

    create = sub.add_parser("create")
    create.add_argument("--title", required=True)
    create.add_argument("--slug")
    create.add_argument("--status", default="open")
    create.add_argument("--start")
    create.add_argument("--end")
    create.add_argument("--capacity-days", type=float, default=5.0)
    create.add_argument("--phase")
    create.add_argument("--moscow")
    create.add_argument("--reach", type=float)
    create.add_argument("--impact", type=float)
    create.add_argument("--confidence", type=float)
    create.add_argument("--effort", type=float)

    add_phase = sub.add_parser("add-phase")
    add_phase.add_argument("sprint")
    add_phase.add_argument("phase")
    add_phase.add_argument("--moscow")
    add_phase.add_argument("--reach", type=float)
    add_phase.add_argument("--impact", type=float)
    add_phase.add_argument("--confidence", type=float)
    add_phase.add_argument("--effort", type=float)
    add_phase.add_argument("--hotfix", action="store_true")
    add_phase.add_argument("--reason")

    lock = sub.add_parser("lock")
    lock.add_argument("sprint")
    lock.add_argument("--reason")

    unlock = sub.add_parser("unlock")
    unlock.add_argument("sprint")
    unlock.add_argument("--confirm", action="store_true")
    unlock.add_argument("--reason")

    close = sub.add_parser("close")
    close.add_argument("sprint")
    close.add_argument("--carry-over", action="store_true")

    sub.add_parser("list")
    show = sub.add_parser("show")
    show.add_argument("sprint")

    discover = sub.add_parser("discover")
    discover.add_argument("--capacity-days", type=float, default=5.0)

    status = sub.add_parser("status")
    status.add_argument("sprint", nargs="?")

    next_work = sub.add_parser("next-work")
    next_work.add_argument("sprint", nargs="?")

    lint = sub.add_parser("lint")
    lint.add_argument("sprint")

    sub.add_parser("commands")

    return parser


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()
    dispatch = {
        "next-sprint": cmd_next_sprint,
        "resolve": cmd_resolve,
        "create": cmd_create,
        "add-phase": cmd_add_phase,
        "lock": cmd_lock,
        "unlock": cmd_unlock,
        "close": cmd_close,
        "list": cmd_list,
        "show": cmd_show,
        "discover": cmd_discover,
        "status": cmd_status,
        "next-work": cmd_next_work,
        "lint": cmd_lint,
        "commands": cmd_commands,
    }
    dispatch[args.command](args)


if __name__ == "__main__":
    main()
