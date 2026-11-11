#!/usr/bin/env python3
"""Symlink one directory into codebase-symlinks/ and refresh the codebase map."""

from __future__ import annotations

import argparse
import contextlib
import importlib.util
import json
import os
import re
import sys
from pathlib import Path
from types import ModuleType
from typing import Any

LINK_DIR = "codebase-symlinks"
NAME_PATTERN = re.compile(r"^[A-Za-z0-9._-]+$")
LINKER_PATH = Path(__file__).resolve().parents[2] / "workspace-init" / "scripts" / "link_codebases.py"


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=f"Symlink a directory into {LINK_DIR}/ and refresh knowledgebase/context-history/codebase-map.md."
    )
    parser.add_argument("path", nargs="?", help="Directory to link. Relative paths resolve against the current directory.")
    parser.add_argument("--name", help="Link name inside codebase-symlinks/. Defaults to the directory name.")
    parser.add_argument("--root", default=".", help="Workspace root. Defaults to the current directory.")
    parser.add_argument("--siblings", action="store_true", help="List sibling directories of the workspace and whether each is linked.")
    parser.add_argument("--dry-run", action="store_true", help="Validate and report without creating the link or touching the map.")
    return parser.parse_args(argv)


def emit(payload: dict[str, Any], code: int = 0) -> int:
    print(json.dumps(payload, indent=2))
    return code


def refuse(reason: str, **extra: Any) -> int:
    return emit({"status": "refused", "reason": reason, **extra}, 1)


def load_linker() -> ModuleType | None:
    if not LINKER_PATH.is_file():
        return None
    spec = importlib.util.spec_from_file_location("link_codebases", LINKER_PATH)
    if spec is None or spec.loader is None:
        return None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def existing_links(link_root: Path) -> dict[str, Path]:
    links: dict[str, Path] = {}
    if not link_root.is_dir():
        return links
    for entry in sorted(link_root.iterdir(), key=lambda item: item.name.lower()):
        if entry.is_symlink():
            links[entry.name] = entry.resolve(strict=False)
    return links


def is_within(child: Path, parent: Path) -> bool:
    return child == parent or parent in child.parents


def refresh_map(linker: ModuleType | None, root: Path, link_root: Path) -> str | None:
    if linker is None:
        return None
    with contextlib.redirect_stdout(sys.stderr):
        map_path: Path = linker.refresh_codebase_map(root, link_root)
    return str(map_path.relative_to(root))


def list_siblings(root: Path, link_root: Path, linker: ModuleType | None) -> int:
    linked = {target: name for name, target in existing_links(link_root).items()}
    siblings = []
    for path in sorted(root.parent.iterdir(), key=lambda item: item.name.lower()):
        try:
            resolved = path.resolve()
            if not path.is_dir() or resolved == root:
                continue
        except OSError:
            continue
        try:
            purpose = linker.summarize_repository(resolved)[0] if linker else path.name
        except OSError:
            purpose = path.name
        siblings.append({"name": path.name, "path": str(resolved), "purpose": purpose, "linked_as": linked.get(resolved)})
    return emit({"status": "ok", "workspace": str(root), "siblings": siblings})


def link(args: argparse.Namespace, root: Path, link_root: Path, linker: ModuleType | None) -> int:
    raw = Path(args.path).expanduser()
    if not raw.exists():
        return refuse(f"Path does not exist: {raw}")
    target = raw.resolve()
    if not target.is_dir():
        return refuse(f"Not a directory: {target}")
    if target == root:
        return refuse("Refusing to link the workspace into itself.", target=str(target))
    if is_within(root, target):
        return refuse("Refusing to link a parent of the workspace; it would create a symlink loop.", target=str(target))
    if is_within(target, root):
        return refuse("Directory is already inside the workspace; no link needed.", target=str(target))

    name = args.name or target.name
    if name in {".", ".."} or not NAME_PATTERN.match(name):
        return refuse(f"Invalid link name {name!r}: use letters, digits, '.', '_' or '-'.")

    dest = link_root / name
    rel_dest = f"{LINK_DIR}/{name}"
    for other, other_target in existing_links(link_root).items():
        if other_target == target and other != name:
            return emit({"status": "already-linked", "link": f"{LINK_DIR}/{other}", "target": str(target)})

    if os.path.lexists(dest):
        if dest.is_symlink() and dest.resolve(strict=False) == target:
            map_rel = None if args.dry_run else refresh_map(linker, root, link_root)
            return emit({"status": "preserved", "link": rel_dest, "target": str(target), "map": map_rel})
        current = os.readlink(dest) if dest.is_symlink() else "a non-symlink path"
        return refuse(f"{rel_dest} already exists and points to {current}; choose another --name.", link=rel_dest)

    relative_target = os.path.relpath(target, link_root)
    if args.dry_run:
        return emit({"status": "would-create", "link": rel_dest, "target": str(target), "relative_target": relative_target})

    link_root.mkdir(parents=True, exist_ok=True)
    dest.symlink_to(relative_target, target_is_directory=True)
    map_rel = refresh_map(linker, root, link_root)
    payload: dict[str, Any] = {"status": "created", "link": rel_dest, "target": str(target), "relative_target": relative_target, "map": map_rel}
    if map_rel is None:
        payload["warning"] = f"Codebase map not refreshed: {LINKER_PATH} is missing."
    return emit(payload)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    root = Path(args.root).expanduser().resolve()
    if not root.is_dir():
        return refuse(f"Workspace root not found: {root}")
    link_root = root / LINK_DIR
    if link_root.is_symlink():
        return refuse(f"Refusing to operate on a symlinked {LINK_DIR}/ directory.")
    linker = load_linker()

    if args.siblings:
        return list_siblings(root, link_root, linker)
    if not args.path:
        return refuse("Give the directory to link, or --siblings to list candidates.")
    return link(args, root, link_root, linker)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
