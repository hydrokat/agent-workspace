#!/usr/bin/env python3
"""List files and directories that may not belong in an agent workspace, and delete approved ones.

Stdlib only. JSON on stdout. Exit codes: 0 ok, 1 refused/nothing to delete from, 2 invalid input.

  scan_workspace.py [--root DIR] [--stale-days N]
      {"root", "scanned_at", "counts": {category: n}, "candidates": [{path, kind, category, reason,
       needs_review, modified, size}]}
      Categories: os-editor-cruft, stale-backup, empty-directory, orphaned-generated-output,
      stale-scratch, unrecognized-file. The scan is saved to .agent-workspace/cleanup-scan.json.
      Never listed: tracked unmodified files, specs/, knowledgebase/business-flows/,
      knowledgebase/context-history/, dependency and git directories, symlinked directories.

  scan_workspace.py [--root DIR] --delete PATH...
      Deletes only the given paths, and only if each is in the saved scan, unchanged since the scan,
      inside the root, and not protected (specs/, knowledgebase/ except *.bak files, .git, AGENTS.md).
      Never follows symlinks. {"deleted": [...], "refused": [{path, reason}]}; exit 1 if any refused.
"""
from __future__ import annotations

import argparse
import fnmatch
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, NoReturn

STATE_DIR = ".agent-workspace"
STATE_FILE = "cleanup-scan.json"
DEFAULT_STALE_DAYS = 14
MAX_STATE_BYTES = 20_000_000
SKIP_DIRS = {".git", "node_modules", "codebase-symlinks", STATE_DIR, ".venv", "venv", "__pycache__", ".tox"}
USER_OWNED = ("specs", "knowledgebase/business-flows", "knowledgebase/context-history")
MANAGED_DIRS = {"specs", "knowledgebase", ".agents", ".claude", ".gemini", ".codex", ".tmp", "codebase-symlinks", STATE_DIR}
KNOWN_ROOT = {
    "AGENTS.md", "CLAUDE.md", "GEMINI.md", "WORKFLOW.md", ".gitignore", ".gitattributes", ".editorconfig",
    ".git", ".github", "src", "run.sh",
}
KNOWN_ROOT_PATTERNS = ("README*", "LICENSE*", "CHANGELOG*")
KNOWN_AGENTS_DIR = {"agents", "skills"}
CRUFT_FILES = {".DS_Store", "Thumbs.db", "desktop.ini"}
CRUFT_PATTERNS = ("*.swp", "*.swo", "*~")
CRUFT_DIRS = {".idea", ".vscode"}
GENERATED = (
    (".claude/skills", "skills", False), (".gemini/skills", "skills", False),
    (".claude/agents", "agents", True), (".gemini/agents", "agents", True),
)
CATEGORY_REASONS = {
    "os-editor-cruft": "OS or editor leftover",
    "stale-backup": "backup left behind by `update` or a manual edit",
    "empty-directory": "empty directory not documented in AGENTS.md",
    "orphaned-generated-output": "generated file with no source under .agents/",
    "stale-scratch": "old scratch file not referenced by specs/in-progress.md",
    "unrecognized-file": "not covered by AGENTS.md, .gitignore, or version control",
}


def fail(message: str, code: int = 2, **extra: Any) -> NoReturn:
    print(json.dumps({"error": message, **extra}))
    sys.exit(code)


def emit(data: dict[str, Any]) -> None:
    print(json.dumps(data, ensure_ascii=False))


def git_lines(root: Path, *args: str) -> list[str] | None:
    try:
        out = subprocess.run(
            ["git", "-C", str(root), *args], capture_output=True, text=True, timeout=30, check=True,
        ).stdout
    except (OSError, subprocess.SubprocessError):
        return None
    return [line for line in out.split("\0") if line]


def under(path: str, prefix: str) -> bool:
    return path == prefix or path.startswith(prefix + "/")


def protected_from_scan(rel: str) -> bool:
    return any(under(rel, p) for p in USER_OWNED)


def modified_iso(mtime_ns: int) -> str:
    return datetime.fromtimestamp(mtime_ns / 1e9, tz=timezone.utc).strftime("%Y-%m-%d")


class Workspace:
    def __init__(self, root: Path, stale_days: int) -> None:
        self.root = root
        self.stale_seconds = stale_days * 86400
        tracked = git_lines(root, "ls-files", "-z")
        self.tracked: set[str] | None = set(tracked) if tracked is not None and (root / ".git").exists() else None
        self.tracked_dirs = {str(Path(p).parent.as_posix()) for p in self.tracked or ()}
        self.agents_md = self.read("AGENTS.md")
        self.in_progress = self.read("specs/in-progress.md")
        self.found: dict[str, dict[str, Any]] = {}

    def read(self, rel: str) -> str:
        try:
            return (self.root / rel).read_text(encoding="utf-8", errors="replace")
        except OSError:
            return ""

    def is_tracked(self, rel: str, is_dir: bool) -> bool:
        if self.tracked is None:
            return False
        if is_dir:
            return any(t == rel or t.startswith(rel + "/") for t in self.tracked)
        return rel in self.tracked

    def is_ignored(self, rel: str) -> bool:
        if self.tracked is None:
            return False
        try:
            proc = subprocess.run(
                ["git", "-C", str(self.root), "check-ignore", "-q", "--", rel], capture_output=True, timeout=10,
            )
        except (OSError, subprocess.SubprocessError):
            return False
        return proc.returncode == 0

    def documented(self, rel: str) -> bool:
        name = rel.rsplit("/", 1)[-1]
        text = self.agents_md
        return bool(re.search(rf"(?<![\w.-]){re.escape(rel)}/?(?![\w-])", text)) or bool(
            re.search(rf"(?<![\w.-]){re.escape(name)}/", text) or re.search(rf"`/?{re.escape(name)}/?`", text)
        )

    def add(self, rel: str, category: str, needs_review: bool = False, reason: str | None = None) -> None:
        if rel in self.found or protected_from_scan(rel):
            return
        try:
            stat = (self.root / rel).lstat()
        except OSError:
            return
        is_dir = (self.root / rel).is_dir() and not (self.root / rel).is_symlink()
        self.found[rel] = {
            "path": rel, "kind": "dir" if is_dir else "file", "category": category,
            "reason": reason or CATEGORY_REASONS[category], "needs_review": needs_review,
            "modified": modified_iso(stat.st_mtime_ns), "size": 0 if is_dir else stat.st_size,
            "mtime_ns": stat.st_mtime_ns,
        }

    def walk(self) -> None:
        for current, dirs, files in os.walk(self.root, followlinks=False):
            rel_dir = Path(current).relative_to(self.root).as_posix()
            rel_dir = "" if rel_dir == "." else rel_dir
            keep = []
            for name in sorted(dirs):
                rel = f"{rel_dir}/{name}".lstrip("/")
                if name in SKIP_DIRS or protected_from_scan(rel) or (self.root / rel).is_symlink():
                    continue
                if name in CRUFT_DIRS and not self.is_tracked(rel, True):
                    self.add(rel, "os-editor-cruft")
                    continue
                keep.append(name)
            dirs[:] = keep
            for name in sorted(files):
                rel = f"{rel_dir}/{name}".lstrip("/")
                if protected_from_scan(rel):
                    continue
                if name in CRUFT_FILES or any(fnmatch.fnmatch(name, p) for p in CRUFT_PATTERNS):
                    self.add(rel, "os-editor-cruft")
                elif name.endswith(".bak"):
                    self.add(rel, "stale-backup")
                elif under(rel, ".tmp"):
                    self.check_scratch(rel)

    def check_scratch(self, rel: str) -> None:
        try:
            age = time.time() - (self.root / rel).lstat().st_mtime
        except OSError:
            return
        if age > self.stale_seconds and rel not in self.in_progress:
            self.add(rel, "stale-scratch")

    def check_generated(self) -> None:
        for base, source_kind, is_file in GENERATED:
            directory = self.root / base
            if not directory.is_dir() or directory.is_symlink():
                continue
            for entry in sorted(directory.iterdir()):
                name = entry.name
                if entry.is_symlink() or name.endswith(".bak"):
                    continue
                if is_file:
                    stem = name[:-3] if name.endswith(".md") else None
                    if stem is None or any((self.root / ".agents/agents" / stem / extra).exists() for extra in ("", "AGENT.md")):
                        continue
                    if (self.root / ".agents/agents" / f"{stem}.md").exists():
                        continue
                elif entry.is_dir() and (self.root / ".agents/skills" / name).exists():
                    continue
                elif not entry.is_dir():
                    continue
                self.add(f"{base}/{name}", "orphaned-generated-output")

    def empty_dirs(self) -> None:
        def is_empty(path: Path) -> bool:
            try:
                entries = list(os.scandir(path))
            except OSError:
                return False
            return all(e.is_dir(follow_symlinks=False) and e.name not in SKIP_DIRS and is_empty(Path(e.path)) for e in entries)

        for current, dirs, _ in os.walk(self.root, followlinks=False):
            rel_dir = Path(current).relative_to(self.root).as_posix()
            rel_dir = "" if rel_dir == "." else rel_dir
            keep = []
            for name in sorted(dirs):
                rel = f"{rel_dir}/{name}".lstrip("/")
                path = self.root / rel
                if name in SKIP_DIRS or path.is_symlink() or under(rel, "specs") or under(rel, "knowledgebase"):
                    continue
                if rel in self.found:
                    continue
                if name in MANAGED_DIRS and not rel_dir:
                    keep.append(name)
                    continue
                if is_empty(path):
                    if not self.documented(rel) and not self.is_ignored(rel):
                        self.add(rel, "empty-directory")
                    continue
                keep.append(name)
            dirs[:] = keep

    def unrecognized(self) -> None:
        for base, known in (("", KNOWN_ROOT), (".agents", KNOWN_AGENTS_DIR)):
            directory = self.root / base if base else self.root
            if not directory.is_dir():
                continue
            for entry in sorted(directory.iterdir()):
                name = entry.name
                rel = f"{base}/{name}".lstrip("/")
                if base == "" and (
                    name in known or name in MANAGED_DIRS or any(fnmatch.fnmatch(name, p) for p in KNOWN_ROOT_PATTERNS)
                ):
                    continue
                if base == ".agents" and name in known:
                    continue
                if rel in self.found or name.endswith(".bak") or name in SKIP_DIRS:
                    continue
                is_dir = entry.is_dir() and not entry.is_symlink()
                if self.is_tracked(rel, is_dir) or self.is_ignored(rel) or self.documented(rel):
                    continue
                if is_dir and not any(entry.iterdir()):
                    continue
                self.add(rel, "unrecognized-file", needs_review=True)


def load_root(arg: str | None) -> Path:
    root = Path(arg) if arg else Path.cwd()
    if not root.is_dir():
        fail("--root must be a directory")
    return root.resolve()


def write_state(root: Path, payload: dict[str, Any]) -> None:
    state_dir = root / STATE_DIR
    if state_dir.is_symlink():
        fail(f"refusing symlinked {STATE_DIR}", 1)
    state_dir.mkdir(mode=0o700, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=str(state_dir), prefix=".scan-", suffix=".tmp")
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            json.dump(payload, handle)
        os.replace(tmp, state_dir / STATE_FILE)
    except BaseException:
        if os.path.exists(tmp):
            os.unlink(tmp)
        raise


def cmd_scan(root: Path, stale_days: int) -> None:
    ws = Workspace(root, stale_days)
    ws.walk()
    ws.check_generated()
    ws.empty_dirs()
    ws.unrecognized()
    candidates = sorted(ws.found.values(), key=lambda c: (c["category"], c["path"]))
    counts: dict[str, int] = {}
    for item in candidates:
        counts[item["category"]] = counts.get(item["category"], 0) + 1
    scanned_at = datetime.now(timezone.utc).isoformat(timespec="seconds")
    write_state(root, {"scanned_at": scanned_at, "candidates": candidates})
    shown = [{k: v for k, v in c.items() if k != "mtime_ns"} for c in candidates]
    emit({"root": str(root), "scanned_at": scanned_at, "counts": counts, "candidates": shown})


def load_state(root: Path) -> dict[str, dict[str, Any]]:
    path = root / STATE_DIR / STATE_FILE
    try:
        if path.is_symlink() or path.stat().st_size > MAX_STATE_BYTES:
            raise ValueError
        data = json.loads(path.read_text(encoding="utf-8"))
        entries = {c["path"]: c for c in data["candidates"] if isinstance(c, dict) and isinstance(c.get("path"), str)}
    except (OSError, ValueError, KeyError, TypeError):
        fail("no usable saved scan: run a scan first", 1)
    return entries


def refusal(rel: str, entry: dict[str, Any] | None, root: Path) -> str | None:
    if entry is None:
        return "not in the last scan"
    parts = Path(rel).parts
    if Path(rel).is_absolute() or ".." in parts or not parts:
        return "path escapes the workspace"
    if parts[0] in (".git", STATE_DIR) or rel in ("AGENTS.md", "CLAUDE.md", "GEMINI.md"):
        return "protected path"
    if protected_from_scan(rel) or under(rel, "specs"):
        return "protected: user-owned content"
    if under(rel, "knowledgebase") and not (rel.endswith(".bak") and entry.get("kind") == "file"):
        return "protected: only *.bak files may be deleted under knowledgebase/"
    path = root / rel
    for parent in path.parents:
        if parent == root:
            break
        if parent.is_symlink():
            return "inside a symlinked directory"
    try:
        stat = path.lstat()
    except OSError:
        return "no longer exists"
    kind = "dir" if path.is_dir() and not path.is_symlink() else "file"
    if kind != entry.get("kind") or stat.st_mtime_ns != entry.get("mtime_ns"):
        return "changed since the scan"
    return None


def cmd_delete(root: Path, paths: list[str]) -> None:
    state = load_state(root)
    deleted: list[str] = []
    refused: list[dict[str, str]] = []
    for rel in paths:
        entry = state.get(rel)
        reason = refusal(rel, entry, root)
        if reason is None and entry is not None:
            path = root / rel
            try:
                if entry["kind"] == "dir":
                    if entry["category"] == "empty-directory":
                        os.rmdir(path)
                    else:
                        shutil.rmtree(path)
                else:
                    path.unlink()
            except OSError as exc:
                reason = f"could not delete: {exc.strerror}"
        if reason is None:
            deleted.append(rel)
            state.pop(rel, None)
        else:
            refused.append({"path": rel, "reason": reason})
    if deleted:
        write_state(root, {"scanned_at": datetime.now(timezone.utc).isoformat(timespec="seconds"),
                           "candidates": list(state.values())})
    emit({"deleted": deleted, "refused": refused})
    if refused:
        sys.exit(1)


def main() -> None:
    parser = argparse.ArgumentParser(description="Find files that may not belong in the workspace")
    parser.add_argument("--root", help="workspace root (default: cwd)")
    parser.add_argument("--stale-days", type=int, default=DEFAULT_STALE_DAYS)
    parser.add_argument("--delete", nargs="+", metavar="PATH")
    args = parser.parse_args()
    if args.stale_days < 1:
        fail("--stale-days must be at least 1")
    root = load_root(args.root)
    if args.delete:
        cmd_delete(root, args.delete)
    else:
        cmd_scan(root, args.stale_days)


if __name__ == "__main__":
    main()
