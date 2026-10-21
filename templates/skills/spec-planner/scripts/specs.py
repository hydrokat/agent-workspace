#!/usr/bin/env python3
"""Parse specs/ phases and tasks into a typed model and answer queries.

Stdlib only. JSON on stdout. Exit codes: 0 ok, 1 not found/ambiguous, 2 invalid input.

Read commands:
  list [--json]               {"phases": [{dir, path, legacy, number, title, scope_type,
                              counts{status: n}, total, next_task}]}
                              next_task is the first task that is not Done.
  show <phase> [--task ID]    phase: {dir, path, legacy, number, title, scope_type, primary_repo,
                              repositories[{name, role}], branches{working, target, pattern, source},
                              objectives[], architecture, decisions[{topic, decision}],
                              open_questions[{task, text}], counts, tasks[], warnings[]}
                              with --task: {phase, title, task}
                              task: {id, title, type, status, status_source, in_impl_checked,
                              description, rationale, objectives{checked,total}, testing{...},
                              security{...}, affected_paths[], depends_on[], external_gates[],
                              file, warnings[]}
                              Missing sections come back as null with a warnings[] entry.
  next-phase                  {"next": "004"}
  next-task <phase>           {"next": "t-012"}
  resolve [phase]             {"phase", "path", "legacy", "selected": "explicit"|"active"}
                              Accepts p-002, 002, 2, a full directory name, or nothing (the latest
                              phase with an incomplete task).

Scaffold commands (render ../assets/*.md; `{{field}}` is script-filled, `[prose]` is for the agent):
  new-phase (--repo SLUG | --multi) --title T --branch B --target B [--pattern P] [--pattern-source S]
                              Creates specs/p-###-<slug>/impl.md and the delivery task t-001.md.
                              {"phase", "path", "files"}. Exit 1 if the directory already exists.
  new-task PHASE --title T --type {New Feature,Enhancement,Bug Fix} [--depends t-###...] [--paths P...]
                              Inserts the task before the delivery task, renumbers the delivery task, and
                              updates the impl.md Timeline and Tasks lists.
                              {"task", "file", "delivery", "warnings"}. Legacy phases are read-only (exit 2).
                              Writes the new delivery file, then the new task file, then impl.md.
  lint PHASE                  {"phase", "ok", "problems": [{file, code, message}]}; exit 1 when not ok.
                              Codes: missing-section, delivery-missing, delivery-not-last, multiple-deliveries,
                              task-not-in-impl, task-file-missing, status-mismatch, title-mismatch,
                              placeholder, unrendered-field.

Parallel planning:
  waves PHASE [--max-parallel N] [--cleared GATE...] [--done t-###...] [--running t-###...] [--write]
                              Orders the unfinished tasks into waves that can run side by side. Edges come from
                              ## Depends On; tasks whose ## Affected Paths overlap (equal, parent/child, same
                              glob root, or a shared HOT_FILES name) never share a wave; a task with no paths
                              runs alone; the delivery task runs alone, last. Other Depends On entries are gates:
                              the task is `gated` until a --cleared text is contained in the gate text.
                              Tasks claimed in coordination (`coord.py claims`) or In Progress count as running.
                              {"waves": [[ids]], "order": [ids], "tasks": {id: {title, status, state, wave,
                              reasons, conflicts_with}}, "gates", "held", "warnings"}. state is one of done,
                              running, ready, waiting, gated, blocked, in-review. Exit 1 names a dependency cycle.
                              --write stores the plan as the `## Parallel Waves` section of impl.md.

Status and compile commands:
  set-status PHASE TASK STATUS
                              STATUS is Pending | In Progress | Blocked | In Review | Done. Updates the task's
                              ## Status and the impl.md Timeline label and Tasks checkbox together.
                              Done is refused (exit 1, {"missing": [...]}) unless the objectives, testing, and
                              security checklists are fully checked (a delivery task needs only objectives).
                              {"task", "from", "to", "changed", "files"}.
  normalize PHASE TASK        Appends the missing sections of ../../spec-implement/assets/task-metadata.md before
                              ## Status. Existing content is never rewritten. {"task", "added": [...]}.
  compile [all|completed|incomplete|NNN|NNN-NNN] [--date YYYY-MM-DD]
                              Classifies every task (Done / Tested / Security Audited, per
                              ../../spec-compile/references/completion-criteria.md), clears the old output, and
                              writes specs/compiled/{completed,incomplete}.md from ../../spec-compile/assets.
                              {"filter", "scope", "written", "phases": {in_scope, complete, incomplete},
                              "tasks": {in_scope, complete}, "note", "warnings"}.
"""
from __future__ import annotations

import argparse
import datetime
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any, NoReturn

MAX_FILE_BYTES = 1_000_000
MAX_TITLE = 120
DEFAULT_PARALLEL = 3
MAX_PARALLEL = 8
HOT_FILES = (
    "CHANGELOG.md", "package.json", "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "uv.lock",
    "poetry.lock", "Cargo.lock", "composer.lock", "Gemfile.lock", "go.sum", "test/pack.test.js",
)
MAX_PATH_TEXT = 300
MAX_PATHS = 50
SLUG_RE = re.compile(r"^[a-z0-9][a-z0-9-]{0,47}$")
BRANCH_RE = re.compile(r"^[A-Za-z0-9_][A-Za-z0-9._/-]{0,99}$")
CTRL_RE = re.compile(r"[\x00-\x1f\x7f-\x9f]")
FIELD_RE = re.compile(r"\{\{([a-z_]+)\}\}")
PLACEHOLDER_RE = re.compile(r"\[[^\]\n]{3,}\]")
FENCED_RE = re.compile(r"^(`{3,}|~{3,}).*?^\1[ \t]*$", re.MULTILINE | re.DOTALL)
INLINE_CODE_RE = re.compile(r"`[^`\n]*`")
DELIVERY_TITLE = "Delivery: Commit, PR, Review, CI, Merge"
IMPL_SECTIONS = ("branch strategy", "repository scope", "code structure diagram", "timeline", "tasks")
TASK_SECTIONS = (
    ("affected paths",), ("depends on",), ("testing", "test results", "tests"), ("security",),
    ("knowledge updates", "knowledgebase updates"),
)
DELIVERY_SECTIONS = (("objectives",), ("status",))
STATUS_LINE_RE = re.compile(r"^- \[[ xX]\].*$", re.MULTILINE)
DATE_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")
RANGE_RE = re.compile(r"^(\d{3})-(\d{3})$")
BLOCK_RE = "<!-- {tag} -->\n?(.*?)<!-- /{tag} -->\n?"
DONE_LINE_RE = re.compile(r"^\s*Status:\s*(Done|Complete|✅)", re.IGNORECASE | re.MULTILINE)
TESTED_LINE_RE = re.compile(r"^\s*(Tested:\s*(Yes|✅)|Testing:\s*Passed)", re.IGNORECASE | re.MULTILINE)
SECURITY_LINE_RE = re.compile(
    r"^\s*(Security Audit:\s*Passed|Security:\s*✅|Security Reviewed:\s*Yes)", re.IGNORECASE | re.MULTILINE,
)
STATUS_TESTED_RE = re.compile(r"^- \[[xX]\]\s*Tested\b", re.IGNORECASE | re.MULTILINE)
STATUS_SECURITY_RE = re.compile(r"^- \[[xX]\]\s*Security (Audited|Reviewed)\b", re.IGNORECASE | re.MULTILINE)
METADATA_ALIASES = {
    "agent assignment": ("agent assignment",),
    "knowledgebase references": ("knowledgebase references", "knowledge references"),
    "progress": ("progress",),
    "knowledgebase updates": ("knowledgebase updates", "knowledge updates"),
}
STATUSES = ("Pending", "In Progress", "Blocked", "In Review", "Done")
TYPES = ("New Feature", "Enhancement", "Bug Fix")
DELIVERY = "Delivery"
PHASE_DIR_RE = re.compile(r"^p-(\d{3})-[a-z0-9-]+$")
LEGACY_DIR_RE = re.compile(r"^phase-(\d{3})(?:-[a-z0-9-]+)?$")
NUMBER_ARG_RE = re.compile(r"^(?:p-|phase-)?(\d{1,3})$")
TASK_ID_RE = re.compile(r"^(?:t|task)-\d{3}$")
TASK_FILE_RE = re.compile(r"^(?:t|task)-(\d{3})\.md$")
CHECKBOX_RE = re.compile(r"^\s*- \[( |x|X)\]\s*(.*)$")
FENCE_RE = re.compile(r"^\s*(`{3,}|~{3,})")
EXTERNAL_RE = re.compile(r"\bp-\d{3}\b|\bPR\s*#\d+|\bphase-\d{3}\b")


def fail(message: str, code: int = 2, **extra: Any) -> NoReturn:
    print(json.dumps({"error": message, **extra}))
    sys.exit(code)


def emit(data: dict[str, Any]) -> None:
    print(json.dumps(data, ensure_ascii=False))


@dataclass
class Checklist:
    checked: int
    total: int


@dataclass
class Task:
    id: str
    title: str | None
    type: str | None
    status: str
    status_source: str
    in_impl_checked: bool | None
    timeline_status: str | None
    description: str | None
    rationale: str | None
    objectives: Checklist | None
    testing: Checklist | None
    security: Checklist | None
    affected_paths: list[str] | None
    depends_on: list[str]
    external_gates: list[str]
    file: str | None
    warnings: list[str] = field(default_factory=list)


@dataclass
class Phase:
    dir: str
    path: str
    legacy: bool
    number: int
    title: str | None
    scope_type: str | None
    primary_repo: str | None
    repositories: list[dict[str, str]] | None
    branches: dict[str, str | None] | None
    objectives: list[str] | None
    architecture: str | None
    decisions: list[dict[str, str]] | None
    open_questions: list[dict[str, str | None]]
    counts: dict[str, int]
    tasks: list[Task]
    warnings: list[str] = field(default_factory=list)


def find_specs(root_arg: str | None) -> Path:
    if root_arg:
        specs = Path(root_arg) / "specs"
        if not specs.is_dir():
            fail(f"specs/ directory not found under {root_arg}", 1)
        return specs.resolve()
    here = Path.cwd().resolve()
    for candidate in (here, *here.parents):
        if (candidate / "specs").is_dir():
            return (candidate / "specs").resolve()
    fail("specs/ directory not found", 1)


def read_text(path: Path, warnings: list[str]) -> str | None:
    try:
        if path.stat().st_size > MAX_FILE_BYTES:
            warnings.append(f"{path.name}: larger than {MAX_FILE_BYTES} bytes, skipped")
            return None
        return path.read_text(encoding="utf-8", errors="replace")
    except OSError as exc:
        warnings.append(f"{path.name}: unreadable ({exc.strerror})")
        return None


def split_sections(text: str) -> tuple[str | None, dict[str, list[str]]]:
    title: str | None = None
    sections: dict[str, list[str]] = {}
    current: list[str] | None = None
    fence: str | None = None
    for line in text.splitlines():
        match = FENCE_RE.match(line)
        if match:
            marker = match.group(1)
            if fence is None:
                fence = marker[0] * len(marker)
            elif marker.startswith(fence):
                fence = None
        if fence is None and not match:
            if line.startswith("# ") and title is None:
                title = line[2:].strip()
                continue
            if line.startswith("## "):
                current = sections.setdefault(line[3:].strip().lower(), [])
                continue
        if current is not None:
            current.append(line)
    return title, sections


def section(sections: dict[str, list[str]], *prefixes: str) -> list[str] | None:
    for name, lines in sections.items():
        if any(name.startswith(prefix) for prefix in prefixes):
            return lines
    return None


def checklist(lines: list[str] | None) -> Checklist | None:
    if lines is None:
        return None
    boxes = [m for m in map(CHECKBOX_RE.match, lines) if m]
    if not boxes:
        return None
    return Checklist(sum(1 for m in boxes if m.group(1) != " "), len(boxes))


def bullet_field(lines: list[str] | None, name: str) -> str | None:
    if lines is None:
        return None
    pattern = re.compile(rf"^\s*- \*\*{re.escape(name)}\*\*:\s*(.*)$")
    for line in lines:
        match = pattern.match(line)
        if match:
            value = match.group(1).strip()
            ticked = re.match(r"`([^`]+)`", value)
            return ticked.group(1) if ticked else value
    return None


def excerpt(lines: list[str] | None, limit: int) -> str | None:
    if lines is None:
        return None
    text = "\n".join(lines).strip()
    if not text:
        return None
    return text if len(text) <= limit else text[:limit].rstrip() + "…"


def list_items(lines: list[str] | None) -> list[str]:
    items: list[str] = []
    for line in lines or []:
        match = re.match(r"^(?:\d+\.|-|\*)\s+(.*)$", line)
        if match:
            items.append(match.group(1).strip())
        elif items and line.startswith((" ", "\t")) and line.strip():
            items[-1] += " " + line.strip()
    return items


def parse_status_label(label: str) -> str | None:
    cleaned = label.strip().strip("*_ ").lower()
    for status in STATUSES:
        lowered = status.lower()
        if cleaned == lowered or re.match(rf"^{re.escape(lowered)}\b", cleaned):
            return status
    return None


def resolve_status(
    status_lines: list[str] | None, timeline: str | None, text: str,
    objectives: Checklist | None,
) -> tuple[str, str]:
    for line in status_lines or []:
        match = CHECKBOX_RE.match(line)
        if match:
            label = parse_status_label(match.group(2))
            if label:
                return label, "status-section"
            break
    if timeline:
        return timeline, "timeline"
    for line in status_lines or []:
        match = CHECKBOX_RE.match(line)
        if match and match.group(1) != " ":
            return "Done", "criteria"
    if re.search(r"^\s*Status:\s*(Done|Complete|✅)", text, re.IGNORECASE | re.MULTILINE):
        return "Done", "criteria"
    if objectives and objectives.checked == objectives.total:
        return "Done", "criteria"
    return "Pending", "default"


def parse_paths(lines: list[str] | None) -> list[str] | None:
    if lines is None:
        return None
    paths: list[str] = []
    for item in list_items(lines):
        spans = re.findall(r"`([^`]+)`", item)
        if spans:
            paths.extend(spans)
        elif item.lower() != "none":
            paths.append(item)
    return paths


def parse_depends(lines: list[str] | None) -> tuple[list[str], list[str]]:
    depends: list[str] = []
    gates: list[str] = []
    for item in list_items(lines):
        if EXTERNAL_RE.search(item):
            gates.append(item)
            continue
        for found in re.findall(r"\b(?:t|task)-\d{3}\b", item):
            if found not in depends:
                depends.append(found)
    return depends, gates


def parse_task(
    task_id: str, path: Path | None, specs: Path, impl_entry: dict[str, Any] | None,
    timeline: str | None,
) -> Task:
    warnings: list[str] = []
    text = read_text(path, warnings) if path else None
    if path is None:
        warnings.append(f"{task_id}: listed in impl.md but has no task file")
    title: str | None = impl_entry["title"] if impl_entry else None
    sections: dict[str, list[str]] = {}
    if text is not None:
        file_title, sections = split_sections(text)
        heading = re.match(r"^(?:t|task)-\d{3}:\s*(.*)$", file_title or "", re.IGNORECASE)
        if heading:
            title = heading.group(1).strip()
        elif file_title and re.match(r"^task\s*\d+", file_title, re.IGNORECASE):
            title = re.sub(r"^task\s*\d+\s*[:\-–—]?\s*", "", file_title, flags=re.IGNORECASE).strip()
        else:
            warnings.append(f"{path.name if path else task_id}: missing '# {task_id}: <title>' heading")
    classification = section(sections, "requirement classification")
    task_type = bullet_field(classification, "Type")
    type_name = next((t for t in TYPES if task_type and t.lower() in task_type.lower()), None)
    if title and title.lower().startswith("delivery"):
        type_name = DELIVERY
    elif type_name is None and impl_entry and impl_entry["type"]:
        type_name = impl_entry["type"]
    if type_name is None and text is not None:
        warnings.append("no Requirement Classification type")
    objectives = checklist(section(sections, "objectives"))
    testing = checklist(section(sections, "testing", "test results", "tests"))
    security = checklist(section(sections, "security"))
    if text is not None:
        if objectives is None:
            warnings.append("no Objectives checklist")
        if testing is None and type_name != DELIVERY:
            warnings.append("no Testing checklist")
        if security is None and type_name != DELIVERY:
            warnings.append("no Security checklist")
    status, source = resolve_status(section(sections, "status"), timeline, text or "", objectives)
    paths = parse_paths(section(sections, "affected paths"))
    depends, gates = parse_depends(section(sections, "depends on"))
    relative = path.relative_to(specs.parent).as_posix() if path else None
    return Task(
        id=task_id, title=title, type=type_name, status=status, status_source=source,
        in_impl_checked=impl_entry["checked"] if impl_entry else None,
        timeline_status=timeline,
        description=excerpt(section(sections, "description"), 800),
        rationale=bullet_field(classification, "Rationale"),
        objectives=objectives, testing=testing, security=security,
        affected_paths=paths, depends_on=depends, external_gates=gates,
        file=relative, warnings=warnings,
    )


def parse_impl_lists(lines: list[str] | None) -> dict[str, dict[str, Any]]:
    entries: dict[str, dict[str, Any]] = {}
    for line in lines or []:
        match = re.match(
            r"^- \[( |x|X)\]\s*((?:t|task)-\d{3}):\s*(?:\[([^\]]+)\]\s*)?(.*)$", line,
        )
        if match:
            entries[match.group(2)] = {
                "checked": match.group(1) != " ",
                "type": match.group(3) if match.group(3) in TYPES else None,
                "title": match.group(4).strip(),
            }
    return entries


def parse_timeline(lines: list[str] | None) -> dict[str, str]:
    labels: dict[str, str] = {}
    for line in lines or []:
        match = re.match(r"^- \*\*((?:t|task)-\d{3})\*\*:.*\(([^()]*)\)\s*$", line)
        if match:
            status = parse_status_label(match.group(2))
            if status:
                labels[match.group(1)] = status
    return labels


def parse_decisions(lines: list[str] | None) -> list[dict[str, str]] | None:
    if lines is None:
        return None
    rows: list[dict[str, str]] = []
    for line in lines:
        if not line.strip().startswith("|"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) < 2 or set("".join(cells)) <= set("-: "):
            continue
        rows.append({"topic": cells[0], "decision": cells[1]})
    return rows[1:] if rows else None


def parse_questions(lines: list[str] | None) -> list[dict[str, str | None]]:
    questions: list[dict[str, str | None]] = []
    for item in list_items(lines):
        task = re.search(r"\(((?:t|task)-\d{3})\)", item)
        questions.append({"task": task.group(1) if task else None, "text": item})
    return questions


def parse_repositories(lines: list[str] | None) -> list[dict[str, str]] | None:
    if lines is None:
        return None
    repos = []
    for line in lines:
        match = re.match(r"^\s{2,}- `([^`]+)`(?:\s*\([^)]*\))?:\s*(.*)$", line)
        if match:
            repos.append({"name": match.group(1), "role": match.group(2).strip()})
    return repos


def parse_phase(directory: Path, specs: Path) -> Phase:
    warnings: list[str] = []
    legacy = LEGACY_DIR_RE.match(directory.name) is not None
    number = phase_number(directory.name)
    impl_path = directory / "impl.md"
    text = read_text(impl_path, warnings) if impl_path.is_file() else None
    if text is None and impl_path.is_file() is False:
        warnings.append("impl.md missing")
    title: str | None = None
    sections: dict[str, list[str]] = {}
    if text is not None:
        raw_title, sections = split_sections(text)
        title = re.sub(r"^Implementation Plan:\s*", "", raw_title or "").strip() or None
        if title is None:
            warnings.append("impl.md: missing '# Implementation Plan: <name>' heading")
    entries = parse_impl_lists(section(sections, "tasks"))
    timeline = parse_timeline(section(sections, "timeline"))
    files = {
        f.name.split(".")[0]: f
        for f in sorted(directory.iterdir())
        if TASK_FILE_RE.match(f.name) and f.is_file() and f.resolve().is_relative_to(specs)
    }
    for task_id in files:
        if task_id not in entries and entries:
            warnings.append(f"{task_id}: task file not listed in impl.md Tasks")
    ids = sorted(set(entries) | set(files))
    tasks = [
        parse_task(i, files.get(i), specs, entries.get(i), timeline.get(i)) for i in ids
    ]
    if not tasks:
        warnings.append("phase has no tasks")
    counts = {status: 0 for status in STATUSES}
    for task in tasks:
        counts[task.status] += 1
    branch_lines = section(sections, "branch strategy")
    branches = None
    if branch_lines is not None:
        branches = {
            "working": bullet_field(branch_lines, "Working Branch"),
            "target": bullet_field(branch_lines, "Merge Target"),
            "pattern": bullet_field(branch_lines, "Observed Branch Pattern"),
            "source": bullet_field(branch_lines, "Branch Pattern Source"),
        }
    elif text is not None:
        warnings.append("impl.md: no Branch Strategy section")
    scope = section(sections, "repository scope")
    if scope is None and text is not None:
        warnings.append("impl.md: no Repository Scope section")
    objective_lines = section(sections, "phase objectives", "objectives")
    summary = excerpt(objective_lines, 600)
    objectives = list_items(objective_lines) or ([summary] if summary else None)
    return Phase(
        dir=directory.name, path=f"specs/{directory.name}", legacy=legacy, number=number,
        title=title, scope_type=bullet_field(scope, "Scope Type"),
        primary_repo=bullet_field(scope, "Primary Repository"),
        repositories=parse_repositories(scope), branches=branches,
        objectives=objectives, architecture=excerpt(section(sections, "architecture"), 1200),
        decisions=parse_decisions(section(sections, "decisions", "key decisions")),
        open_questions=parse_questions(section(sections, "open questions")),
        counts=counts, tasks=tasks, warnings=warnings,
    )


def phase_number(name: str) -> int:
    match = PHASE_DIR_RE.match(name) or LEGACY_DIR_RE.match(name)
    return int(match.group(1)) if match else 0


def phase_dirs(specs: Path) -> list[Path]:
    found: list[Path] = []
    for entry in specs.iterdir():
        if not (PHASE_DIR_RE.match(entry.name) or LEGACY_DIR_RE.match(entry.name)):
            continue
        if entry.is_dir() and entry.resolve().is_relative_to(specs):
            found.append(entry)
    return sorted(found, key=lambda d: (phase_number(d.name), not LEGACY_DIR_RE.match(d.name), d.name))


def next_open(phase: Phase) -> str | None:
    return next((t.id for t in phase.tasks if t.status != "Done"), None)


def is_incomplete(phase: Phase) -> bool:
    return not phase.tasks or any(t.status != "Done" for t in phase.tasks)


def summarize(phase: Phase) -> dict[str, Any]:
    return {
        "dir": phase.dir, "path": phase.path, "legacy": phase.legacy, "number": phase.number,
        "title": phase.title, "scope_type": phase.scope_type, "counts": phase.counts,
        "total": len(phase.tasks), "next_task": next_open(phase),
    }


def resolve_dir(specs: Path, arg: str | None) -> tuple[Path, str]:
    dirs = phase_dirs(specs)
    if arg is None:
        for directory in reversed(dirs):
            if is_incomplete(parse_phase(directory, specs)):
                return directory, "active"
        fail("no phase with incomplete tasks", 1, valid=[d.name for d in dirs])
    exact = [d for d in dirs if d.name == arg]
    number = NUMBER_ARG_RE.match(arg)
    if not exact and number:
        wanted = int(number.group(1))
        matches = [d for d in dirs if phase_number(d.name) == wanted]
        current = [d for d in matches if PHASE_DIR_RE.match(d.name)]
        exact = current or matches
        if len(exact) > 1:
            fail(f"ambiguous phase {arg}", 1, valid=[d.name for d in exact])
    elif not exact and not (PHASE_DIR_RE.match(arg) or LEGACY_DIR_RE.match(arg)):
        fail(f"invalid phase argument: {arg!r}")
    if not exact:
        fail(f"phase not found: {arg}", 1, valid=[d.name for d in dirs])
    return exact[0], "explicit"


def check_phase_arg(arg: str) -> str:
    if not (NUMBER_ARG_RE.match(arg) or PHASE_DIR_RE.match(arg) or LEGACY_DIR_RE.match(arg)):
        fail(f"invalid phase argument: {arg!r}")
    return arg


def cmd_list(args: argparse.Namespace) -> None:
    specs = find_specs(args.root)
    emit({"phases": [summarize(parse_phase(d, specs)) for d in phase_dirs(specs)]})


def cmd_show(args: argparse.Namespace) -> None:
    specs = find_specs(args.root)
    directory, _ = resolve_dir(specs, check_phase_arg(args.phase))
    phase = parse_phase(directory, specs)
    if args.task is None:
        emit(asdict(phase))
        return
    if not TASK_ID_RE.match(args.task):
        fail(f"invalid task argument: {args.task!r}")
    task = next((t for t in phase.tasks if t.id == args.task), None)
    if task is None:
        fail(f"task not found: {args.task}", 1, valid=[t.id for t in phase.tasks])
    emit({"phase": phase.dir, "title": phase.title, "task": asdict(task)})


def cmd_next_phase(args: argparse.Namespace) -> None:
    specs = find_specs(args.root)
    numbers = [phase_number(d.name) for d in phase_dirs(specs)]
    emit({"next": f"{max(numbers, default=0) + 1:03d}"})


def cmd_next_task(args: argparse.Namespace) -> None:
    specs = find_specs(args.root)
    directory, _ = resolve_dir(specs, check_phase_arg(args.phase))
    numbers = [int(m.group(1)) for f in directory.iterdir() if (m := TASK_FILE_RE.match(f.name))]
    emit({"next": f"t-{max(numbers, default=0) + 1:03d}"})


def cmd_resolve(args: argparse.Namespace) -> None:
    specs = find_specs(args.root)
    arg = check_phase_arg(args.phase) if args.phase else None
    directory, selected = resolve_dir(specs, arg)
    emit({
        "phase": directory.name, "path": f"specs/{directory.name}",
        "legacy": LEGACY_DIR_RE.match(directory.name) is not None, "selected": selected,
    })


def assets_dir() -> Path:
    return Path(__file__).resolve().parent.parent / "assets"


def sibling_assets(skill: str) -> Path:
    return Path(__file__).resolve().parents[2] / skill / "assets"


def read_template(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except OSError:
        fail(f"template not found: {path.name}; copy the asset files by hand", 1)


def fill(template: str, fields: dict[str, str], name: str) -> str:
    missing = sorted(set(FIELD_RE.findall(template)) - set(fields))
    if missing:
        fail(f"template {name} has fields with no value: {missing}", 1)
    return FIELD_RE.sub(lambda m: fields[m.group(1)], template)


def render(name: str, fields: dict[str, str]) -> str:
    return fill(read_template(assets_dir() / name), fields, name)


def write_atomic(path: Path, content: str) -> None:
    fd, tmp = tempfile.mkstemp(dir=str(path.parent), prefix=".specs-", suffix=".tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="") as handle:
            handle.write(content)
        os.replace(tmp, path)
    except BaseException:
        if os.path.exists(tmp):
            os.unlink(tmp)
        raise


def clean_line(value: str, label: str, limit: int) -> str:
    cleaned = value.strip()
    if not cleaned or len(cleaned) > limit or CTRL_RE.search(cleaned):
        fail(f"invalid {label}: one line of 1..{limit} characters, no control characters")
    return cleaned


def check_branch(value: str, label: str) -> str:
    if not BRANCH_RE.match(value) or ".." in value or value.endswith(("/", ".lock")):
        fail(f"invalid {label} name: {value!r}")
    return value


def next_phase_number(specs: Path) -> int:
    return max((phase_number(d.name) for d in phase_dirs(specs)), default=0) + 1


def cmd_new_phase(args: argparse.Namespace) -> None:
    specs = find_specs(args.root)
    if bool(args.repo) == bool(args.multi):
        fail("choose exactly one of --repo <slug> or --multi")
    slug = args.repo if args.repo else "multi"
    if not SLUG_RE.match(slug):
        fail("invalid repo slug: lowercase letters, digits, and '-' only")
    title = clean_line(args.title, "title", MAX_TITLE)
    branch, target = check_branch(args.branch, "branch"), check_branch(args.target, "target")
    pattern = clean_line(args.pattern or "N/A", "pattern", MAX_TITLE)
    source = clean_line(args.pattern_source or "N/A", "pattern source", MAX_TITLE)
    dirname = f"p-{next_phase_number(specs):03d}-{slug}"
    phase_dir = specs / dirname
    if phase_dir.exists() or phase_dir.is_symlink():
        fail(f"{dirname} already exists", 1)
    delivery_id = "t-001"
    impl = render("impl.md", {
        "title": title, "branch": branch, "target": target, "pattern": pattern, "pattern_source": source,
        "phase_dir": dirname, "scope_type": "Multi Repo" if args.multi else "Single Repo",
        "primary_repo": slug,
        "timeline": f"- **{delivery_id}**: {DELIVERY_TITLE} (Pending)",
        "tasks": f"- [ ] {delivery_id}: {DELIVERY_TITLE}",
    })
    delivery = render("delivery-task.md", {"task_id": delivery_id, "branch": branch, "target": target})
    phase_dir.mkdir()
    write_atomic(phase_dir / f"{delivery_id}.md", delivery)
    write_atomic(phase_dir / "impl.md", impl)
    emit({"phase": dirname, "path": f"specs/{dirname}", "files": ["impl.md", f"{delivery_id}.md"]})


def insert_task_lines(
    impl: str, delivery_id: str | None, new_id: str, new_delivery_id: str | None, title: str, type_name: str,
) -> str | None:
    timeline_new = f"- **{new_id}**: {title} (Pending)"
    tasks_new = f"- [ ] {new_id}: [{type_name}] {title}"
    lines = impl.split("\n")
    out_lines: list[str] = []
    done_timeline = done_tasks = False
    last_timeline = last_tasks = -1
    for line in lines:
        if delivery_id and not done_timeline and re.match(rf"^- \*\*{delivery_id}\*\*:", line):
            out_lines += [timeline_new, line.replace(delivery_id, str(new_delivery_id), 1)]
            done_timeline = True
            continue
        if delivery_id and not done_tasks and re.match(rf"^- \[[ xX]\] {delivery_id}:", line):
            out_lines += [tasks_new, line.replace(delivery_id, str(new_delivery_id), 1)]
            done_tasks = True
            continue
        if re.match(r"^- \*\*(?:t|task)-\d{3}\*\*:", line):
            last_timeline = len(out_lines)
        elif re.match(r"^- \[[ xX]\] (?:t|task)-\d{3}:", line):
            last_tasks = len(out_lines)
        out_lines.append(line)
    if delivery_id is None and last_timeline >= 0 and last_tasks >= 0:
        out_lines.insert(last_tasks + 1, tasks_new)
        out_lines.insert(last_timeline + 1, timeline_new)
        return "\n".join(out_lines)
    if delivery_id and done_timeline and done_tasks:
        return "\n".join(out_lines)
    return None


def cmd_new_task(args: argparse.Namespace) -> None:
    specs = find_specs(args.root)
    directory, _ = resolve_dir(specs, check_phase_arg(args.phase))
    if LEGACY_DIR_RE.match(directory.name):
        fail("legacy phases are read-only")
    title = clean_line(args.title, "title", MAX_TITLE)
    if args.type not in TYPES:
        fail(f"invalid type: choose one of {', '.join(TYPES)}")
    phase = parse_phase(directory, specs)
    known = {t.id for t in phase.tasks}
    deliveries = [t for t in phase.tasks if t.type == DELIVERY]
    depends = args.depends or []
    for dep in depends:
        if not re.match(r"^t-\d{3}$", dep) or dep not in known or any(dep == d.id for d in deliveries):
            fail(f"invalid dependency: {dep!r} is not an implementation task of this phase")
    paths = args.paths or []
    if len(paths) > MAX_PATHS or any(not p.strip() or len(p) > MAX_PATH_TEXT or CTRL_RE.search(p) for p in paths):
        fail(f"invalid --paths: at most {MAX_PATHS} entries of 1..{MAX_PATH_TEXT} characters")
    branches = phase.branches or {}
    if not branches.get("working") or not branches.get("target"):
        fail("impl.md has no Branch Strategy with a working branch and merge target", 1)
    numbers = sorted(int(t.id.split("-")[1]) for t in phase.tasks)
    warnings: list[str] = []
    delivery_old: Path | None = None
    delivery_text = ""
    if deliveries:
        last_id = phase.tasks[-1].id
        if len(deliveries) != 1 or deliveries[0].id != last_id or deliveries[0].file is None:
            fail("the delivery task is missing, duplicated, or not last; run lint and fix the phase first", 1)
        new_num = int(last_id.split("-")[1])
        delivery_old = directory / f"{last_id}.md"
        delivery_text = delivery_old.read_text(encoding="utf-8")
        heading = f"# {last_id}:"
        if not delivery_text.startswith(heading):
            fail(f"{last_id}.md does not start with '{heading}'", 1)
        delivery_new_id: str | None = f"t-{new_num + 1:03d}"
        delivery_text = f"# {delivery_new_id}:" + delivery_text[len(heading):]
    else:
        new_num, delivery_new_id = (numbers[-1] if numbers else 0) + 1, None
        warnings.append("phase has no delivery task; add one with the delivery asset")
    new_id = f"t-{new_num:03d}"
    impl_path = directory / "impl.md"
    new_impl = insert_task_lines(
        impl_path.read_text(encoding="utf-8"), deliveries[0].id if deliveries else None,
        new_id, delivery_new_id, title, args.type,
    )
    if new_impl is None:
        fail("impl.md Timeline or Tasks has no entry to insert next to", 1)
    task_text = render("task.md", {
        "task_id": new_id, "title": title, "type": args.type,
        "paths": "\n".join(f"- `{p}`" for p in paths)
        or "- [Files or directories this task will change; used to detect conflicts between tasks]",
        "depends": "\n".join(f"- {d}" for d in depends) or "- None",
        "branch": str(branches["working"]), "target": str(branches["target"]),
    })
    if delivery_new_id is not None:
        write_atomic(directory / f"{delivery_new_id}.md", delivery_text)
    write_atomic(directory / f"{new_id}.md", task_text)
    write_atomic(impl_path, new_impl)
    emit({
        "task": new_id, "file": f"specs/{directory.name}/{new_id}.md", "delivery": delivery_new_id,
        "warnings": warnings,
    })


def strip_code(text: str) -> str:
    return INLINE_CODE_RE.sub("", FENCED_RE.sub("", text))


def asset_placeholders() -> set[str]:
    found: set[str] = set()
    sources = [*assets_dir().glob("*.md"), sibling_assets("spec-implement") / "task-metadata.md"]
    for asset in sources:
        if asset.is_file():
            found.update(p for p in PLACEHOLDER_RE.findall(asset.read_text(encoding="utf-8"))
                         if p not in ("[ ]", "[x]"))
    return found


def has_section(sections: dict[str, list[str]], names: tuple[str, ...]) -> bool:
    return any(key.startswith(name) for key in sections for name in names)


def normalize_title(value: str | None) -> str:
    unannotated = re.sub(r"\s*\([^()]*\)\s*$", "", value or "")
    return re.sub(r"[\s`*_]+", " ", unannotated).strip().lower()


def cmd_lint(args: argparse.Namespace) -> None:
    specs = find_specs(args.root)
    directory, _ = resolve_dir(specs, check_phase_arg(args.phase))
    if LEGACY_DIR_RE.match(directory.name):
        fail("lint supports p-### phases only; legacy phases are read-only")
    phase = parse_phase(directory, specs)
    problems: list[dict[str, str]] = []

    def problem(file: str, code: str, message: str) -> None:
        problems.append({"file": file, "code": code, "message": message})

    placeholders = asset_placeholders() if (assets_dir()).is_dir() else set()
    texts: dict[str, str] = {}
    for md in sorted(directory.glob("*.md")):
        if md.is_file() and md.resolve().is_relative_to(specs):
            warn: list[str] = []
            texts[md.name] = read_text(md, warn) or ""
    impl_text = texts.get("impl.md")
    if impl_text is None:
        problem("impl.md", "missing-section", "impl.md is missing")
    else:
        _, impl_sections = split_sections(impl_text)
        for name in IMPL_SECTIONS:
            if not has_section(impl_sections, (name,)):
                problem("impl.md", "missing-section", f"missing section: {name.title()}")
    by_id = {t.id: t for t in phase.tasks}
    for task in phase.tasks:
        name = f"{task.id}.md"
        if task.file is None:
            problem("impl.md", "task-file-missing", f"{task.id} is listed in impl.md but {name} does not exist")
            continue
        if task.in_impl_checked is None and impl_text is not None and parse_impl_lists(
            section(split_sections(impl_text)[1], "tasks"),
        ):
            problem(name, "task-not-in-impl", f"{name} is not listed in impl.md Tasks")
        _, sections = split_sections(texts.get(name, ""))
        required = DELIVERY_SECTIONS if task.type == DELIVERY else TASK_SECTIONS
        for names in required:
            if not has_section(sections, names):
                problem(name, "missing-section", f"missing section: {names[0].title()}")
        if task.in_impl_checked is not None and task.in_impl_checked != (task.status == "Done"):
            problem(name, "status-mismatch",
                    f"impl.md Tasks marks {task.id} {'done' if task.in_impl_checked else 'not done'}, task status is {task.status}")
        if task.status_source == "status-section" and task.timeline_status and task.timeline_status != task.status:
            problem(name, "status-mismatch",
                    f"impl.md Timeline says {task.timeline_status}, task status is {task.status}")
    if impl_text is not None:
        listed = parse_impl_lists(section(split_sections(impl_text)[1], "tasks"))
        for task_id, entry in listed.items():
            listed_task = by_id.get(task_id)
            if listed_task and listed_task.title and normalize_title(entry["title"]) != normalize_title(listed_task.title):
                problem(f"{task_id}.md", "title-mismatch",
                        f"impl.md lists '{entry['title']}', the file says '{listed_task.title}'")
    deliveries = [t for t in phase.tasks if t.type == DELIVERY]
    if not deliveries:
        problem("impl.md", "delivery-missing", "the phase has no delivery task")
    elif len(deliveries) > 1:
        problem("impl.md", "multiple-deliveries", f"delivery tasks: {', '.join(t.id for t in deliveries)}")
    elif phase.tasks[-1].id != deliveries[0].id:
        problem(f"{deliveries[0].id}.md", "delivery-not-last",
                f"{deliveries[0].id} is the delivery task but {phase.tasks[-1].id} comes after it")
    for name, text in texts.items():
        visible = strip_code(text)
        fields = sorted(set(FIELD_RE.findall(visible)))
        if fields:
            problem(name, "unrendered-field", f"unrendered template fields: {', '.join(fields)}")
        left = sorted(p for p in placeholders if p in visible)
        if left:
            problem(name, "placeholder", f"unfilled placeholders: {', '.join(left[:6])}{' …' if len(left) > 6 else ''}")
    emit({"phase": directory.name, "ok": not problems, "problems": problems})
    if problems:
        sys.exit(1)


def load_task(args: argparse.Namespace) -> tuple[Path, Path, Phase, Task]:
    specs = find_specs(args.root)
    directory, _ = resolve_dir(specs, check_phase_arg(args.phase))
    if LEGACY_DIR_RE.match(directory.name):
        fail("legacy phases are read-only")
    if not re.match(r"^t-\d{3}$", args.task):
        fail(f"invalid task argument: {args.task!r}")
    phase = parse_phase(directory, specs)
    task = next((t for t in phase.tasks if t.id == args.task), None)
    if task is None:
        fail(f"task not found: {args.task}", 1, valid=[t.id for t in phase.tasks])
    if task.file is None:
        fail(f"{args.task} has no task file", 1)
    return specs, directory, phase, task


def status_line(status: str) -> str:
    return f"- [{'x' if status == 'Done' else ' '}] {status}"


def heading_lines(lines: list[str]) -> list[tuple[int, str]]:
    found: list[tuple[int, str]] = []
    fence: str | None = None
    for index, line in enumerate(lines):
        match = FENCE_RE.match(line)
        if match:
            marker = match.group(1)
            if fence is None:
                fence = marker[0] * len(marker)
            elif marker.startswith(fence):
                fence = None
        elif fence is None and line.startswith("## "):
            found.append((index, line[3:].strip().lower()))
    return found


def replace_status_section(text: str, status: str) -> str:
    lines = text.split("\n")
    headings = heading_lines(lines)
    for position, (index, name) in enumerate(headings):
        if not name.startswith("status"):
            continue
        end = headings[position + 1][0] if position + 1 < len(headings) else len(lines)
        for line_index in range(index + 1, end):
            if CHECKBOX_RE.match(lines[line_index]):
                lines[line_index] = status_line(status)
                return "\n".join(lines)
        lines.insert(index + 1, status_line(status))
        return "\n".join(lines)
    return text.rstrip("\n") + f"\n\n## Status\n{status_line(status)}\n"


def update_impl_status(impl: str, task_id: str, status: str) -> str | None:
    timeline = re.compile(rf"^(- \*\*{task_id}\*\*:.*?)(?:\s*\([^()]*\))?\s*$", re.MULTILINE)
    tasks = re.compile(rf"^- \[[ xX]\] ({task_id}:.*)$", re.MULTILINE)
    if not timeline.search(impl) or not tasks.search(impl):
        return None
    impl = timeline.sub(lambda m: f"{m.group(1)} ({status})", impl, count=1)
    return tasks.sub(lambda m: f"- [{'x' if status == 'Done' else ' '}] {m.group(1)}", impl, count=1)


def done_gaps(task: Task) -> list[str]:
    gaps = []
    required = [("objectives", task.objectives)]
    if task.type != DELIVERY:
        required += [("testing", task.testing), ("security", task.security)]
    for name, checklist_ in required:
        if checklist_ is None or checklist_.total == 0 or checklist_.checked != checklist_.total:
            gaps.append(name)
    return gaps


def cmd_set_status(args: argparse.Namespace) -> None:
    if args.status not in STATUSES:
        fail(f"invalid status: choose one of {', '.join(STATUSES)}")
    specs, directory, _, task = load_task(args)
    if args.status == "Done":
        gaps = done_gaps(task)
        if gaps:
            fail(f"{task.id} cannot be Done: finish {', '.join(gaps)} first", 1, missing=gaps)
    task_path, impl_path = directory / f"{task.id}.md", directory / "impl.md"
    task_text = task_path.read_text(encoding="utf-8")
    impl_text = impl_path.read_text(encoding="utf-8")
    new_task = replace_status_section(task_text, args.status)
    new_impl = update_impl_status(impl_text, task.id, args.status)
    if new_impl is None:
        fail(f"impl.md has no Timeline or Tasks entry for {task.id}", 1)
    files = []
    if new_task != task_text:
        write_atomic(task_path, new_task)
        files.append(task_path.name)
    if new_impl != impl_text:
        write_atomic(impl_path, new_impl)
        files.append(impl_path.name)
    emit({"task": task.id, "from": task.status, "to": args.status, "changed": bool(files), "files": files})


def metadata_blocks() -> list[tuple[str, str]]:
    template = read_template(sibling_assets("spec-implement") / "task-metadata.md")
    blocks: list[tuple[str, str]] = []
    for part in re.split(r"(?m)^(?=## )", template):
        if part.startswith("## "):
            blocks.append((part.split("\n", 1)[0][3:].strip(), part.rstrip("\n") + "\n"))
    return blocks


def cmd_normalize(args: argparse.Namespace) -> None:
    _, directory, _, task = load_task(args)
    path = directory / f"{task.id}.md"
    text = path.read_text(encoding="utf-8")
    _, sections = split_sections(text)
    missing = [
        (title, block) for title, block in metadata_blocks()
        if not has_section(sections, METADATA_ALIASES.get(title.lower(), (title.lower(),)))
    ]
    if missing:
        addition = "\n".join(block for _, block in missing)
        lines = text.split("\n")
        status_at = next((i for i, name in heading_lines(lines) if name.startswith("status")), None)
        if status_at is not None:
            text = "\n".join([*lines[:status_at], *(addition + "\n").split("\n")[:-1], *lines[status_at:]])
        else:
            text = text.rstrip("\n") + "\n\n" + addition
        write_atomic(path, text)
    emit({"task": task.id, "added": [title for title, _ in missing], "file": f"specs/{directory.name}/{path.name}"})


def criteria_of(text: str) -> tuple[bool, bool, bool]:
    _, sections = split_sections(text)
    objectives = section(sections, "objectives") or []
    boxes = [m for m in map(CHECKBOX_RE.match, objectives) if m]
    if boxes and any(m.group(1) == " " for m in boxes):
        done = False
    else:
        status_lines = section(sections, "status") or []
        done = (
            (bool(boxes))
            or any((m := CHECKBOX_RE.match(line)) and m.group(1) != " " for line in status_lines)
            or bool(DONE_LINE_RE.search(text))
        )

    def checked_in(*prefixes: str) -> bool:
        lines = section(sections, *prefixes) or []
        return any((m := CHECKBOX_RE.match(line)) and m.group(1) != " " for line in lines)

    status_text = "\n".join(section(sections, "status") or [])
    tested = checked_in("testing", "test results", "tests") or bool(
        TESTED_LINE_RE.search(text) or STATUS_TESTED_RE.search(status_text),
    )
    secure = checked_in("security") or bool(
        SECURITY_LINE_RE.search(text) or STATUS_SECURITY_RE.search(status_text),
    )
    return done, tested, secure


def parse_compile_filter(arg: str | None) -> tuple[str, set[int] | None, str, str | None]:
    if arg is None or arg == "all":
        return "all", None, "all phases", None
    if arg == "completed":
        return "completed", None, "completed filter (all phases scanned)", None
    if arg == "incomplete":
        return "incomplete", None, "incomplete filter (all phases scanned)", None
    if re.match(r"^\d{3}$", arg):
        return "single", {int(arg)}, f"phase {arg}", None
    match = RANGE_RE.match(arg)
    if match and int(match.group(1)) <= int(match.group(2)):
        low, high = int(match.group(1)), int(match.group(2))
        return "range", set(range(low, high + 1)), f"phases {match.group(1)}–{match.group(2)}", None
    shown = arg[:40]
    reason = "invalid range" if match else "unrecognised argument"
    return "all", None, "all phases", f"{reason} {shown!r}; defaulted to all"


MARK = {True: "✅", False: "❌", None: "❓"}


def classify_phase(directory: Path, specs: Path) -> dict[str, Any]:
    phase = parse_phase(directory, specs)
    tasks = sorted(phase.tasks, key=lambda t: int(t.id.split("-")[1]))
    rows: list[dict[str, Any]] = []
    for task in tasks:
        path = directory / f"{task.id}.md"
        warn: list[str] = []
        text = read_text(path, warn) if task.file else None
        flags: tuple[bool | None, bool | None, bool | None] = (None, None, None)
        if text is not None:
            flags = criteria_of(text)
        rows.append({"task": task, "flags": flags, "path": f"specs/{directory.name}/{task.id}.md"})
    complete = bool(rows) and all(all(r["flags"]) for r in rows)
    return {"phase": phase, "rows": rows, "complete": complete}


def missing_reason(label: str, value: bool | None, task: Task, path: str) -> str | None:
    if value:
        return None
    if value is None:
        return f"- [ ] {label} — unknown, `{path}` could not be read"
    reasons = {
        "Done": "objectives not fully checked" if task.objectives and task.objectives.checked < task.objectives.total
        else "no completion signal found",
        "Tested": "no testing section found" if task.testing is None else "no checked testing item",
        "Security Audited": "no security audit section found" if task.security is None
        else "no checked security item",
    }
    return f"- [ ] {label} — {reasons[label]}"


def task_row(row: dict[str, Any]) -> str:
    task: Task = row["task"]
    done, tested, secure = row["flags"]
    title = (task.title or "").replace("|", "\\|")
    return f"| {task.id} | {title} | {MARK[done]} | {MARK[tested]} | {MARK[secure]} |"


def phase_fields(entry: dict[str, Any], incomplete: bool) -> dict[str, str]:
    phase: Phase = entry["phase"]
    rows = entry["rows"]
    ordered = sorted(rows, key=lambda r: all(r["flags"])) if incomplete else rows
    blocks = []
    for row in rows:
        if all(row["flags"]):
            continue
        task: Task = row["task"]
        lines = [m for m in (
            missing_reason(label, value, task, row["path"])
            for label, value in zip(("Done", "Tested", "Security Audited"), row["flags"])
        ) if m]
        blocks.append(f"#### {task.id}: {task.title}\n\nMissing:\n" + "\n".join(lines))
    objectives = "\n".join(f"- {o[:200]}" for o in (phase.objectives or [])[:5]) or "—"
    return {
        "phase_dir": phase.dir, "phase_title": phase.title or phase.dir, "task_count": str(len(rows)),
        "complete_tasks": str(sum(1 for r in rows if all(r["flags"]))),
        "task_rows": "\n".join(task_row(r) for r in ordered) or "| — | no task files | ❌ | ❌ | ❌ |",
        "incomplete_tasks": "\n\n".join(blocks) or "Phase has no task files (only impl.md).",
        "objectives": objectives,
    }


def render_compiled(name: str, header: dict[str, str], entries: list[dict[str, Any]], in_scope: int) -> str:
    template = read_template(sibling_assets("spec-compile") / name)
    incomplete = name == "incomplete.md"

    def conditional(tag: str, keep: bool) -> None:
        nonlocal template
        pattern = re.compile(BLOCK_RE.format(tag=f"if:{tag}"), re.DOTALL)
        template = pattern.sub(lambda m: m.group(1) if keep else "", template)

    phase_block = re.compile(BLOCK_RE.format(tag="phase"), re.DOTALL)
    template = phase_block.sub(
        lambda m: "".join(fill(m.group(1), phase_fields(e, incomplete), name) for e in entries), template,
    )
    conditional("phases", bool(entries))
    conditional("empty", not entries and in_scope > 0)
    conditional("no-scope", in_scope == 0)
    return fill(template, header, name)


def cmd_compile(args: argparse.Namespace) -> None:
    specs = find_specs(args.root)
    mode, numbers, scope, note = parse_compile_filter(args.filter)
    date = args.date or datetime.date.today().isoformat()
    if not DATE_RE.match(date):
        fail("invalid --date: use YYYY-MM-DD")
    dirs = [d for d in phase_dirs(specs) if numbers is None or phase_number(d.name) in numbers]
    warnings: list[str] = []
    if numbers is not None:
        absent = sorted(n for n in numbers if all(phase_number(d.name) != n for d in dirs))
        if absent:
            warnings.append("no phase directory for: " + ", ".join(f"{n:03d}" for n in absent))
    classified = [classify_phase(d, specs) for d in dirs]
    done_entries = [c for c in classified if c["complete"]]
    todo_entries = [c for c in classified if not c["complete"]]
    wanted = {"completed": ["completed.md"], "incomplete": ["incomplete.md"]}.get(mode, ["completed.md", "incomplete.md"])
    outputs: dict[str, str] = {}
    for name in wanted:
        entries = done_entries if name == "completed.md" else todo_entries
        header = {"date": date, "scope": scope, "count": str(len(entries)), "total": str(len(classified))}
        outputs[name] = render_compiled(name, header, entries, len(classified))
    target = specs / "compiled"
    if target.is_symlink() or (target.exists() and not target.is_dir()):
        fail("specs/compiled must be a real directory, not a link or file", 1)
    if mode == "all" and target.is_dir():
        shutil.rmtree(target)
    target.mkdir(exist_ok=True)
    for name in wanted:
        stale = target / name
        if stale.exists() or stale.is_symlink():
            stale.unlink()
        write_atomic(stale, outputs[name])
    all_rows = [r for c in classified for r in c["rows"]]
    emit({
        "filter": mode, "scope": scope, "written": wanted, "note": note, "warnings": warnings,
        "phases": {"in_scope": len(classified), "complete": len(done_entries), "incomplete": len(todo_entries)},
        "tasks": {"in_scope": len(all_rows), "complete": sum(1 for r in all_rows if all(r["flags"]))},
    })


def footprint(paths: list[str] | None) -> list[str] | None:
    if not paths:
        return None
    cleaned = []
    for raw in paths:
        path = raw.strip()
        while path.startswith("./"):
            path = path[2:]
        for index, char in enumerate(path):
            if char in "*?[":
                path = path[:index].rpartition("/")[0]
                break
        cleaned.append(path.rstrip("/"))
    return cleaned


def hot_match(a: str, b: str) -> bool:
    for hot in HOT_FILES:
        in_a = a == hot or a.endswith("/" + hot)
        in_b = b == hot or b.endswith("/" + hot)
        if in_a and in_b:
            return True
    return False


def overlapping_path(a: list[str], b: list[str]) -> str | None:
    best: str | None = None
    for x in a:
        for y in b:
            hit = x == y or x == "" or y == "" or x.startswith(y + "/") or y.startswith(x + "/") or hot_match(x, y)
            if hit:
                candidate = max(x, y, key=len) or "."
                best = candidate if best is None or len(candidate) > len(best) else best
    return best


def claimed_tasks(phase_dir: str, warnings: list[str]) -> set[str]:
    coord = Path(__file__).resolve().parents[2] / "agent-coordination/scripts/coord.py"
    if not coord.is_file():
        return set()
    try:
        proc = subprocess.run(
            [sys.executable, str(coord), "claims"], capture_output=True, text=True, timeout=10, check=True,
        )
        claims = json.loads(proc.stdout)["claims"]
    except (OSError, subprocess.SubprocessError, ValueError, KeyError):
        warnings.append("coordination unavailable; claimed tasks are not marked running")
        return set()
    return {
        str(c["task"]) for c in claims
        if isinstance(c, dict) and c.get("phase") == phase_dir and TASK_ID_RE.match(str(c.get("task", "")))
    }


def reach(edges: dict[str, set[str]], start: str) -> set[str]:
    seen: set[str] = set()
    stack = list(edges.get(start, ()))
    while stack:
        node = stack.pop()
        if node not in seen:
            seen.add(node)
            stack.extend(edges.get(node, ()))
    return seen


def find_cycle(edges: dict[str, set[str]]) -> list[str] | None:
    state: dict[str, int] = {}
    trail: list[str] = []

    def visit(node: str) -> list[str] | None:
        state[node] = 1
        trail.append(node)
        for nxt in sorted(edges.get(node, ())):
            if state.get(nxt) == 1:
                return trail[trail.index(nxt):] + [nxt]
            if state.get(nxt) is None:
                found = visit(nxt)
                if found:
                    return found
        trail.pop()
        state[node] = 2
        return None

    for node in sorted(edges):
        if state.get(node) is None:
            found = visit(node)
            if found:
                return found
    return None


def plan_waves(
    phase: Phase, max_parallel: int, cleared: list[str], extra_done: set[str], extra_running: set[str],
    claimed: set[str],
) -> dict[str, Any]:
    by_id = {t.id: t for t in phase.tasks}
    ids = sorted(by_id)
    done = {i for i in ids if by_id[i].status == "Done"} | (extra_done & set(ids))
    running = ({i for i in ids if by_id[i].status == "In Progress"} | extra_running | claimed) & set(ids) - done
    blocked = {i for i in ids if by_id[i].status == "Blocked" and i not in done | running}
    in_review = {i for i in ids if by_id[i].status == "In Review" and i not in done | running}
    pending = [i for i in ids if i not in done | running | blocked | in_review]
    delivery = next((i for i in ids if by_id[i].type == DELIVERY), None)
    held: dict[str, list[str]] = {}
    changed = True
    while changed:
        changed = False
        for task_id in pending:
            if task_id in held:
                continue
            why_held: list[str] = []
            for dep in by_id[task_id].depends_on:
                if dep not in by_id:
                    why_held.append(f"depends:{dep} (unknown task)")
                elif dep in blocked:
                    why_held.append(f"depends:{dep} (blocked)")
                elif dep in in_review:
                    why_held.append(f"depends:{dep} (in review)")
                elif dep in held:
                    why_held.append(f"depends:{dep} (held)")
            if task_id == delivery:
                why_held += [f"depends:{i} ({'blocked' if i in blocked else 'in review' if i in in_review else 'held'})"
                            for i in ids if i != delivery and (i in blocked or i in in_review or i in held)]
            if why_held:
                held[task_id] = why_held
                changed = True
    nodes = [i for i in ids if i in running or (i in pending and i not in held)]
    dep_edges: dict[str, set[str]] = {n: set() for n in nodes}
    for n in nodes:
        for dep in by_id[n].depends_on:
            if dep in dep_edges:
                dep_edges[dep].add(n)
        if n == delivery:
            for other in nodes:
                if other != delivery:
                    dep_edges[other].add(n)
    cycle = find_cycle(dep_edges)
    if cycle:
        fail("dependency cycle: " + " -> ".join(cycle), 1, cycle=cycle[:-1])
    topo: list[str] = []
    indegree = {n: 0 for n in nodes}
    for src in nodes:
        for dst in dep_edges[src]:
            indegree[dst] += 1
    available = sorted((n for n in nodes if indegree[n] == 0), key=lambda n: (n not in running, n))
    while available:
        node = available.pop(0)
        topo.append(node)
        for dst in sorted(dep_edges[node]):
            indegree[dst] -= 1
            if indegree[dst] == 0:
                available.append(dst)
        available.sort(key=lambda n: (n not in running, n))
    position = {n: i for i, n in enumerate(topo)}
    reachable = {n: reach(dep_edges, n) for n in nodes}
    feet = {n: footprint(by_id[n].affected_paths) for n in nodes}
    edges = {n: set(dep_edges[n]) for n in nodes}
    conflict_with: dict[str, dict[str, str]] = {n: {} for n in nodes}
    for a in nodes:
        for b in nodes:
            if position[a] >= position[b] or b in reachable[a] or a in reachable[b]:
                continue
            fa, fb = feet[a], feet[b]
            if fa is None or fb is None:
                reason = f"unknown-footprint:{a if fa is None else b}"
            else:
                path = overlapping_path(fa, fb)
                if path is None:
                    continue
                reason = f"paths:{path}"
            edges[a].add(b)
            conflict_with[b][a] = reason
    level: dict[str, int] = {}
    preds: dict[str, set[str]] = {n: set() for n in nodes}
    for src in nodes:
        for dst in edges[src]:
            preds[dst].add(src)
    for n in topo:
        level[n] = 1 + max((level[p] for p in preds[n]), default=0)
    ordered_levels: dict[int, list[str]] = {}
    for n in topo:
        ordered_levels.setdefault(level[n], []).append(n)
    wave_lists: list[list[str]] = []
    for lvl in sorted(ordered_levels):
        members = sorted(ordered_levels[lvl], key=lambda n: (n not in running, n))
        wave_lists += [members[i:i + max_parallel] for i in range(0, len(members), max_parallel)]
    wave_of = {n: index + 1 for index, members in enumerate(wave_lists) for n in members}
    gates = []
    gated: set[str] = set()
    for n in nodes:
        for gate in by_id[n].external_gates:
            if n in pending and not any(c.lower() in gate.lower() for c in cleared if c):
                gates.append({"task": n, "gate": gate})
                gated.add(n)
    tasks: dict[str, dict[str, Any]] = {}
    for i in ids:
        task = by_id[i]
        reasons: list[str] = []
        conflicts: list[str] = []
        if i in done:
            state = "done"
        elif i in running:
            state = "running"
        elif i in blocked:
            state = "blocked"
        elif i in in_review:
            state = "in-review"
        elif i in held:
            state, reasons = "waiting", list(held[i])
        else:
            reasons += [f"depends:{d}" for d in task.depends_on if d in nodes]
            if i == delivery:
                reasons.append("delivery:last")
            for pred, why in sorted(conflict_with[i].items()):
                conflicts.append(pred)
                reasons.append(why)
            if feet[i] is None and i != delivery:
                reasons.append("unknown-footprint")
            reasons += [f"gate:{g['gate']}" for g in gates if g["task"] == i]
            has_preds = bool(preds[i])
            state = "gated" if i in gated else ("waiting" if has_preds else "ready")
        tasks[i] = {
            "title": task.title, "status": task.status, "state": state, "wave": wave_of.get(i),
            "reasons": list(dict.fromkeys(reasons)), "conflicts_with": conflicts,
        }
    return {
        "waves": wave_lists, "order": [n for w in wave_lists for n in w], "tasks": tasks, "gates": gates,
        "held": sorted(held), "max_parallel": max_parallel, "warnings": [],
    }


def waves_section(plan: dict[str, Any], phase_dir: str) -> str:
    rows = []
    for index, wave in enumerate(plan["waves"], 1):
        why = " · ".join(f"{t}: {', '.join(plan['tasks'][t]['reasons'])}" for t in wave if plan["tasks"][t]["reasons"])
        rows.append(f"| {index} | {' · '.join(wave)} | {(why or 'ready now').replace('|', chr(92) + '|')} |")
    held = plan["held"]
    footer = f"\nHeld back by a blocked or in-review prerequisite: {', '.join(held)}.\n" if held else ""
    return (
        "## Parallel Waves\n\nGenerated by `specs.py waves <phase> --write`; regenerate after changing tasks, "
        "dependencies, or paths. Tasks in one wave can run side by side.\n\n"
        "| Wave | Tasks | Why the rest wait |\n|---|---|---|\n" + "\n".join(rows) + "\n" + footer + "\n"
    )


def write_waves_section(impl_path: Path, section_text: str) -> None:
    lines = impl_path.read_text(encoding="utf-8").split("\n")
    headings = heading_lines(lines)
    start = next((i for i, name in headings if name == "parallel waves"), None)
    block = section_text.rstrip("\n").split("\n") + [""]
    if start is not None:
        end = next((i for i, _ in headings if i > start), len(lines))
        lines[start:end] = block
    else:
        anchor = next((i for i, name in headings if name.startswith("open questions")), None)
        if anchor is None:
            lines = [*lines[:-1], *block] if lines and lines[-1] == "" else [*lines, "", *block]
        else:
            lines[anchor:anchor] = block
    write_atomic(impl_path, "\n".join(lines))


def cmd_waves(args: argparse.Namespace) -> None:
    specs = find_specs(args.root)
    directory, _ = resolve_dir(specs, check_phase_arg(args.phase))
    if LEGACY_DIR_RE.match(directory.name):
        fail("waves supports p-### phases only; legacy phases are read-only")
    if not 1 <= args.max_parallel <= MAX_PARALLEL:
        fail(f"--max-parallel must be 1..{MAX_PARALLEL}")
    for value in [*(args.done or []), *(args.running or [])]:
        if not re.match(r"^t-\d{3}$", value):
            fail(f"invalid task id: {value!r}")
    phase = parse_phase(directory, specs)
    warnings: list[str] = []
    claimed = claimed_tasks(directory.name, warnings)
    plan = plan_waves(
        phase, args.max_parallel, args.cleared or [], set(args.done or []), set(args.running or []), claimed,
    )
    plan["warnings"] = warnings
    if args.write:
        write_waves_section(directory / "impl.md", waves_section(plan, directory.name))
    emit({"phase": directory.name, **plan})


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Spec tooling for planners and implementers")
    parser.add_argument("--root", help="workspace root containing specs/ (default: nearest parent of cwd)")
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("list").add_argument("--json", action="store_true")
    show = sub.add_parser("show")
    show.add_argument("phase")
    show.add_argument("--task")
    sub.add_parser("next-phase")
    sub.add_parser("next-task").add_argument("phase")
    sub.add_parser("resolve").add_argument("phase", nargs="?")
    new_phase = sub.add_parser("new-phase")
    new_phase.add_argument("--repo")
    new_phase.add_argument("--multi", action="store_true")
    new_phase.add_argument("--title", required=True)
    new_phase.add_argument("--branch", required=True)
    new_phase.add_argument("--target", required=True)
    new_phase.add_argument("--pattern")
    new_phase.add_argument("--pattern-source")
    new_task = sub.add_parser("new-task")
    new_task.add_argument("phase")
    new_task.add_argument("--title", required=True)
    new_task.add_argument("--type", required=True)
    new_task.add_argument("--depends", nargs="*")
    new_task.add_argument("--paths", nargs="*")
    sub.add_parser("lint").add_argument("phase")
    set_status = sub.add_parser("set-status")
    set_status.add_argument("phase")
    set_status.add_argument("task")
    set_status.add_argument("status")
    normalize = sub.add_parser("normalize")
    normalize.add_argument("phase")
    normalize.add_argument("task")
    waves = sub.add_parser("waves")
    waves.add_argument("phase")
    waves.add_argument("--max-parallel", type=int, default=DEFAULT_PARALLEL)
    waves.add_argument("--cleared", action="append")
    waves.add_argument("--done", nargs="*")
    waves.add_argument("--running", nargs="*")
    waves.add_argument("--write", action="store_true")
    compile_parser = sub.add_parser("compile")
    compile_parser.add_argument("filter", nargs="?")
    compile_parser.add_argument("--date")
    return parser


COMMANDS = {
    "list": cmd_list, "show": cmd_show, "next-phase": cmd_next_phase,
    "next-task": cmd_next_task, "resolve": cmd_resolve, "new-phase": cmd_new_phase,
    "new-task": cmd_new_task, "lint": cmd_lint, "set-status": cmd_set_status, "normalize": cmd_normalize,
    "compile": cmd_compile, "waves": cmd_waves,
}


def main() -> None:
    args = build_parser().parse_args()
    COMMANDS[args.command](args)


if __name__ == "__main__":
    main()
