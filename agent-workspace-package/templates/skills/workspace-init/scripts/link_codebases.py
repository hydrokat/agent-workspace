#!/usr/bin/env python3
"""Link sibling codebases into codebase-symlinks/ and refresh the codebase map."""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from dataclasses import dataclass
from pathlib import Path

CELL_TEXT_LIMIT = 120


@dataclass(frozen=True)
class Candidate:
    name: str
    path: Path
    summary: str
    source_note: str


@dataclass(frozen=True)
class LinkedCodebase:
    name: str
    source: Path
    symlink: Path
    purpose: str
    source_note: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Interactively link sibling codebases one directory above the workspace "
            "into codebase-symlinks/ and regenerate the codebase map."
        )
    )
    parser.add_argument(
        "workspace",
        nargs="?",
        default=".",
        help="Workspace root directory. Defaults to the current directory.",
    )
    return parser.parse_args()


def normalize_text(value: str, limit: int | None = None) -> str:
    cleaned = " ".join(value.split()).replace("|", "&#124;")
    if limit is not None and len(cleaned) > limit:
        cleaned = cleaned[: max(0, limit - 3)].rstrip() + "..."
    return cleaned


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8", errors="ignore")
    except OSError:
        return ""


def extract_readme_summary(path: Path) -> str:
    text = read_text(path)
    if not text:
        return ""

    lines = text.splitlines()
    paragraph: list[str] = []
    saw_heading = False

    for raw_line in lines:
        line = raw_line.strip()
        if not line:
            if paragraph:
                break
            continue
        if line.startswith("#"):
            saw_heading = True
            continue
        if saw_heading:
            paragraph.append(line)

    if paragraph:
        return normalize_text(" ".join(paragraph), CELL_TEXT_LIMIT)

    headings = [line.lstrip("#").strip() for line in lines if line.strip().startswith("#")]
    for heading in headings:
        if heading:
            return normalize_text(heading, CELL_TEXT_LIMIT)

    nonempty = [line.strip() for line in lines if line.strip()]
    if nonempty:
        return normalize_text(nonempty[0], CELL_TEXT_LIMIT)

    return ""


def extract_json_description(path: Path) -> str:
    try:
        data = json.loads(read_text(path))
    except json.JSONDecodeError:
        return ""

    for key in ("description", "name"):
        value = data.get(key)
        if isinstance(value, str) and value.strip():
            return normalize_text(value.strip(), CELL_TEXT_LIMIT)

    return ""


def extract_toml_field(path: Path, field: str) -> str:
    pattern = re.compile(rf'^\s*{re.escape(field)}\s*=\s*["\'](.+?)["\']\s*$')
    for raw_line in read_text(path).splitlines():
        match = pattern.match(raw_line)
        if match:
            return normalize_text(match.group(1).strip(), CELL_TEXT_LIMIT)
    return ""


def summarize_repository(path: Path) -> tuple[str, str]:
    if not path.exists():
        return "Target missing", "missing target"

    readme = path / "README.md"
    if readme.is_file():
        summary = extract_readme_summary(readme)
        if summary:
            return summary, "README.md"

    package_json = path / "package.json"
    if package_json.is_file():
        summary = extract_json_description(package_json)
        if summary:
            return summary, "package.json"

    pyproject = path / "pyproject.toml"
    if pyproject.is_file():
        summary = extract_toml_field(pyproject, "description") or extract_toml_field(
            pyproject, "name"
        )
        if summary:
            return summary, "pyproject.toml"

    cargo = path / "Cargo.toml"
    if cargo.is_file():
        summary = extract_toml_field(cargo, "description") or extract_toml_field(
            cargo, "name"
        )
        if summary:
            return summary, "Cargo.toml"

    return normalize_text(path.name, CELL_TEXT_LIMIT), "directory name"


def discover_sibling_codebases(workspace_root: Path) -> list[Candidate]:
    source_parent = workspace_root.parent
    candidates: list[Candidate] = []
    workspace_resolved = workspace_root.resolve()

    for path in sorted(source_parent.iterdir(), key=lambda item: item.name.lower()):
        if not path.is_dir():
            continue

        try:
            if path.resolve() == workspace_resolved:
                continue
        except OSError:
            continue

        summary, source_note = summarize_repository(path)
        candidates.append(
            Candidate(
                name=path.name,
                path=path.resolve(),
                summary=summary,
                source_note=source_note,
            )
        )

    return candidates


def print_candidates(candidates: list[Candidate], source_parent: Path) -> None:
    print(f"Scanning sibling directories in: {source_parent}")
    print("Available codebases:")
    for index, candidate in enumerate(candidates, start=1):
        print(f"  {index}. {candidate.name}")
        print(f"     Purpose: {candidate.summary}")
        print(f"     Source: {candidate.source_note}")


def prompt_selection(candidates: list[Candidate]) -> list[Candidate]:
    while True:
        raw = input(
            "Select codebases to symlink (all, comma-separated numbers, or q to quit): "
        ).strip()

        if not raw:
            print("Please choose at least one codebase, or type q to quit.")
            continue

        lowered = raw.lower()
        if lowered in {"q", "quit", "exit"}:
            return []

        if lowered in {"a", "all", "*"}:
            return candidates

        chosen_indexes: list[int] = []
        tokens = [token for token in re.split(r"[,\s]+", raw) if token]
        valid = True
        for token in tokens:
            if not token.isdigit():
                valid = False
                break
            index = int(token)
            if index < 1 or index > len(candidates):
                valid = False
                break
            if index not in chosen_indexes:
                chosen_indexes.append(index)

        if not valid or not chosen_indexes:
            print("Invalid selection. Use numbers, a comma-separated list, or all.")
            continue

        return [candidates[index - 1] for index in chosen_indexes]


def link_codebase(source: Path, destination: Path) -> str:
    destination.parent.mkdir(parents=True, exist_ok=True)

    if destination.exists() or destination.is_symlink():
        if destination.is_symlink():
            try:
                current_target = destination.resolve(strict=False)
            except OSError:
                current_target = None
            if current_target == source.resolve():
                print(f"Preserved existing link: {destination} -> {source}")
                return "preserved"

        print(f"Skipping existing path: {destination}")
        return "skipped"

    relative_target = os.path.relpath(source, destination.parent)
    destination.symlink_to(relative_target, target_is_directory=True)
    print(f"Created symlink: {destination} -> {relative_target}")
    return "created"


def collect_linked_codebases(destination_root: Path) -> list[LinkedCodebase]:
    if not destination_root.exists():
        return []

    linked: list[LinkedCodebase] = []
    for path in sorted(destination_root.iterdir(), key=lambda item: item.name.lower()):
        if not path.is_symlink():
            print(f"Warning: skipping non-symlink entry in {destination_root}: {path.name}")
            continue

        source = path.resolve(strict=False)
        purpose, source_note = summarize_repository(source)
        linked.append(
            LinkedCodebase(
                name=path.name,
                source=source,
                symlink=path,
                purpose=purpose,
                source_note=source_note,
            )
        )

    return linked


def render_codebase_map(
    entries: list[LinkedCodebase], workspace_root: Path, destination_root: Path
) -> str:
    symlink_root = destination_root.relative_to(workspace_root)
    lines = [
        "# Codebase Map",
        "",
        "This note records sibling repositories linked into `codebase-symlinks/` so agents can identify the right codebase before traversing back to the parent project.",
        "",
        f"- Symlink root: `{normalize_text(str(symlink_root), CELL_TEXT_LIMIT)}`",
        "",
        "| Codebase | Symlink | Purpose | Source note |",
        "| --- | --- | --- | --- |",
    ]

    if not entries:
        lines.append("| _None_ | _None_ | No linked codebases recorded yet. | - |")
        lines.extend(
            [
                "",
                "Regenerate this note after linking sibling codebases through the workspace initializer.",
            ]
        )
        return "\n".join(lines) + "\n"

    for entry in entries:
        lines.append(
            "| "
            + " | ".join(
                [
                    f"`{normalize_text(entry.name, CELL_TEXT_LIMIT)}`",
                    f"`{normalize_text(str(entry.symlink.relative_to(workspace_root)), CELL_TEXT_LIMIT)}`",
                    normalize_text(entry.purpose, CELL_TEXT_LIMIT),
                    normalize_text(entry.source_note, CELL_TEXT_LIMIT),
                ]
            )
            + " |"
        )

    lines.extend(
        [
            "",
            "Keep the purpose line short and specific when the linked repos change.",
        ]
    )
    return "\n".join(lines) + "\n"


def refresh_codebase_map(workspace_root: Path, destination_root: Path) -> Path:
    map_path = workspace_root / "knowledgebase" / "context-history" / "codebase-map.md"
    map_path.parent.mkdir(parents=True, exist_ok=True)

    entries = collect_linked_codebases(destination_root)
    map_path.write_text(
        render_codebase_map(entries, workspace_root, destination_root), encoding="utf-8"
    )
    return map_path


def main() -> int:
    args = parse_args()
    workspace_root = Path(args.workspace).expanduser().resolve()

    if not workspace_root.is_dir():
        print(f"Error: workspace directory not found: {workspace_root}", file=sys.stderr)
        return 1

    source_parent = workspace_root.parent
    destination_root = workspace_root / "codebase-symlinks"

    candidates = discover_sibling_codebases(workspace_root)
    if not candidates:
        print(f"No sibling directories found in {source_parent}.")
        return 0

    print(f"Workspace root: {workspace_root}")
    print(f"Symlink destination: {destination_root}")
    print_candidates(candidates, source_parent)

    try:
        selected = prompt_selection(candidates)
    except KeyboardInterrupt:
        print("\nSelection canceled.")
        return 130
    except EOFError:
        print("\nSelection canceled.")
        return 130

    if not selected:
        print("No codebases selected. Nothing linked.")
        if destination_root.exists():
            map_path = refresh_codebase_map(workspace_root, destination_root)
            print(f"Refreshed codebase map: {map_path}")
        return 0

    created = 0
    preserved = 0
    skipped = 0

    for candidate in selected:
        status = link_codebase(candidate.path, destination_root / candidate.name)
        if status == "created":
            created += 1
        elif status == "preserved":
            preserved += 1
        else:
            skipped += 1

    map_path = refresh_codebase_map(workspace_root, destination_root)

    print(
        "Linking complete: "
        f"{created} created, {preserved} preserved, {skipped} skipped."
    )
    print(f"Updated codebase map: {map_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
